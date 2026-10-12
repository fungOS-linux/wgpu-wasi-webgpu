# wgpu-wasi-webgpu

wgpu's `custom` dispatch traits implemented over `wasi:webgpu@0.0.1`, the
WebGPU interface the ffrwd sidecar hosts. A program written against wgpu,
a renderer such as vello_hybrid or a compute kernel, builds for
`wasm32-wasip2` and runs in a component unchanged:
`wgpu_wasi_webgpu::instance()` is a `wgpu::Instance` whose adapter is the
host's GPU, and every call below it is a wit import the host answers.

On any target but wasm32 the crate holds only its byte counters and
`host_descriptor`, so a workspace that depends on it still builds and
tests natively.

## Using it

```toml
[dependencies]
wgpu-wasi-webgpu = { git = "https://github.com/imbcmdth/wgpu-wasi-webgpu" }
wgpu = { version = "29", default-features = false, features = ["wgsl"] }
```

```rust
let instance = wgpu_wasi_webgpu::instance();
let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;
// From here on it is ordinary wgpu.
```

Build the module with `cargo build --target wasm32-wasip2`. The crate
carries its own bindings, so the component imports
`wasi:webgpu/webgpu@0.0.1` without the module declaring any wit for it.
Run it under `ffrwd-wasm` with a `-gpu` grant naming the module; without
one the sidecar refuses to instantiate it:

```
ffrwd-wasm -gpu module.wasm --invoke module.wasm render '{}'
```

The host answers every call synchronously, so each future the crate hands
back is already complete: `pollster::block_on` returns at once, a
`map_async` callback has run before `map_async` returns, and `device.poll`
has nothing left to wait for.

## Textures the host hands over

A module that reads its frames as textures of the host's device, and
hands frames back as textures (ffrwd's `ffrwd:gpu` interface does both),
binds a world of its own that names `wasi:webgpu`. Mapping that interface
onto this crate's bindings with wit-bindgen's `with` makes its
`gpu-texture` this crate's type:

```rust
wit_bindgen::generate!({
    path: "wit",
    world: "ffrwd:gpu/gpu-module@0.1.0",
    with: {
        "wasi:webgpu/webgpu@0.0.1": wgpu_wasi_webgpu::wasi::webgpu::webgpu,
        "wasi:graphics-context/graphics-context@0.0.1": wgpu_wasi_webgpu::wasi::graphics_context::graphics_context,
        "wasi:io/poll@0.2.0": wgpu_wasi_webgpu::wasi::io::poll,
    },
});
```

Two functions cross between the resource and wgpu:

- `texture_from_host(texture, width, height, format)` wraps a
  `gpu-texture` the host handed over as a `wgpu::Texture` of the module's
  device, described by `host_descriptor`: 2D, one level, one sample, the
  size and format given (which must be the host's), and readable only
  (`TEXTURE_BINDING` and `COPY_SRC`). A module samples it, copies from it,
  or binds a view of it like any other texture. Dropping it drops the
  resource handle; the host's texture is the host's.
- `host_texture(texture)` takes the `gpu-texture` out of a `wgpu::Texture`
  the module made, for a wit function that takes ownership of one. The
  handle is spent: the `wgpu::Texture`, its clones and its views are
  not to be used in commands again, and dropping them calls nothing on the
  host. Handing the same texture over twice panics.

Neither copies anything: the texture stays where it is on the device.

## Scope

Every trait method that the 0.0.1 wit can express is implemented as a call
to it: instance, adapter (features, limits, info, WGSL language features),
device (buffers, textures and views, samplers, bind groups and their
layouts, pipeline layouts, WGSL shader modules with their compilation
messages, render and compute pipelines, query sets, command and render
bundle encoders, error scopes, `destroy`), queue (`write-buffer`,
`write-texture`, submit, `on-submitted-work-done`, the timestamp period),
buffer mapping in both directions including `mapped_at_creation`, the four
copies, `clear-buffer`, `resolve-query-set` and encoder timestamps, compute
passes (pipelines, bind groups with dynamic offsets, direct and indirect
dispatch, debug groups and markers, timestamp writes), and render passes
(vertex and index buffers, direct, indexed and indirect draws, viewport,
scissor, blend constant, stencil reference, occlusion queries, timestamp
writes, bundles). `multi_draw_indirect` and `multi_draw_indexed_indirect`
become one indirect draw per command, as wgpu's own WebGPU backend does.

wgpu's dispatch traits carry labels only in descriptors, and the crate
passes each one on; the wit's `label` and `set-label` functions have no
trait method that would call them.

One gap is wgpu's rather than the wit's: wgpu 29's
`RenderBundleEncoder::finish` is `unimplemented!()` for every custom
backend, and a `wgpu::RenderBundle` cannot be built outside wgpu. The crate
implements the bundle encoder, its `finish` and `execute-bundles`, but a
wgpu program cannot reach them until wgpu does.

## How the 0.0.1 wit shapes it

- **Mapping is a copy.** `get-mapped-range-get-with-copy` hands back a
  list, so a read mapping is that list and `get_mapped_range` borrows it.
  A write mapping (`mapped_at_creation`, or `map_async` with `Write`) is a
  zeroed buffer the program fills, not the buffer's current contents; when
  wgpu drops the mapped range, the whole range goes back through
  `get-mapped-range-set-with-copy`.
- **Uploads are copies.** `write-buffer-with-copy` and
  `write-texture-with-copy` take the bytes as a list, which the canonical
  ABI copies out of wasm memory and wgpu's staging copies again.
- **Bytes are counted.** Every list that crosses the wit is added to a
  counter: `wit_bytes()` reads the totals (up: writes and write mappings;
  down: read mappings) and `take_wit_bytes()` reads and clears them.

Two more things follow from the wit:

- A refused device panics with the host's message, since
  `RequestDeviceError` has no constructor outside wgpu. A host with no
  adapter is a `RequestAdapterError`. A WGSL error reaches
  `get_compilation_info` as a message with its line; the host also prints
  it as an uncaptured error, which the 0.0.1 wit offers the guest only as
  a pollable, so `on_uncaptured_error` handlers are not called.
- The wit names the adapter but not the host's backend, so
  `AdapterInfo::backend` is `BrowserWebGpu`, the nearest wgpu has to
  WebGPU through a host.

## Not in the interface

These trait methods have no function in `wasi:webgpu@0.0.1` to call. Each
panics with the method's name and says so; `create_surface` panics too,
because `CreateSurfaceError` has no constructor outside wgpu.

- `Instance::create_surface`
- `Device::create_shader_module` with SPIR-V, GLSL or naga IR source (the
  wit takes WGSL only)
- `Device::create_shader_module_passthrough`
- `Device::create_mesh_pipeline`
- `Device::create_pipeline_cache`
- `Device::create_external_texture`
- `Device::create_blas`, `Device::create_tlas`, `Queue::compact_blas`,
  `CommandEncoder::build_acceleration_structures` and
  `CommandEncoder::mark_acceleration_structures_built`
- `Device::create_bind_group` with a binding array, an acceleration
  structure or an external texture, and a bind group layout entry with a
  `count`
- `Device::create_render_bundle_encoder` with `multiview`
- `CommandEncoder::clear_texture`
- `set_immediates` on compute passes, render passes and bundle encoders
- `write_timestamp` inside compute and render passes (a pass descriptor's
  `timestamp_writes` work, as does `CommandEncoder::write_timestamp`)
- `begin_pipeline_statistics_query` and `end_pipeline_statistics_query`
  on compute and render passes
- `RenderPass::draw_mesh_tasks`, `draw_mesh_tasks_indirect`,
  `multi_draw_mesh_tasks_indirect` and
  `multi_draw_mesh_tasks_indirect_count`
- `RenderPass::multi_draw_indirect_count` and
  `multi_draw_indexed_indirect_count`

Values that are wgpu extensions with no WebGPU name panic the same way
when they are converted: the 16-bit normalized texture formats, `R64Uint`,
the multi-planar video formats and HDR ASTC; the plane texture aspects;
`AddressMode::ClampToBorder`; `StorageTextureAccess::Atomic`; the vertex
formats WebGPU leaves out, such as `Float64`; and
`QueryType::PipelineStatistics`.

A few methods do nothing because the host has nothing to report:
`set_device_lost_callback` (the host does not answer `lost`),
`on_uncaptured_error` (above), `transition_resources`, and the graphics
debugger captures.

## wgpu's `webgpu` feature

On a wasm32 target, wgpu's build script turns on its browser backend
whenever the `webgpu` feature is set. wgpu 29's `build.rs` has

```
webgpu: { all(not(native), not(Emscripten), feature = "webgpu") },
```

with no exception for WASI, and `webgpu` is one of wgpu's default
features. The browser backend reaches the browser through wasm-bindgen
imports, which a wasm32-wasip2 component cannot carry.

This crate takes wgpu with its default features off, so it turns on none of
them. Cargo unifies features across a build, though, and a renderer crate
that depends on wgpu with its defaults turns `webgpu` on for every crate in
it: anyrender_vello_hybrid 0.10 does, and vello_hybrid 0.1 does through its
`wgpu_default` feature (which its own defaults include, so take it with
`default-features = false`). No feature of this crate can turn another
crate's feature off. A module in that position needs a copy of wgpu with
`webgpu` removed from its default features, patched in with
`[patch.crates-io]`, until wgpu leaves `target_os = "wasi"` out of that
cfg upstream. The test modules here take vello_hybrid with
`default-features = false` and need no patch.

## The wit

`wit/deps/` holds the wit the crate binds, so it builds from its own tree:
`webgpu`, `graphics-context` (which webgpu's types name) and `wasi:io`.
They are copied from ffrwd-cli's `sidecar/vendor/wasi-webgpu-wasmtime/wit/deps`
at commit `705e402`, which also ships the same files in the `ffrwd/wasm`
package under `wit/deps`. `wit/world.wit` is the crate's own world, which
imports the webgpu interface.

The package keeps the upstream name, `wasi:webgpu@0.0.1`, and is a superset
of it: the sidecar's host adds `gpu-command-encoder.write-timestamp` (wgpu's
`TIMESTAMP_QUERY_INSIDE_ENCODERS`, which the host grants with
`timestamp-query`) and `gpu-queue.get-timestamp-period`, without which a
resolved timestamp is in unknown units. A module built against the
proposal's own files links against the host unchanged, and one built with
this crate may call the two additions.

The copy here and the one in ffrwd-cli must stay identical, or the guest
and the host disagree about the interface.
`scripts/refresh-wit.sh` copies them from an ffrwd-cli checkout beside this
one (or the one it is given) and shows the diff; `--check` compares
without copying. It also refreshes `tests/modules/wit/av.wit`, the
`ffrwd:av` world the test modules export.

## Versions

The crate's version follows wgpu's: 29.x.y implements wgpu 29's dispatch
traits, and its minor and patch move on their own. A wgpu major changes
those traits, so it gets a matching major of this crate.

## Testing

```
cargo test
```

runs the native unit tests and `tests/gpu.rs`. The GPU tests build the
value modules under `tests/modules` (a cargo workspace of its own) for
wasm32-wasip2 and run each through `ffrwd-wasm --invoke` with a `-gpu`
grant:

- `hello-compute` is wgpu's hello-compute example. It counts the Collatz
  steps of each number it is given and must answer `[1, 2, 3, 4]` with
  `[0, 1, 7, 2]`, the assertion wgpu's own test of the example makes.
- `hello-triangle` draws wgpu's hello-triangle shader into a 256x256
  rgba8 texture and reads it back. The test checks sampled pixels and the
  image's sha256 against a hash pinned per adapter in `tests/gpu.rs`; an
  adapter with no pin gets the sample checks and a printed hash to add.
- `vello-scenes` renders a scene (a filled circle, a stroked rectangle and
  a gradient-filled path) with vello_hybrid at 512x512. The test renders
  the same scene with vello_cpu and requires every channel of all but
  0.1% of the pixels to be within 2 levels, and none beyond 8.

The tests find the sidecar through `FFRWD_WASM`, or else at
`../ffrwd-cli/sidecar/target/release/ffrwd-wasm` beside this repository.
With neither they print `SKIPPED` and pass, which is what happens in CI.
Each writes the image it checked to `tests/out/`, which git ignores.

## License

MIT.
