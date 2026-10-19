//! Controller schemas and RPC handler dispatch for the voice domain.
//!
//! Split by concern:
//! - `registry.rs` — the controller table (`all_voice_controller_schemas`,
//!   `all_voice_registered_controllers`, `voice_schemas`).
//! - `params.rs` — `Deserialize` param structs for each RPC method.
//! - `helpers.rs` — shared plumbing (`to_json`, `deserialize_params`,
//!   provider validation, silent-WAV generation).
//! - `handlers.rs` — re-exports the actual `handle_voice_*` /
//!   handlers for transcription, synthesis, factory dispatch, and provider
//!   settings.

mod handlers;
mod helpers;
mod params;
mod registry;

// Re-export the public API that callers outside this module use.
pub use registry::{all_voice_controller_schemas, all_voice_registered_controllers, voice_schemas};

// ---------------------------------------------------------------------------
// Internal re-exports used by the test companion file.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(test)]
use helpers::{
    deserialize_params, generate_silent_wav, to_json, validate_stt_provider, validate_tts_provider,
};
#[cfg(test)]
use params::{
    SetProvidersParams, SttDispatchParams, TranscribeBytesParams, TranscribeParams,
    TtsDispatchParams, TtsParams, VoiceListModelsParams, VoiceTestProviderParams,
    VoiceUpdateProviderSettingsParams,
};

#[cfg(test)]
use crate::core::Outcome;
#[cfg(test)]
use serde_json::Map;
#[cfg(test)]
use serde_json::Value;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "../schemas_tests.rs"]
mod tests;
