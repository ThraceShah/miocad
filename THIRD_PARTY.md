# Third-party components

- `third_party/usd-brep`: NVIDIA USD BRep, pinned by Git submodule; Apache-2.0 with exceptions and dependency notices in its LICENSE and THIRD_PARTY_NOTICES.md. Sources are unmodified. The standalone build links SMLib and SM_API only.
- `third_party/json`: nlohmann/json v3.12.0, unmodified single header, MIT, full notice in LICENSE.MIT (used by the service, not the kernel probe).

- Optional OCCT backend: Open CASCADE Technology 7.9.2, dynamically linked system/local SDK, LGPL-2.1 with special exception. Unmodified upstream license texts are kept in `third_party/occt/`; source is available at https://github.com/Open-Cascade-SAS/OCCT/tree/V7_9_2. Retain OCCT copyright and license notices and comply with the applicable library/source/relinking obligations when distributing linked binaries. This is separate from the SMLib Apache-2.0 license.
- The local Debian/Ubuntu SDK bootstrap records exact binary package versions and SHA256 in `build/occt-sdk/packages.json`. Distribution-packaged STEP toolkits have transitive OCAF/visualization runtime dependencies, and TBB (Apache-2.0) is extracted as a runtime dependency. Include the applicable dependency copyright/license files from the SDK's `usr/share/doc` when distributing those libraries. The SDK and binary packages are not committed here.

This project has not selected its own distribution license yet. Do not infer that the third-party licenses license the original project code. Include applicable notices when distributing binaries.
