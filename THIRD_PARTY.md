# Third-party components

- `third_party/usd-brep`: NVIDIA USD BRep, pinned by Git submodule; Apache-2.0 with exceptions and dependency notices in its LICENSE and THIRD_PARTY_NOTICES.md. Sources are unmodified. The standalone build links SMLib and SM_API only.
- `third_party/json`: nlohmann/json v3.12.0, unmodified single header, MIT, full notice in LICENSE.MIT (used by the service, not the kernel probe).

The original project code is licensed under the MIT License; see `LICENSE`. The MIT license does not
cover the third-party components listed above, which remain under their own licenses. Do not infer
that the project license overrides third-party terms. Include applicable notices when distributing binaries.


Rust P2 dependencies are locked in Cargo.lock: serde 1.0.229 and serde_json 1.0.154 (MIT OR Apache-2.0),
Tokio 1.53.3 (MIT), plus their transitive dependencies. Crate license metadata was checked locally.
The lockfile is the version inventory; preserve the applicable crate notices when packaging Rust binaries.
Rust 1.90.0 is the pinned development toolchain, not an application runtime dependency.

P3 enables Tokio networking/signals; socket2 0.6.5 (MIT OR Apache-2.0) is newly locked.
Its local crate license metadata was checked; the service singleton uses std::fs file locking without a new lock dependency.

P4 adds png 0.17.16 and base64 0.22.1 (both MIT OR Apache-2.0), plus their locked compression dependencies.
Local crate metadata and license files were checked. scripts/package.py copies available dependency license/notice files,
upstream SMLib/JSON notices, Cargo.lock and a name/version/license inventory into the native bundle.
The Rust renderer encodes PNGs; no Python renderer or interpreter is included in the product package.

P5 pins egui/eframe 0.33.3 and wgpu 27.0.1 (MIT OR Apache-2.0; metadata MSRV 1.88, compatible with Rust 1.90).
The native host enables Vulkan/GLES explicitly. Local metadata and package notice files were inspected.
The packager also includes nested notices and epaint_default_fonts font license texts (Hack, Ubuntu/UFL, Noto/OFL, emoji icon font).
GUI test Xvfb/XTest/ffmpeg are internal verification tools, not bundled product runtimes.

Some egui crate archives omit their workspace-root license files. Unmodified upstream 0.33.3 license texts are retained in third_party/notices/egui-0.33.3 and copied into the bundle.

P6 adds axum 0.8.9 / tower-http 0.6.11 (MIT), wasm-bindgen 0.2.129 / js-sys / web-sys (MIT OR Apache-2.0),
gloo-net 0.6.0 / gloo-timers 0.3.0 (MIT OR Apache-2.0), futures-channel/util and locked HTTP dependencies.
The packager unions native and wasm32 dependency inventories and copies available notices; Web uses eframe's glow/WebGL path.
Playwright 1.56.1 and its Chromium fallback are internal browser test tools, not shipped application runtimes.

P7 reuses pinned wgpu 27.0.1 for headless compute passes, pollster 0.4.0, bytemuck 1.25.2 (MIT OR Apache-2.0),
and flate2 1.1.10 for bounded frame compression. These dependencies were already present transitively and are now used directly by cad-render.
The bundle retains native/WASM crate notices and WGSL source belongs to this project; no third-party kernel source was modified.

- Droid Sans Fallback (`assets/fonts/DroidSansFallbackFull.ttf`), Google Corp., Apache-2.0. Bundled for Chinese Agent conversations. Copyright notice and license: `third_party/notices/droid-sans/`. Original unmodified font from the installed Debian `fonts-droid-fallback` package.
