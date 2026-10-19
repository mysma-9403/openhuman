//! Registry records for the `tinymcp` and `tinyconnectors` modules.

use crate::modules::types::{LoadPolicy, ModuleRecord, PlatformAsset, SourceSpec};

/// The `tinymcp` module: the Model Context Protocol client.
///
/// Owns both transports (Streamable HTTP and a subprocess over stdio), the
/// statically declared server set a host puts in its own configuration, the
/// dynamic registry of user-installed servers with its SQLite store, the
/// reconnect supervisor, the browser sign-in flow, and the write-audit log.
///
/// Lazy, because dialing an MCP server is something most sessions never do: a
/// host with no installed servers and no configured ones would otherwise pay a
/// download and a `dlopen` for a capability it never reaches. That differs from
/// the module's own `lazy = false` export hint, which speaks for a host whose
/// servers should be connected the moment it comes up — this host decides when
/// that moment is, and does so on the first ask.
///
/// **What stays out of the module is host policy**, and the split is the same
/// one the contract's own documentation draws: the prompt-injection scan over
/// remote tool definitions, the `mcp_clients` RPC surface, the
/// agent-facing tools, and the proxy *scoping* decision all belong to this
/// application's threat model, not to a protocol client. `tinymcp-bus` carries
/// the vocabulary; this table says which bytes may speak it.
pub(crate) const TINYMCP: ModuleRecord = ModuleRecord {
    id: "tinymcp",
    description: "Model Context Protocol client: transports, registry, and the write-audit log",
    bus_name: tinymcp_bus::names::INTERFACE,
    object_path: tinymcp_bus::names::OBJECT_PATH,
    version: "0.19.2",
    release_url: "https://github.com/tinyhumansai/tinymcp/releases/tag/v0.19.2",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-rolling-arm64",
            archive: "tinymcp-0.19.2-archlinux-arm64.tar.gz",
            sha256: "cb357041fa7a3815b913fbf235bb10e9ff4c475e3fbf921bc93d8fff644e55e2",
        },
        PlatformAsset {
            host_key: "archlinux-rolling-x86_64",
            archive: "tinymcp-0.19.2-archlinux-x86_64.tar.gz",
            sha256: "90bc66c70fb1fb3581ca382e9281fc28bc27c836ec1f65998413a4e8641d4c7e",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinymcp-0.19.2-fedora-43-arm64.tar.gz",
            sha256: "cc805cc004f6baec0adc62e1153f0368ef17180e816c96a895db1b17726d4a2c",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinymcp-0.19.2-fedora-43-x86_64.tar.gz",
            sha256: "3ad7ff3b83922d5cf195dfdea9377ddc6295d7d523467f34f3449df38667bc3d",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinymcp-0.19.2-fedora-44-arm64.tar.gz",
            sha256: "d4a50e75598c3886527abf8f628d40f96b5a86488df3a59d9bafa1b317cf7576",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinymcp-0.19.2-fedora-44-x86_64.tar.gz",
            sha256: "62b0fd7a25128af5922ca1a9a80599b4b16fa53e8646c0ee5ff67babd1cf3cd3",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinymcp-0.19.2-macos-15-arm64.tar.gz",
            sha256: "57cb298306eedc7c3149b1438b174fca2d73ce4bfe9ee68211887f9782c7a1c4",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinymcp-0.19.2-macos-15-x86_64.tar.gz",
            sha256: "8e9f0cfb7886fbf462238257f056f9d3a2309a94328d79643d4283682648f4dc",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinymcp-0.19.2-macos-26-arm64.tar.gz",
            sha256: "f566ef326b754d08b5579927a8df9c9c4db65cf0236468cdc520fce22163f843",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinymcp-0.19.2-macos-26-x86_64.tar.gz",
            sha256: "37e3e974af8cdabc40483a6e137480a6d0cf9d3623bc98e7eba6f41ef77bbbf7",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinymcp-0.19.2-ubuntu-22.04-arm64.tar.gz",
            sha256: "fe474bce2f4666d0e5c0ccfd6e582a33a9f170bbb8212e491c46fedc0ce5bbe3",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinymcp-0.19.2-ubuntu-22.04-x86_64.tar.gz",
            sha256: "9a60527d63a162e4ee4a49b28cc9e52d996e32cebcf354107ec5671759888e92",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinymcp-0.19.2-ubuntu-24.04-arm64.tar.gz",
            sha256: "33a9c5d5e1f54b1bd4bf6557695d59de414f7e07062bb052ec26807daa2b47e0",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinymcp-0.19.2-ubuntu-24.04-x86_64.tar.gz",
            sha256: "699dc226b2c3c4a91f99e6f456f8a24ec14caeb77b2cd08b2741d9dccabb38a5",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinymcp-0.19.2-windows-11-arm64.zip",
            sha256: "d1a067cf4e0bacec0645a949095124b86df6d967a0e01c3ddeb8ea1559becb49",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinymcp-0.19.2-windows-2022-x86_64.zip",
            sha256: "ff01a8e99c9e793248e1479ff4ffd79d5c97283589bbf3ad7c0e48bbcde55a93",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinymcp-0.19.2-windows-2025-x86_64.zip",
            sha256: "6a9f9c9c53e908d4868d7060502e4c5cd4ccc7ec6717df058aae14afdebe106e",
        },
    ],
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinymcp",
        manifest: "crates/tinymcp/Cargo.toml",
        package: "tinymcp",
        lib_name: "tinymcp",
        features: &[],
    },
};

pub(crate) const TINYCONNECTORS: ModuleRecord = ModuleRecord {
    id: "tinyconnectors",
    description: "OAuth connector integrations: accounts, actions, and triggers",
    bus_name: tinyconnectors_bus::names::INTERFACE,
    object_path: tinyconnectors_bus::names::OBJECT_PATH,
    version: "0.19.3",
    release_url: "https://github.com/tinyhumansai/tinyconnectors/releases/tag/v0.19.3",
    assets: &[
        PlatformAsset {
            host_key: "archlinux-arm64",
            archive: "tinyconnectors-0.19.3-archlinux-arm64.tar.gz",
            sha256: "1b223604c366769a67d0345d12da9b1b679722b54062157c68fba08b608f3ec9",
        },
        PlatformAsset {
            host_key: "archlinux-x86_64",
            archive: "tinyconnectors-0.19.3-archlinux-x86_64.tar.gz",
            sha256: "b7c83dbf1a4f40a343fe687708412f5b33c137cf384ebea5d8b9b6223d890f58",
        },
        PlatformAsset {
            host_key: "fedora-43-arm64",
            archive: "tinyconnectors-0.19.3-fedora-43-arm64.tar.gz",
            sha256: "386552936cc9964ec4a006664656fa1ac58aaccd104dbf8fd71626824805dac4",
        },
        PlatformAsset {
            host_key: "fedora-43-x86_64",
            archive: "tinyconnectors-0.19.3-fedora-43-x86_64.tar.gz",
            sha256: "7d810750a28d64ff332448095c91e57064da1dfdd70028ac378c86c88df8ca2c",
        },
        PlatformAsset {
            host_key: "fedora-44-arm64",
            archive: "tinyconnectors-0.19.3-fedora-44-arm64.tar.gz",
            sha256: "a7094a70e28f99dbe4312be4c0d5b49337b5b35e605bd07c7dde323e247fcdf6",
        },
        PlatformAsset {
            host_key: "fedora-44-x86_64",
            archive: "tinyconnectors-0.19.3-fedora-44-x86_64.tar.gz",
            sha256: "39c4938e9daba8643910e2b4f8573c0559d1cf8a15b44bd99e615a9dab926081",
        },
        PlatformAsset {
            host_key: "macos-15-arm64",
            archive: "tinyconnectors-0.19.3-macos-15-arm64.tar.gz",
            sha256: "644a1c58c2ab7911ea2808c23b75b96e3ae312af1d12f97cca55f71971f551b9",
        },
        PlatformAsset {
            host_key: "macos-15-x86_64",
            archive: "tinyconnectors-0.19.3-macos-15-x86_64.tar.gz",
            sha256: "53297c0fffad47ac19dba05a9e360af1601cf4de779e7db0d9e06d2059e91099",
        },
        PlatformAsset {
            host_key: "macos-26-arm64",
            archive: "tinyconnectors-0.19.3-macos-26-arm64.tar.gz",
            sha256: "96290a9f9b9b07fffbe3f333478002d7f46e5c2a563542d73ddac47420e597a4",
        },
        PlatformAsset {
            host_key: "macos-26-x86_64",
            archive: "tinyconnectors-0.19.3-macos-26-x86_64.tar.gz",
            sha256: "990a0414fb4a2fc834efeedcd5e4e51e7057aed928f9c51aa4fae2c4a43a7f0d",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-arm64",
            archive: "tinyconnectors-0.19.3-ubuntu-22.04-arm64.tar.gz",
            sha256: "f5ab8ced9095150df3ab0f3b84b4fe505a61d1fec64de535355d59a7ee13954f",
        },
        PlatformAsset {
            host_key: "ubuntu-22.04-x86_64",
            archive: "tinyconnectors-0.19.3-ubuntu-22.04-x86_64.tar.gz",
            sha256: "26daaa208a624c1afc5520fdd3d715eca032b956324ad5918431171097381620",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-arm64",
            archive: "tinyconnectors-0.19.3-ubuntu-24.04-arm64.tar.gz",
            sha256: "ded63f30313e3196c0f75e6c6d211920d799df5e39b4d0657c4901de419db96e",
        },
        PlatformAsset {
            host_key: "ubuntu-24.04-x86_64",
            archive: "tinyconnectors-0.19.3-ubuntu-24.04-x86_64.tar.gz",
            sha256: "15cfa5718aeabe2882503c618e22c5067f050fbc3aaa4be31e541071700900f5",
        },
        PlatformAsset {
            host_key: "windows-11-arm64",
            archive: "tinyconnectors-0.19.3-windows-11-arm64.zip",
            sha256: "02a7e82e452c9190db68b5f2eef49b6fad73bf067dbb8f13ab3688c003dedf61",
        },
        PlatformAsset {
            host_key: "windows-2022-x86_64",
            archive: "tinyconnectors-0.19.3-windows-2022-x86_64.zip",
            sha256: "cf0d99f72732d934b24428cfa2ef414a649d41c612de330ea76af4e037641822",
        },
        PlatformAsset {
            host_key: "windows-2025-x86_64",
            archive: "tinyconnectors-0.19.3-windows-2025-x86_64.zip",
            sha256: "271bd3855a1565fedd041a02bbca2555d418bcd16d764b55385f6896d8e9682b",
        },
    ],
    // Lazy: a user with no connected accounts should not pay to load it, and
    // most sessions never touch a connector. Safe even signed out — the module
    // loads without configuration and still answers the capability members.
    load: LoadPolicy::Lazy,
    source: SourceSpec {
        submodule: "tinyconnectors",
        manifest: "crates/tinyconnectors/Cargo.toml",
        package: "tinyconnectors",
        lib_name: "tinyconnectors",
        features: &[],
    },
};
