use super::resolve_local_runtime_key;
use crate::openhuman::config::Config;

#[test]
fn omlx_key_falls_back_to_local_ai_api_key() {
    let mut config = Config::default();
    config.local_ai.api_key = Some("  sk-omlx-list  ".into());
    assert_eq!(
        resolve_local_runtime_key("omlx", String::new(), &config),
        "sk-omlx-list"
    );
}

#[test]
fn looked_up_key_wins_over_local_ai() {
    let mut config = Config::default();
    config.local_ai.api_key = Some("sk-local".into());
    assert_eq!(
        resolve_local_runtime_key("omlx", "from-profiles".into(), &config),
        "from-profiles"
    );
}

#[test]
fn non_omlx_slug_does_not_fall_back() {
    let mut config = Config::default();
    config.local_ai.api_key = Some("sk-local".into());
    assert_eq!(
        resolve_local_runtime_key("ollama", String::new(), &config),
        ""
    );
}
