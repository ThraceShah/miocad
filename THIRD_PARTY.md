# Third-party components

- `third_party/usd-brep`: NVIDIA USD BRep, pinned by Git submodule; Apache-2.0 with exceptions and dependency notices in its LICENSE and THIRD_PARTY_NOTICES.md. Sources are unmodified. The standalone build links SMLib and SM_API only.
- `third_party/json`: nlohmann/json v3.12.0, unmodified single header, MIT, full notice in LICENSE.MIT (used by the service, not the kernel probe).

This project has not selected its own distribution license yet. Do not infer that the third-party licenses license the original project code. Include applicable notices when distributing binaries.


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
