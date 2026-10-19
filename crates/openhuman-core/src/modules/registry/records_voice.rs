//! Registry record for the `tinyvoice` module.

#[cfg(feature = "voice")]
use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

#[cfg(feature = "voice")]
/// The `tinyvoice` module: the host-agnostic half of the voice pipeline.
///
/// STT hallucination detection used by the voice transcription pipeline.
///
/// Lazy: transcript screening loads the module only when voice transcription
/// needs it.
pub(crate) const TINYVOICE: ModuleRecord = ModuleRecord {
    id: "tinyvoice",
    description: "Voice transcript hallucination detection",
    bus_name: tinyvoice_bus::names::BUS_NAME,
    object_path: tinyvoice_bus::names::OBJECT_PATH,
    version: "0.3.2",
    release_url: "https://github.com/tinyhumansai/tinyvoice/releases/tag/v0.3.2",
    assets: &[
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyvoice-module-0.3.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "6ee278898d33f3b3ca7a9bf8a313dc4ab4e207eabf970608d37f56d825587519",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyvoice-module-0.3.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "f2145cd4bf729b5b6ad563687b061557d502c786ba935a65433e4338adf75a7a",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyvoice-module-0.3.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "d173a63a6b20e0441d18d639906ec9c74f96178c50e274ebd32bf94abbf5260b",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyvoice-module-0.3.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "533a3715f5efad9911e7768e8280bbe724c60785e0ff67939867ae21a20f690e",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyvoice-module-0.3.2-macos-26-arm64.tar.gz",
            sha256: "2a4ee8f87229422019adfd28f07ac26d5e2e78165550b04f0e563764a66a429e",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyvoice-module-0.3.2-macos-26-x86_64.tar.gz",
            sha256: "4199913e36c12987a82606f98c93e2a85e086aaa17380469a9aa68b719c853d9",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyvoice-module-0.3.2-macos-15-arm64.tar.gz",
            sha256: "316cb2b9b9a1630f2c815744121e3bbc1d0df58364bb012dd6f11f78978ba947",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyvoice-module-0.3.2-macos-15-x86_64.tar.gz",
            sha256: "51045754d67386db5b09a19714ec8b1f319ad9896bb3f936325d7935ce5dce0e",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyvoice-module-0.3.2-windows-2025-x86_64.zip",
            sha256: "04cf6b21ed7d73375181c6cf5a18fa52a50fb09049031f0926cd46c3bcf33431",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyvoice-module-0.3.2-windows-2022-x86_64.zip",
            sha256: "7b7a8a8e04a57adc306b2b840bff43ea6a27090e2b089b74cfcf451f18172456",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyvoice-module-0.3.2-windows-11-arm64.zip",
            sha256: "b3cbff9b477495027839b0266201299b0db5cb585a9e41b3110cf46f16c35c99",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyvoice",
        manifest: "crates/tinyvoice-module/Cargo.toml",
        package: "tinyvoice-module",
        lib_name: "tinyvoice_module",
        features: &[],
    },
};
