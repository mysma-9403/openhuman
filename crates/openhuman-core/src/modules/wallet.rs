//! Calling the `tinywallet` module.
//!
//! The host half of `ai.tinyhumans.tinywallet.Wallet`. Two ways to get a signed
//! transaction, and which one a chain uses is a property of that chain.
//!
//! # The confidential flow — preferred, and what BTC, EVM and Tron use
//!
//! [`sign_transaction_in_module`] sends the recovery phrase itself, once, and
//! the module derives, encodes, signs and assembles. No private key is ever
//! reassembled in this process.
//!
//! ```text
//!   host  ==SignTransaction{phrase, path, fields}==>  module   (confidential)
//!   host  <=={raw transaction, txid}================  module
//! ```
//!
//! The phrase only goes to a recipient tinybus has attested, and
//! [`attested_proxy`] additionally insists the attested digest is one
//! `registry.rs` pinned — see there for why both checks are worth having.
//!
//! # The split flow — still here, still correct for some hosts
//!
//! [`sign_transaction`] keeps the key in this process: the module returns
//! digests, this process signs them, the module reassembles.
//!
//! ```text
//!   host  --BuildUnsigned{fields, public key}-->  module
//!   host  <--[digests to sign]-------------------  module
//!   host    (signs locally, with a key the module never sees)
//!   host  --AttachSignature{fields, signatures}->  module
//!   host  <--{raw transaction, txid}-------------  module
//! ```
//!
//! It is not deprecated. It is the only option for a backend reached across a
//! transport, where the bus cannot say what is on the other end, and it is what
//! Solana still uses here — Solana hand-builds SPL messages that
//! `TransactionSpec::Solana` does not model, so there is nothing to send.
//!
//! # What the confidential flow does and does not buy
//!
//! A loaded module shares this address space and could read the phrase out of
//! process memory whichever flow is used, so neither is a hard isolation
//! boundary and neither is claimed as one.
//!
//! What changes is that this binary no longer performs derivation or signing at
//! all — the key exists only inside the module, for the duration of one call,
//! rather than being assembled here and held across two round trips. The bus
//! also refuses to carry the phrase to anything that is not an allowlisted,
//! hash-verified module, which the split flow could not express.
//!
//! # The fields are sent twice in the split flow, deliberately
//!
//! `AttachSignature` re-sends everything `BuildUnsigned` was given rather than a
//! handle to something the module remembered. That is what lets the module hold
//! no state between the calls — no store, no bound on it, no expiry for a host
//! that never comes back. Building is deterministic, so the module rebuilds the
//! transaction the digests were computed over. The confidential flow needs none
//! of this: it is one call.
//!
//! # Two signing schemes, and the difference matters
//!
//! A `Secp256k1Prehash` payload is **already hashed** and must be signed with a
//! prehash entry point; hashing it again produces a valid signature over the
//! wrong thing. An `Ed25519` payload is the whole message and must **not** be
//! pre-hashed, because ed25519 hashes internally. The module tags every payload
//! with which it is, and [`sign_payload`] dispatches on the tag rather than on
//! the chain — so a chain that changes scheme cannot silently sign wrongly.

use serde::de::DeserializeOwned;
use serde::Serialize;
use tinywallet_bus::names::methods;
use tinywallet_bus::wire::{
    DeriveMnemonicSeedRequest, DeriveMnemonicSeedResponse, DerivedAccount,
    EncodeErc20TransferRequest, EncodeErc20TransferResponse, ExportRequest, ExportedKey,
    GenerateMnemonicRequest, GenerateMnemonicResponse, Scheme, SecretMaterial, SignMessageRequest,
    SignRequest, Signature, SignedTransaction, TransactionSpec,
};
use zeroize::Zeroize;

use super::{ops, registry};
use crate::config::Config;

/// Registry id of the module these calls go to.
const MODULE_ID: &str = "tinywallet";

/// Why a wallet call did not produce a signed transaction.
///
/// Three variants because the wallet tools map them onto three different
/// user-facing outcomes: something the caller can correct, something it cannot,
/// and a capability that is not present on this host at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletCallError {
    /// The module is not loaded and cannot be: unsupported host, downloads off,
    /// disabled in config, or a load that already failed in this process.
    Unavailable(String),
    /// The request was rejected. The caller can act on this.
    InvalidInput(String),
    /// Building, signing, or assembly failed.
    Failed(String),
}

impl std::fmt::Display for WalletCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::InvalidInput(message) | Self::Failed(message) => {
                f.write_str(message)
            }
        }
    }
}

/// Derive, build, sign and assemble entirely inside the module.
///
/// The counterpart to [`sign_transaction`], and the one to prefer: the phrase
/// crosses the bus once and no key material is ever reassembled here.
///
/// # Errors
///
/// [`WalletCallError`]. `Unavailable` if the module is not an attested
/// recipient — see [`attested_proxy`], which refuses to send the phrase at all
/// in that case.
pub async fn sign_transaction_in_module(
    config: &Config,
    transaction: &TransactionSpec,
    secret: &SecretMaterial,
) -> Result<SignedTransaction, WalletCallError> {
    let proxy = attested_proxy(config).await?;
    log::debug!(
        "[modules:wallet] sign_transaction chain={:?} module={MODULE_ID} (confidential)",
        transaction.chain()
    );
    proxy
        .call_confidential(
            methods::SIGN_TRANSACTION,
            (SignRequest {
                secret: secret.clone(),
                transaction: transaction.clone(),
            },),
        )
        .await
        .map_err(|error| classify(&error))
}

/// Ask the module for an address and public key.
///
/// # Errors
///
/// [`WalletCallError`].
pub async fn derive_account(
    config: &Config,
    secret: &SecretMaterial,
) -> Result<DerivedAccount, WalletCallError> {
    log::debug!(
        "[modules:wallet] derive_account chain={:?} module={MODULE_ID} (confidential)",
        secret.chain
    );
    let mut request = secret.clone();
    let result = call_confidential(config, methods::DERIVE_ACCOUNT, &request).await;
    request.mnemonic.zeroize();
    result
}

/// Generate a recovery phrase inside the attested TinyWallet module.
///
/// # Errors
///
/// [`WalletCallError`]. The phrase is returned to the host for encryption and
/// custody, but generation and BIP-39 vocabulary remain module-owned.
pub async fn generate_mnemonic(
    config: &Config,
    word_count: u8,
) -> Result<GenerateMnemonicResponse, WalletCallError> {
    call_confidential(
        config,
        methods::GENERATE_MNEMONIC,
        &GenerateMnemonicRequest { word_count },
    )
    .await
}

/// Derive a standard BIP-39 seed inside the attested module.
///
/// The caller uses the seed only for OpenHuman's generic storage-key KDF.
///
/// # Errors
///
/// [`WalletCallError`]. Invalid or unsupported phrases are classified as
/// [`WalletCallError::InvalidInput`].
pub async fn derive_mnemonic_seed(
    config: &Config,
    mnemonic: String,
) -> Result<DeriveMnemonicSeedResponse, WalletCallError> {
    let mut request = DeriveMnemonicSeedRequest { mnemonic };
    let result = call_confidential(config, methods::DERIVE_MNEMONIC_SEED, &request).await;
    request.mnemonic.zeroize();
    result
}

/// Encode ERC-20 transfer calldata with the module-owned ABI implementation.
///
/// # Errors
///
/// Returns [`WalletCallError`] if the module is unavailable or rejects the
/// recipient or base-unit amount.
pub async fn encode_erc20_transfer(
    config: &Config,
    to_address: &str,
    amount_raw: &str,
) -> Result<String, WalletCallError> {
    let response: EncodeErc20TransferResponse = call(
        config,
        methods::ENCODE_ERC20_TRANSFER,
        &EncodeErc20TransferRequest {
            to_address: to_address.to_owned(),
            amount_raw: amount_raw.to_owned(),
        },
    )
    .await?;
    Ok(response.calldata)
}

/// Sign opaque bytes with the key derived from `secret`.
///
/// Blind: the module cannot check what the bytes mean, so
/// [`sign_transaction_in_module`] is the right call wherever the request can be
/// expressed as a `TransactionSpec`. This exists for the two encodings the wire
/// contract does not model — Solana SPL transfers and x402 payments — where the
/// alternative is not a verified signature but deriving the key in this process
/// and signing here, which is what these callers used to do.
///
/// # Errors
///
/// [`WalletCallError`].
pub async fn sign_message(
    config: &Config,
    secret: &SecretMaterial,
    message: &[u8],
    scheme: Scheme,
) -> Result<Signature, WalletCallError> {
    let proxy = attested_proxy(config).await?;
    log::debug!(
        "[modules:wallet] sign_message chain={:?} bytes={} module={MODULE_ID} (confidential)",
        secret.chain,
        message.len()
    );
    proxy
        .call_confidential(
            methods::SIGN_MESSAGE,
            (SignMessageRequest {
                secret: secret.clone(),
                message_hex: hex(message),
                scheme,
            },),
        )
        .await
        .map_err(|error| classify(&error))
}

/// Ask the module for the raw derived key.
///
/// A compatibility call for downstream hosts that must drive a signer locally.
/// OpenHuman's own wallet paths sign inside the module.
///
/// # Errors
///
/// [`WalletCallError`].
pub async fn export_key(
    config: &Config,
    secret: &SecretMaterial,
) -> Result<ExportedKey, WalletCallError> {
    let proxy = attested_proxy(config).await?;
    log::debug!(
        "[modules:wallet] export_key chain={:?} module={MODULE_ID} (confidential)",
        secret.chain
    );
    proxy
        .call_confidential(
            methods::EXPORT_KEY,
            (ExportRequest {
                secret: secret.clone(),
            },),
        )
        .await
        .map_err(|error| classify(&error))
}

/// Call a typed Wallet, Web3, or x402 member with one request argument.
/// Business rules stay in the module; the host only transports the request.
pub async fn call<Request, Reply>(
    config: &Config,
    member: &str,
    request: Request,
) -> Result<Reply, WalletCallError>
where
    Request: Serialize + Send,
    Reply: DeserializeOwned,
{
    let (runtime, record) = ready(config).await?;
    proxy(runtime, record)?
        .call::<Reply>(member, (request,))
        .await
        .map_err(|error| classify(&error))
}

/// Call a Wallet member carrying secret material over TinyBus's confidential
/// transport. The loaded module must be attested and match this build's pin.
pub async fn call_confidential<Request, Reply>(
    config: &Config,
    member: &str,
    request: &Request,
) -> Result<Reply, WalletCallError>
where
    Request: Serialize + Send,
    Reply: DeserializeOwned,
{
    attested_proxy(config)
        .await?
        .call_confidential::<Reply>(member, (request,))
        .await
        .map_err(|error| classify(&error))
}

/// A proxy that has proved it is the artifact this build pinned.
///
/// # Why check here when the broker already refuses
///
/// The broker will not route a confidential call to an unattested recipient, so
/// omitting this would still be safe against the case it covers. Two reasons to
/// check anyway.
///
/// The first is the error. A broker refusal arrives after the request has been
/// serialized, which means a frame containing the recovery phrase was built and
/// handed to the bus before anything said no. Checking first means the phrase is
/// never put into a buffer on a call that was always going to fail.
///
/// The second is that this compares the digest against **this build's own
/// table**, which the broker cannot do — it only knows the host vouched for
/// something. Here we can insist it vouched for one of the artifacts
/// `registry.rs` names. That closes the gap where a host is somehow induced to
/// attest a different artifact for this bus name.
///
/// Both are belt-and-braces over a check that already exists. That is the right
/// posture for the one code path that hands over a recovery phrase.
async fn attested_proxy(config: &Config) -> Result<tinybus::Proxy, WalletCallError> {
    let (runtime, record) = ready(config).await?;
    let proxy = proxy(runtime, record)?;

    let attestation = proxy
        .attestation()
        .await
        .map_err(|error| WalletCallError::Failed(error.to_string()))?
        .ok_or_else(|| {
            WalletCallError::Unavailable(
                "the wallet module is loaded but not an attested recipient, so it cannot be sent \
                 key material. This host is running a build whose module loader does not record \
                 an attestation for pinned releases; upgrade it rather than working around this."
                    .to_string(),
            )
        })?;

    if attestation.name.as_str() != record.bus_name {
        return Err(WalletCallError::Failed(format!(
            "the wallet module's attestation names '{}' rather than '{}'",
            attestation.name.as_str(),
            record.bus_name
        )));
    }

    if !digest_is_pinned(record, &attestation.sha256) {
        return Err(WalletCallError::Failed(
            "the loaded wallet module is not one of the artifacts this build pinned, so it will \
             not be sent key material"
                .to_string(),
        ));
    }

    Ok(proxy)
}

// Embedded library checksums verified from all 17 archives in the
// registry-pinned TinyWallet v0.24.1 release. Each library is trusted only
// while its archive checksum remains pinned in the supplied ModuleRecord.
const PINNED_WALLET_LIBRARIES: &[(&str, &str)] = &[
    // archlinux-arm64
    (
        "f0e013d305d35744911e4e88ae391bd5b698ca9b5d4b5965ff2cf7e038022a55",
        "00b673be17b9e71c1ca578368ee65d6fa3bd09330a788aa97240e27d3a46781c",
    ),
    // archlinux-x86_64
    (
        "e8feeacc13150230db39885471868f3cb983d32cdca3b54076cfa3b7b99ca291",
        "bdab255f2bbe4a0b5efaf40b85f58526392195d2f1e240ceffa3370f51f1e5e7",
    ),
    // fedora-43-arm64
    (
        "a5eda11c12b04838101113bb691809cccaab4f02b63927ed87b9ae23354e8024",
        "5fa2aea35c297bd2f77f20dec6ca1f5e989252328332023384ace47aeb17874b",
    ),
    // fedora-43-x86_64
    (
        "81806447a07fb1e5f2e2e11c44cc3bb673b9583836b468086abda33cac7d74c6",
        "11a319662c4dae5d6e2b071c1d3ea4f8d452fa75d1f2e79d4064342c8f9faac4",
    ),
    // fedora-44-arm64
    (
        "1b46b6df147080672ed2f87c9b560f3c4ab1d5edabe16f98e66e3cc1df0556fc",
        "0c98f102ea72993a8709a74723cf456e37c122186c7b1e88104fad25ec687fed",
    ),
    // fedora-44-x86_64
    (
        "021973415b5e6ebde7f903e9ecd7727a3511fe22bf5f4452720eb6d6b8898a7f",
        "0a5c3626bbde1df3111c5484c9ff055af7c642417404ff7a47e7b477096b332c",
    ),
    // macos-15-arm64
    (
        "018451ecd682ca8cdd45667d9d7a10e8200a666f1ce5e76c7784f1ed1fe2a59b",
        "99e583e7d58f94cdd4db41d9b54c06d0ccf9365c0d90f5d0639a1cfcbc3fad46",
    ),
    // macos-15-x86_64
    (
        "8c9887fe1a321e0f13b1af3f9129e6db02186fd791d6769e3a80bae964ddb240",
        "dc9b20beb653583f482a3dda707831240bd383328fe7b17e9ffe387e148fb4af",
    ),
    // macos-26-arm64
    (
        "d0896d7fbb8936542cdfc4c0d138a262982d599ec015401a6dbd1d4410a39d3a",
        "40b997846be24357da6bee06fdb683b05f0369bc3ba6fefdb44e9b16f75b1b5f",
    ),
    // macos-26-x86_64
    (
        "19409a538aa0fd04f246ebcbaacd4aefc7c55279d49f3d066a44257432617314",
        "7112baecdcb3b1caf213d29f699bf73d1d313a9ce3a00bf4770faf480f229110",
    ),
    // ubuntu-22.04-arm64
    (
        "0b7da4811e31e2d7239c62fdb30c8ac4c45667b596b27033f4f3915655fccbe7",
        "5717f0a59354485140ad0b2968fd806b358efca3146d0d61324a5ed55043457b",
    ),
    // ubuntu-22.04-x86_64
    (
        "5b1e62a9fa88939cf1711dc0480a6298d240ce2e49cbd4a448a2f967a2f30255",
        "28f7e2f3a1b7e9b792374fb8f5cbaad4523311ac395ed834a9547a281b0d009c",
    ),
    // ubuntu-24.04-arm64
    (
        "e50ffef6a61dfbfffc0be0ad32e8388ee72c8803f63b71f435ddc6c47c337650",
        "bd8ffbd8382a29fc1b9da8037e3e50fe0a4b305aa73511c8b94925c994e23bc3",
    ),
    // ubuntu-24.04-x86_64
    (
        "11f66fa19923f1fd151252294b947becf6eb5edafe6a7750681daf74c028d28c",
        "1e38209820ffa41857adfc32c322871bee30eec6bfc02dfae46eb7d9f6013def",
    ),
    // windows-11-arm64
    (
        "ad8f432919fa78ca7a0828b8c3ad02fcd1b65855cbaf1f76355dcd0423ff7415",
        "af7d3d3fb2116a7a7488ab8fcfcc40f8e27352b5d22258fa88dd7d2e40cbcb83",
    ),
    // windows-2022-x86_64
    (
        "03badd28042ff3a211a489bafb42ac1fa90bb320de66bb73ef5317c7b4c560d3",
        "cf5797c8a34ddbcb29677cf97d5f8de4b59f3a44c2e82e84e0aef34ddbcd7192",
    ),
    // windows-2025-x86_64
    (
        "53fc1c72e207769fa471373320b0ca73c067c2f6b32a65e73f8fc7ef18a0fa80",
        "d246fa08de8306ca1105d2e61999bba3689eb7951c8fb0d650a9745eb5417389",
    ),
];

fn digest_is_pinned(record: &super::ModuleRecord, sha256: &str) -> bool {
    record.assets.iter().any(|asset| {
        asset.sha256.eq_ignore_ascii_case(sha256)
            || PINNED_WALLET_LIBRARIES.iter().any(|(archive, library)| {
                asset.sha256.eq_ignore_ascii_case(archive) && library.eq_ignore_ascii_case(sha256)
            })
    })
}

/// Load the wallet module if it is not already serving.
///
/// Callers do not have to invoke this — the signing calls do — but a caller
/// that wraps its work in a deadline should, *outside* that deadline. A first
/// use may download and verify an artifact, and charging that against a
/// transaction timeout means the first transfer a user ever makes is the one
/// that fails.
///
/// # Errors
///
/// [`WalletCallError::Unavailable`].
pub async fn ensure_ready(config: &Config) -> Result<(), WalletCallError> {
    ops::ensure_loaded(config, MODULE_ID)
        .await
        .map_err(WalletCallError::Unavailable)
}

/// Ensure the module is serving and hand back what a call needs.
async fn ready(
    config: &Config,
) -> Result<
    (
        &'static tinybus::module::ModuleRuntime,
        &'static super::ModuleRecord,
    ),
    WalletCallError,
> {
    ops::ensure_loaded(config, MODULE_ID)
        .await
        .map_err(WalletCallError::Unavailable)?;
    let record = registry::find(MODULE_ID)
        .ok_or_else(|| WalletCallError::Unavailable(format!("unknown module '{MODULE_ID}'")))?;
    let runtime = super::runtime::runtime()
        .await
        .map_err(|_| WalletCallError::Unavailable("the module bus is not running".to_string()))?;
    Ok((runtime, record))
}

/// A proxy for the module's object.
fn proxy(
    runtime: &'static tinybus::module::ModuleRuntime,
    record: &super::ModuleRecord,
) -> Result<tinybus::Proxy, WalletCallError> {
    runtime
        .proxy(record.bus_name, record.object_path)
        .map_err(|error| WalletCallError::Failed(error.to_string()))
}

/// Map a bus failure onto the shape a caller can act on.
///
/// The wire name is the contract; the message is for a human. An unrecognised
/// name is `Failed` rather than `InvalidInput`, because telling a caller its
/// input was wrong when it was not points at the wrong fix.
fn classify(error: &tinybus::Error) -> WalletCallError {
    let message = error.to_string();
    match error.wire_name() {
        "ai.tinyhumans.tinywallet.Error.InvalidInput" => WalletCallError::InvalidInput(message),
        "ai.tinyhumans.tinywallet.Error.UnsupportedChain" => WalletCallError::Unavailable(message),
        // The module is loaded but not answering: refused, faulted, or gone.
        name if name.contains("ModuleUnavailable") => WalletCallError::Unavailable(message),
        _ => WalletCallError::Failed(message),
    }
}

/// Lowercase hex, unprefixed — the form every field in the wire contract uses.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
#[path = "wallet_tests.rs"]
mod tests;
