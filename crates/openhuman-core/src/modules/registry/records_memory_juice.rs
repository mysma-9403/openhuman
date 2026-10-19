//! Registry record for the `tinyjuice` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

pub(crate) const TINYJUICE: ModuleRecord = ModuleRecord {
    id: "tinyjuice",
    description: "Content-aware tool-output compression and recoverable caching",
    bus_name: tinyjuice_bus::names::BUS_NAME,
    object_path: tinyjuice_bus::names::OBJECT_PATH,
    version: "0.10.8",
    release_url: "https://github.com/tinyhumansai/tinyjuice/releases/tag/v0.10.8",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinyjuice-module-0.10.8-archlinux-arm64.tar.gz",
            sha256: "7aae28a8cd74693e82c55930212097d82f68b712b51004910a17cd970ef3005e",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinyjuice-module-0.10.8-archlinux-x86_64.tar.gz",
            sha256: "018bb2406270bd8d4e81771dca9093daf17b6151ceb143dbc0c1fbadf355e68a",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinyjuice-module-0.10.8-fedora-43-arm64.tar.gz",
            sha256: "f9c2e860ebce568877ee586cf36bfe4f3ba8160a3cc5dc8a239b4217afacbb25",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinyjuice-module-0.10.8-fedora-43-x86_64.tar.gz",
            sha256: "48a8a1f171f0430b7f9d8112a0c9817e03f7c3d9e66a34a4d289223cedbea0e5",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinyjuice-module-0.10.8-fedora-44-arm64.tar.gz",
            sha256: "052f0a355be7675ed2b90deccf361bd828fd0537090960eda0a958c2ba479727",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinyjuice-module-0.10.8-fedora-44-x86_64.tar.gz",
            sha256: "943cf44a3122ec58014d1ad8bff1d389bfa8a04c1bac347da72faeb727e6775f",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyjuice-module-0.10.8-macos-15-arm64.tar.gz",
            sha256: "a5118bbb08caea18cac9680b676ef12a961a8d162f8f2260b3e07797bd86f531",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyjuice-module-0.10.8-macos-15-x86_64.tar.gz",
            sha256: "443ceadda4d9a48511896aafa8abdaa446987733d26ff3708d3813d8e9cb99c2",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyjuice-module-0.10.8-macos-26-arm64.tar.gz",
            sha256: "58147c1369db5b2dedf6a5e6ade89f728f03eee23bf726325f16728574020296",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyjuice-module-0.10.8-macos-26-x86_64.tar.gz",
            sha256: "96b2cdc4dc3b15614a106eab50ebf87d3dbdf13c27c0e57b0108743f24dd5fb1",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyjuice-module-0.10.8-ubuntu-22.04-arm64.tar.gz",
            sha256: "1c67eab4449322a4fb90d9e4a8dbd36279af8997f6851c548378ea1091ab9376",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyjuice-module-0.10.8-ubuntu-22.04-x86_64.tar.gz",
            sha256: "b3bb5f2c0da0b43f7f79ddb3a9b56ca6a7c3bd2512230e4c98dddd9c33ee5cf2",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyjuice-module-0.10.8-ubuntu-24.04-arm64.tar.gz",
            sha256: "ffb4c4232ee80f51f9d118acf1b3cc01038545857ce319f9051803a052e94d50",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyjuice-module-0.10.8-ubuntu-24.04-x86_64.tar.gz",
            sha256: "ba680bce014149750112c599823ed592f6bf4f3a1be4d2a95524c2904935081e",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyjuice-module-0.10.8-windows-11-arm64.zip",
            sha256: "1c1f2227cdb792f158fa17602c9d2e8c19213a04c88a51bd284e2a3e3643f6ab",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyjuice-module-0.10.8-windows-2022-x86_64.zip",
            sha256: "f5b5c8d6e7e57a1af85518d9eaba30bf2247f89ecc54211d89188f4b35588264",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyjuice-module-0.10.8-windows-2025-x86_64.zip",
            sha256: "d171657ce10bf9e8a70dd2b3fdb7a6304c4796d3c25e7c8671afccd1e68580b9",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyjuice",
        manifest: "crates/tinyjuice-module/Cargo.toml",
        package: "tinyjuice-module",
        lib_name: "tinyjuice_module",
        features: &[],
    },
};
