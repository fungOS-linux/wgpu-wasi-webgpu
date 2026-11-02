//! wgpu over `wasi:webgpu@0.0.1`: an implementation of wgpu's `custom`
//! dispatch traits whose every call is a wit import the ffrwd sidecar
//! answers, so a renderer written against wgpu runs in a wasm32-wasip2
//! component unchanged. `instance()` is the entry: a `wgpu::Instance` whose
//! adapters are the host's.
//!
//! Every trait method the 0.0.1 wit can express is a call to it. The rest
//! (surfaces, acceleration structures, mesh shaders, binding arrays,
//! pipeline caches, passthrough shaders, immediates, the indirect-count
//! draws, timestamps and statistics inside passes, `clear_texture`) panic
//! with the method's name; the README lists them.
//!
//! The bytes that cross the wit are counted: `wit_bytes()` reads the
//! totals, `take_wit_bytes()` reads and clears them.
//!
//! A texture can cross between the module and the host as a wit resource:
//! `texture_from_host` wraps a `gpu-texture` the host handed over as a
//! `wgpu::Texture`, and `host_texture` takes the resource out of a
//! `wgpu::Texture` the module made, to hand it over. `wasi` is the bindings
//! the crate imports, for a module's own world to map onto.
//!
//! On any target but wasm32 the crate is the counters alone, so a workspace
//! that holds it builds natively.

/// Bytes that crossed the wit since the counters were last cleared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WitBytes {
    /// Into the host: `write-buffer-with-copy`, `write-texture-with-copy`
    /// and `get-mapped-range-set-with-copy`.
    pub up: u64,
    /// Out of the host: `get-mapped-range-get-with-copy`.
    pub down: u64,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod counters {
    use std::cell::Cell;

    thread_local! {
        static UP: Cell<u64> = const { Cell::new(0) };
        static DOWN: Cell<u64> = const { Cell::new(0) };
    }

    pub fn up(n: usize) {
        UP.with(|c| c.set(c.get() + n as u64));
    }

    pub fn down(n: usize) {
        DOWN.with(|c| c.set(c.get() + n as u64));
    }

    pub fn read(clear: bool) -> super::WitBytes {
        let up = UP.with(|c| {
            let v = c.get();
            if clear {
                c.set(0);
            }
            v
        });
        let down = DOWN.with(|c| {
            let v = c.get();
            if clear {
                c.set(0);
            }
            v
        });
        super::WitBytes { up, down }
    }
}

/// Drive a future to completion without a platform thread park.
///
/// The host answers synchronously, so a pending poll is a short host-side
/// wait. `pollster` uses a Condvar, which panics on wasm32-wasip2.
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::hint::spin_loop(),
        }
    }
}

/// The bytes that crossed the wit so far.
pub fn wit_bytes() -> WitBytes {
    counters::read(false)
}

/// The bytes that crossed the wit so far, clearing the counters.
pub fn take_wit_bytes() -> WitBytes {
    counters::read(true)
}

#[cfg(target_arch = "wasm32")]
mod bindings {
    // `wit/deps` is the wit the ffrwd sidecar hosts; see the README.
    wit_bindgen::generate!({
        path: "wit",
        world: "ffrwd:wgpu-wasi-webgpu/guest",
        generate_all,
    });
}

#[cfg(target_arch = "wasm32")]
mod backend;
#[cfg(target_arch = "wasm32")]
mod conv;

#[cfg(target_arch = "wasm32")]
pub use backend::{host_texture, instance, texture_from_host};

/// The wit bindings the crate imports. A module that binds a world of its
/// own naming `wasi:webgpu` (one that hands textures to and from the host)
/// maps these interfaces onto it with wit-bindgen's `with`, so that its
/// `gpu-texture` is this crate's and `texture_from_host` and
/// `host_texture` take and give it:
///
/// ```ignore
/// with: {
///     "wasi:webgpu/webgpu@0.0.1": wgpu_wasi_webgpu::wasi::webgpu::webgpu,
///     "wasi:graphics-context/graphics-context@0.0.1": wgpu_wasi_webgpu::wasi::graphics_context::graphics_context,
///     "wasi:io/poll@0.2.0": wgpu_wasi_webgpu::wasi::io::poll,
/// },
/// ```
#[cfg(target_arch = "wasm32")]
pub use bindings::wasi;

/// What `texture_from_host` says a host's texture is: 2D, `width` x
/// `height`, one level and one sample, in `format`, and readable only
/// (`TEXTURE_BINDING` and `COPY_SRC`), as a frame the host lends a module
/// for one call is.
pub fn host_descriptor(
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_texture_is_described_as_readable_and_its_own_size() {
        let d = host_descriptor(1920, 1080, wgpu::TextureFormat::R8Unorm);
        assert_eq!(
            d.size,
            wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1
            }
        );
        assert_eq!(d.format, wgpu::TextureFormat::R8Unorm);
        assert_eq!((d.mip_level_count, d.sample_count), (1, 1));
        assert_eq!(d.dimension, wgpu::TextureDimension::D2);
        assert_eq!(
            d.usage,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC
        );
        assert!(!d.usage.contains(wgpu::TextureUsages::RENDER_ATTACHMENT));
        assert!(!d.usage.contains(wgpu::TextureUsages::COPY_DST));
        assert!(d.view_formats.is_empty());
    }

    #[test]
    fn counters_add_up_and_take_clears_them() {
        take_wit_bytes();
        counters::up(3);
        counters::up(4);
        counters::down(5);
        assert_eq!(wit_bytes(), WitBytes { up: 7, down: 5 });
        assert_eq!(take_wit_bytes(), WitBytes { up: 7, down: 5 });
        assert_eq!(wit_bytes(), WitBytes::default());
    }
}
