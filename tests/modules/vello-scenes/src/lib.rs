//! vello_hybrid through wgpu-wasi-webgpu, as a value module: the scene in
//! the `scene` crate rendered into a 512x512 rgba8 texture and read back.
//!
//! `render {}` returns the image as a PNG, base64. A value module has no
//! filesystem, so the bytes travel in the JSON.

wit_bindgen::generate!({
    path: "../wit/av.wit",
    world: "values-module",
});

use base64::Engine;
use exports::ffrwd::av::values::{FunctionMeta, Guest};
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Resources, Scene, TextureBindings};

fn render() -> Result<(Vec<u8>, u32, u32), String> {
    let (width, height) = (u32::from(scene::WIDTH), u32::from(scene::HEIGHT));
    let instance = wgpu_wasi_webgpu::instance();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .map_err(|e| format!("request-adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
        .map_err(|e| format!("request-device: {e}"))?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width,
            height,
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
    // 512 pixels of 4 bytes is 2048, already a multiple of the row alignment.
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(width * height * 4),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut vello_scene = Scene::new(scene::WIDTH, scene::HEIGHT);
    scene::draw!(vello_scene);
    let mut renderer = Renderer::new(
        &device,
        &RenderTargetConfig {
            format,
            width,
            height,
        },
    );
    let mut resources = Resources::new();

    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .render(
            &vello_scene,
            &mut resources,
            &device,
            &queue,
            &mut encoder,
            &RenderSize { width, height },
            &view,
            &TextureBindings::new(),
        )
        .map_err(|e| format!("render: {e}"))?;
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
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
    Ok((pixels, width, height))
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

struct VelloScenes;

impl Guest for VelloScenes {
    fn list_functions() -> Vec<FunctionMeta> {
        vec![FunctionMeta {
            name: "render".to_string(),
            params_schema: r#"{"type":"object","properties":{},"additionalProperties":false}"#
                .to_string(),
            result_schema: r#"{"type":"object","properties":{"width":{"type":"integer"},"height":{"type":"integer"},"png":{"type":"string"}},"required":["width","height","png"]}"#.to_string(),
        }]
    }

    fn invoke(name: String, _args: String) -> Result<String, String> {
        if name != "render" {
            return Err(format!("vello-scenes has no function {name}"));
        }
        let (rgba, width, height) = render()?;
        let png = base64::engine::general_purpose::STANDARD.encode(png(&rgba, width, height)?);
        Ok(serde_json::json!({ "width": width, "height": height, "png": png }).to_string())
    }
}

export!(VelloScenes);
