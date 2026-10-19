//! Registry record for the `tinymemes` module.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

/// The `tinymemes` module: final-reply slang and meme remixing, loaded on
/// first use. The host serves the callbacks it asks for (inference, Jev,
/// search, index storage, delivery).
pub(crate) const TINYMEMES: ModuleRecord = ModuleRecord {
    id: "tinymemes",
    description: "Final-reply slang and meme remixing through TinyMemes",
    bus_name: tinymemes_bus::STATE_INTERFACE,
    object_path: tinymemes_bus::STATE_PATH,
    version: "0.4.1",
    release_url: "https://github.com/tinyhumansai/tinymemes/releases/tag/v0.4.1",
    assets: &[
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymemes-module-0.4.1-macos-15-arm64.tar.gz",
            sha256: "9ef332dfb479260567b4fbf12734d21d635cd6182b338474a5895cafa2093c19",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymemes-module-0.4.1-macos-15-x86_64.tar.gz",
            sha256: "dac379e4fa92152644b1e22191614e5424b58ad04cce9f3baadbb9c9e8463a56",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymemes-module-0.4.1-macos-26-arm64.tar.gz",
            sha256: "7023133d6f88bceea185850da3a9c52a9a0a1ce0f3a065e1095087ac3303b564",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymemes-module-0.4.1-macos-26-x86_64.tar.gz",
            sha256: "372eb95b30bc7ce3accc893b3a7cdbe6812bfd2a7bbe65c62c19f07ef7d8f10c",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymemes-module-0.4.1-ubuntu-22.04-arm64.tar.gz",
            sha256: "92dc4a49608aaca0a83f6c50da4a6da5e726b60665a9424441e40cb1d616f652",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymemes-module-0.4.1-ubuntu-22.04-x86_64.tar.gz",
            sha256: "55060d251d04b62683f0443994f9376b12295f3ac0848258a6faa202fffef781",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymemes-module-0.4.1-ubuntu-24.04-arm64.tar.gz",
            sha256: "c978dcd8e5ed355449785d94817cdaf009208cf4a1a4e7d8c8bc687bffcc0e40",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymemes-module-0.4.1-ubuntu-24.04-x86_64.tar.gz",
            sha256: "8612106d61a42c59a7e74e0476705285cf0ad917aa5b6ea26e86c05f4d666c76",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymemes-module-0.4.1-windows-11-arm64.zip",
            sha256: "bac64bbe575a7053232293051a6a45de6e4503d77a83da15de8b957123cf361b",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymemes-module-0.4.1-windows-2022-x86_64.zip",
            sha256: "a00ddc84ca3232e04613a658bd2766b0bbe2fda77d7b07285706cf65d1a85a03",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymemes-module-0.4.1-windows-2025-x86_64.zip",
            sha256: "8279337613b7984340ba3e0b1dfdfe17bd046e013b467152d21fa151933a1639",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinymemes",
        manifest: "crates/tinymemes-module/Cargo.toml",
        package: "tinymemes-module",
        lib_name: "tinymemes_module",
        features: &[],
    },
};
