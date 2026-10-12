//! The crate on a real GPU: the value modules under `tests/modules`, built
//! for wasm32-wasip2 and run through the ffrwd sidecar's `--invoke` with a
//! `-gpu` grant.
//!
//! The sidecar is `FFRWD_WASM` when that is set, else the release build in
//! an ffrwd-cli checkout beside this one. With neither, every case here
//! skips, loudly, and passes.
//!
//! Each case writes the image it checked to `tests/out/` (not in git).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use base64::Engine;
use serde_json::Value;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The sidecar binary, or None to skip.
fn sidecar() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FFRWD_WASM") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "FFRWD_WASM names {}, which is not a file",
            path.display()
        );
        return Some(path);
    }
    let exe = if cfg!(windows) {
        "ffrwd-wasm.exe"
    } else {
        "ffrwd-wasm"
    };
    let sibling = root().join("../ffrwd-cli/sidecar/target/release").join(exe);
    if sibling.is_file() {
        return Some(sibling);
    }
    eprintln!(
        "SKIPPED: no ffrwd-wasm. Set FFRWD_WASM to the sidecar binary, or build \
         ../ffrwd-cli/sidecar with `cargo build --release`."
    );
    None
}

/// Builds every module for wasm32-wasip2, once per test binary, and
/// returns the directory they land in. `tests/modules` is a workspace of
/// its own, with its own build lock, so this does not wait on the
/// `cargo test` that runs it.
fn modules() -> PathBuf {
    static BUILT: OnceLock<()> = OnceLock::new();
    let dir = root().join("tests/modules");
    BUILT.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&dir)
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .expect("spawn cargo build for tests/modules");
        assert!(
            output.status.success(),
            "building tests/modules failed (status {:?}):\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    });
    dir.join("target/wasm32-wasip2/release")
}

/// `ffrwd-wasm -gpu <module> --invoke <module> <function> <args>`, its
/// stdout read as JSON.
fn invoke(sidecar: &Path, module: &str, function: &str, args: &str) -> Value {
    let module = modules().join(module);
    let output = Command::new(sidecar)
        .arg("-gpu")
        .arg(&module)
        .arg("--invoke")
        .arg(&module)
        .arg(function)
        .arg(args)
        .output()
        .expect("spawn ffrwd-wasm");
    assert!(
        output.status.success(),
        "{} {function} failed (status {:?}):\n{}",
        module.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{function} printed no JSON ({e}):\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// A PNG the module returned as base64, written to `tests/out/<name>` and
/// decoded to rgba bytes.
fn decode_png(base64: &Value, name: &str) -> (Vec<u8>, u32, u32) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.as_str().expect("png is a string"))
        .expect("png is base64");
    save(name, &bytes);
    let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes[..]))
        .read_info()
        .expect("a PNG");
    let mut rgba = vec![
        0;
        reader
            .output_buffer_size()
            .expect("a PNG that fits in memory")
    ];
    let info = reader.next_frame(&mut rgba).expect("a PNG frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    rgba.truncate(info.buffer_size());
    (rgba, info.width, info.height)
}

fn save(name: &str, bytes: &[u8]) {
    let out = root().join("tests/out");
    std::fs::create_dir_all(&out).expect("create tests/out");
    std::fs::write(out.join(name), bytes).expect("write to tests/out");
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
    writer.finish().unwrap();
    out
}

/// wgpu's hello-compute example, and the assertion wgpu's own test of it
/// makes, plus a number whose sequence overflows a u32.
#[test]
fn hello_compute_counts_collatz_steps() {
    let Some(sidecar) = sidecar() else { return };
    let result = invoke(
        &sidecar,
        "hello_compute.wasm",
        "collatz",
        r#"{"numbers":[1,2,3,4]}"#,
    );
    assert_eq!(result["steps"], serde_json::json!([0, 1, 7, 2]));

    let result = invoke(
        &sidecar,
        "hello_compute.wasm",
        "collatz",
        r#"{"numbers":[27,1431655765]}"#,
    );
    assert_eq!(result["steps"], serde_json::json!([111, u32::MAX]));
}

/// The hello-triangle image's sha256 per adapter, as `(adapter name, hash)`.
/// The triangle has no anti-aliasing, so other GPUs are likely to match;
/// an adapter with no entry here gets the sample checks only, and the test
/// prints its hash to add.
const TRIANGLE_PINS: &[(&str, &str)] = &[
    // Windows 11, the sidecar at ffrwd-cli 705e402 (wgpu-core 29).
    (
        "NVIDIA GeForce RTX 4090",
        "c03fa4fda0031923a759d2dd350c2de02af485d7786463e3b57c844188e3eae1",
    ),
];

/// wgpu's hello-triangle shader drawn into a texture and read back.
#[test]
fn hello_triangle_draws_a_red_triangle() {
    let Some(sidecar) = sidecar() else { return };
    let result = invoke(&sidecar, "hello_triangle.wasm", "render", "{}");
    let (rgba, width, height) = decode_png(&result["png"], "hello-triangle.png");
    assert_eq!((width, height), (256, 256));
    assert_eq!(rgba.len(), 256 * 256 * 4);

    let red = serde_json::json!([255, 0, 0, 255]);
    let clear = serde_json::json!([0, 0, 0, 0]);
    let samples = &result["samples"];
    assert_eq!(samples["128,128"], red, "the centre");
    assert_eq!(samples["128,250"], red, "near the bottom edge");
    for corner in ["0,0", "255,0", "0,128", "255,128"] {
        assert_eq!(samples[corner], clear, "{corner}, outside the triangle");
    }

    let adapter = result["adapter"].as_str().expect("adapter is a string");
    let sha256 = result["sha256"].as_str().expect("sha256 is a string");
    match TRIANGLE_PINS.iter().find(|(name, _)| *name == adapter) {
        Some((_, pinned)) => assert_eq!(sha256, *pinned, "the image on {adapter}"),
        None => eprintln!(
            "UNPINNED: {adapter} drew the triangle with sha256 {sha256}; add it to \
             TRIANGLE_PINS once tests/out/hello-triangle.png looks right"
        ),
    }
}

/// How far vello_hybrid on the GPU may stray from vello_cpu: within
/// `NEAR` levels on every channel for all but `FAR_SHARE` of the pixels,
/// and never beyond `MAX`.
const NEAR: u8 = 2;
const FAR_SHARE: f64 = 0.001;
const MAX: u8 = 8;

/// A vello_hybrid render through this crate against vello_cpu's render of
/// the same scene.
#[test]
fn vello_scenes_match_vello_cpu() {
    let Some(sidecar) = sidecar() else { return };
    let result = invoke(&sidecar, "vello_scenes.wasm", "render", "{}");
    let (gpu, width, height) = decode_png(&result["png"], "vello-scenes.png");
    assert_eq!(
        (width, height),
        (u32::from(scene::WIDTH), u32::from(scene::HEIGHT))
    );

    let mut context = vello_cpu::RenderContext::new(scene::WIDTH, scene::HEIGHT);
    scene::draw!(context);
    context.flush();
    let mut pixmap = vello_cpu::Pixmap::new(scene::WIDTH, scene::HEIGHT);
    context.render_with(
        &mut pixmap,
        &mut vello_cpu::Resources::new(),
        vello_cpu::RasterizerSettings {
            render_mode: vello_cpu::RenderMode::OptimizeQuality,
            ..Default::default()
        },
    );
    let cpu = pixmap.data_as_u8_slice();
    save("vello-cpu.png", &encode_png(cpu, width, height));
    assert_eq!(gpu.len(), cpu.len());

    let mut max = 0u8;
    let mut far = 0usize;
    for (g, c) in gpu.as_chunks::<4>().0.iter().zip(cpu.as_chunks::<4>().0) {
        let worst = g.iter().zip(c).map(|(g, c)| g.abs_diff(*c)).max().unwrap();
        max = max.max(worst);
        if worst > NEAR {
            far += 1;
        }
    }
    let pixels = (width * height) as usize;
    let share = far as f64 / pixels as f64;
    eprintln!(
        "vello_hybrid against vello_cpu: {far} of {pixels} pixels ({:.4}%) differ by more \
         than {NEAR} levels; the largest difference is {max}",
        share * 100.0
    );
    assert!(
        share <= FAR_SHARE,
        "{far} of {pixels} pixels differ by more than {NEAR} levels"
    );
    assert!(max <= MAX, "a pixel differs by {max} levels");
}
