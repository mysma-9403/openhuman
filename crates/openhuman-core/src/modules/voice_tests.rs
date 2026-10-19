use super::{is_hallucinated, HallucinationMode, VoiceCallError};

#[tokio::test]
async fn transcript_screening_reports_unavailable_when_modules_are_disabled() {
    let mut config = crate::config::Config::default();
    config.modules.enabled = false;

    assert!(matches!(
        is_hallucinated(&config, "recognized words", HallucinationMode::Conversation).await,
        Err(VoiceCallError::Unavailable(_))
    ));
}

#[test]
fn tinyvoice_module_remains_registered_for_transcript_screening() {
    let record = crate::modules::registry::find("tinyvoice").expect("tinyvoice is registered");
    assert_eq!(record.bus_name, tinyvoice_bus::names::BUS_NAME);
    assert_eq!(record.object_path, tinyvoice_bus::names::OBJECT_PATH);
}
