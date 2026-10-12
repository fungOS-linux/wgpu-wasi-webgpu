//! wgpu's hello-triangle shader as a value module: one red triangle drawn
//! into a 256x256 rgba8 texture cleared to transparent black, read back
//! through a mapped buffer.
//!
//! `render {}` returns the image's sha256 (over its rgba bytes, rows top
//! to bottom), a few sampled pixels keyed `"x,y"`, the adapter's name, and
//! the image as a PNG, base64, for a test to write out.

wit_bindgen::generate!({
    path: "../wit/av.wit",
    world: "values-module",
});

use base64::Engine;
use exports::ffrwd::av::values::{FunctionMeta, Guest};
use sha2::Digest;

const SIZE: u32 = 256;

const SHADER: &str = r#"
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index) - 1);
    let y = f32(i32(index & 1u) * 2 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
"#;

/// The pixels the test samples: the centre, inside the triangle, and
/// points outside it near each top corner and halfway down each side.
const SAMPLES: &[(u32, u32)] = &[
    (128, 128),
    (0, 0),
    (255, 0),
    (0, 128),
    (255, 128),
    (128, 250),
];

/// The image's rgba bytes, and the name of the adapter that drew it.
fn render() -> Result<(Vec<u8>, String), String> {
    let instance = wgpu_wasi_webgpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .map_err(|e| format!("request-adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
        .map_err(|e| format!("request-device: {e}"))?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("triangle"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("triangle"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("triangle"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    // 256 pixels of 4 bytes is 1024, already a multiple of the row alignment.
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(SIZE * SIZE * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("triangle"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_viewport(0.0, 0.0, SIZE as f32, SIZE as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, SIZE, SIZE);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);

    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("poll: {e}"))?;
    let pixels = readback.slice(..).get_mapped_range().to_vec();
    readback.unmap();
    Ok((pixels, adapter.get_info().name))
}

fn png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

struct HelloTriangle;

impl Guest for HelloTriangle {
    fn list_functions() -> Vec<FunctionMeta> {
        vec![FunctionMeta {
            name: "render".to_string(),
            params_schema: r#"{"type":"object","properties":{},"additionalProperties":false}"#
                .to_string(),
            result_schema: r#"{"type":"object","properties":{"adapter":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"},"sha256":{"type":"string"},"samples":{"type":"object"},"png":{"type":"string"}},"required":["adapter","width","height","sha256","samples","png"]}"#.to_string(),
        }]
    }

    fn invoke(name: String, _args: String) -> Result<String, String> {
        if name != "render" {
            return Err(format!("hello-triangle has no function {name}"));
        }
        let (rgba, adapter) = render()?;
        let sha256: String = sha2::Sha256::digest(&rgba)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let samples: serde_json::Map<String, serde_json::Value> = SAMPLES
            .iter()
            .map(|&(x, y)| {
                let i = ((y * SIZE + x) * 4) as usize;
                (format!("{x},{y}"), serde_json::json!(&rgba[i..i + 4]))
            })
            .collect();
        let png = base64::engine::general_purpose::STANDARD.encode(png(&rgba, SIZE, SIZE)?);
        Ok(serde_json::json!({
            "adapter": adapter,
            "width": SIZE,
            "height": SIZE,
            "sha256": sha256,
            "samples": samples,
            "png": png,
        })
        .to_string())
    }
}

export!(HelloTriangle);
