//! wgpu's hello-compute example as a value module: the Collatz step count
//! of each number, computed on the host's GPU through wgpu-wasi-webgpu.
//!
//! `collatz {"numbers": [1, 2, 3, 4]}` returns `{"steps": [0, 1, 7, 2]}`.
//! A number whose sequence overflows a u32 counts 4294967295 steps.

wit_bindgen::generate!({
    path: "../wit/av.wit",
    world: "values-module",
});

use exports::ffrwd::av::values::{FunctionMeta, Guest};
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
@group(0) @binding(0)
var<storage, read_write> numbers: array<u32>;

// How many times n -> n / 2 (even) or n -> 3n + 1 (odd) runs before n is 1.
fn collatz_steps(start: u32) -> u32 {
    var n = start;
    var steps = 0u;
    loop {
        if n <= 1u {
            break;
        }
        if n % 2u == 0u {
            n = n / 2u;
        } else {
            // 3n + 1 would not fit in a u32.
            if n >= 1431655765u {
                return 4294967295u;
            }
            n = 3u * n + 1u;
        }
        steps = steps + 1u;
    }
    return steps;
}

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    numbers[id.x] = collatz_steps(numbers[id.x]);
}
"#;

fn collatz(numbers: &[u32]) -> Result<Vec<u32>, String> {
    if numbers.is_empty() {
        return Ok(Vec::new());
    }
    let instance = wgpu_wasi_webgpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .map_err(|e| format!("request-adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
        .map_err(|e| format!("request-device: {e}"))?;

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("collatz"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let bytes: Vec<u8> = numbers.iter().flat_map(|n| n.to_le_bytes()).collect();
    let size = bytes.len() as u64;
    // Filled through a mapping at creation.
    let storage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("numbers"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("steps"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("collatz"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("numbers"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: storage.as_entire_binding(),
        }],
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("collatz"),
            timestamp_writes: None,
        });
        pass.push_debug_group("collatz");
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.insert_debug_marker("dispatch");
        pass.dispatch_workgroups(numbers.len() as u32, 1, 1);
        pass.pop_debug_group();
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);

    staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("poll: {e}"))?;
    let steps = staging
        .slice(..)
        .get_mapped_range()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    staging.unmap();
    Ok(steps)
}

struct HelloCompute;

impl Guest for HelloCompute {
    fn list_functions() -> Vec<FunctionMeta> {
        vec![FunctionMeta {
            name: "collatz".to_string(),
            params_schema: r#"{"type":"object","properties":{"numbers":{"type":"array","items":{"type":"integer","minimum":0,"maximum":4294967295}}},"required":["numbers"],"additionalProperties":false}"#.to_string(),
            result_schema: r#"{"type":"object","properties":{"steps":{"type":"array","items":{"type":"integer"}}},"required":["steps"]}"#.to_string(),
        }]
    }

    fn invoke(name: String, args: String) -> Result<String, String> {
        if name != "collatz" {
            return Err(format!("hello-compute has no function {name}"));
        }
        let args: serde_json::Value =
            serde_json::from_str(&args).map_err(|e| format!("args: {e}"))?;
        let numbers: Vec<u32> = args["numbers"]
            .as_array()
            .ok_or("args: numbers must be an array")?
            .iter()
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| format!("args: {n} is not a u32"))
            })
            .collect::<Result<_, _>>()?;
        let steps = collatz(&numbers)?;
        Ok(serde_json::json!({ "steps": steps }).to_string())
    }
}

export!(HelloCompute);
