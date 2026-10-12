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
pub use backend::instance;

#[cfg(test)]
mod tests {
    use super::*;

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
