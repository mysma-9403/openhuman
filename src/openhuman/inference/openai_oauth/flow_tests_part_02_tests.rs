use super::*;

#[test]
fn lookup_key_for_slug_uses_legacy_openai_api_key_when_new_style_is_empty() {
    let tmp = tempdir().unwrap();
    let config = test_config(&tmp);
    let store = AuthProfilesStore::new(tmp.path(), false);
    let oauth_profile = AuthProfile::new_oauth(
        OPENAI_PROVIDER_KEY,
        OPENAI_OAUTH_PROFILE_NAME,
        TokenSet {
            access_token: "   ".into(),
            refresh_token: None,
            id_token: None,
            expires_at: Some(Utc::now() + Duration::hours(1)),
            token_type: Some("Bearer".into()),
            scope: None,
        },
    );
    store.upsert_profile(oauth_profile, true).unwrap();
    store
        .upsert_profile(
            AuthProfile::new_token("openai", "default", "sk-legacy-key".to_string()),
            true,
        )
        .unwrap();

    // Legacy bare-slug key resolves through the standard path's legacy
    // fallback, ahead of the OAuth fallback.
    let token = lookup_key_for_slug("openai", &config).unwrap();
    assert_eq!(token, "sk-legacy-key");
}

#[test]
fn lookup_openai_bearer_token_keeps_expired_token_when_refresh_fails_without_runtime() {
    let tmp = tempdir().unwrap();
    let config = test_config(&tmp);
    let store = AuthProfilesStore::new(tmp.path(), false);
    let oauth_profile = AuthProfile::new_oauth(
        OPENAI_PROVIDER_KEY,
        OPENAI_OAUTH_PROFILE_NAME,
        TokenSet {
            access_token: "expired-access".into(),
            refresh_token: Some("refresh".into()),
            id_token: None,
            expires_at: Some(Utc::now() - Duration::minutes(5)),
            token_type: Some("Bearer".into()),
            scope: None,
        },
    );
    store.upsert_profile(oauth_profile, true).unwrap();

    let token = lookup_openai_bearer_token(&config).unwrap();
    assert_eq!(token.as_deref(), Some("expired-access"));
}

#[tokio::test(flavor = "multi_thread")]
async fn lookup_openai_bearer_token_does_not_persist_blank_refreshed_access_token() {
    let _env_lock = crate::openhuman::config::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let tmp = tempdir().unwrap();
    let config = test_config(&tmp);
    let store = AuthProfilesStore::new(tmp.path(), false);
    let original_profile = AuthProfile::new_oauth(
        OPENAI_PROVIDER_KEY,
        OPENAI_OAUTH_PROFILE_NAME,
        TokenSet {
            access_token: "oauth-access".into(),
            refresh_token: Some("refresh-token".into()),
            id_token: None,
            expires_at: Some(Utc::now() - Duration::minutes(5)),
            token_type: Some("Bearer".into()),
            scope: None,
        },
    );
    store.upsert_profile(original_profile, true).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "   ",
            "refresh_token": "refresh-updated",
            "id_token": "id-updated",
            "expires_in": 3600,
        })))
        .mount(&server)
        .await;

    let _env_guard = EnvVarGuard::set(
        "OPENAI_CODEX_OAUTH_TOKEN_URL",
        format!("{}/token", server.uri()),
    );

    let token = lookup_openai_bearer_token(&config).unwrap();
    assert_eq!(
        token.as_deref(),
        Some("oauth-access"),
        "invalid refresh payload should not replace the last known good access token"
    );

    let reloaded = AuthProfilesStore::new(tmp.path(), false).load().unwrap();
    let stored = reloaded
        .profiles
        .get(&format!(
            "{OPENAI_PROVIDER_KEY}:{OPENAI_OAUTH_PROFILE_NAME}"
        ))
        .expect("oauth profile should still exist after invalid refresh response");
    let token_set = stored.token_set.as_ref().expect("oauth token_set");
    assert_eq!(
        token_set.access_token, "oauth-access",
        "invalid refresh payload should not be persisted"
    );
}

#[test]
fn lookup_openai_bearer_token_returns_none_without_profiles_or_access_token() {
    let tmp = tempdir().unwrap();
    let config = test_config(&tmp);
    assert_eq!(lookup_openai_bearer_token(&config).unwrap(), None);

    let store = AuthProfilesStore::new(tmp.path(), false);
    let empty_oauth_profile = AuthProfile::new_oauth(
        OPENAI_PROVIDER_KEY,
        OPENAI_OAUTH_PROFILE_NAME,
        TokenSet {
            access_token: "   ".into(),
            refresh_token: None,
            id_token: None,
            expires_at: Some(Utc::now() - Duration::hours(1)),
            token_type: Some("Bearer".into()),
            scope: None,
        },
    );
    store.upsert_profile(empty_oauth_profile, true).unwrap();

    assert_eq!(lookup_openai_bearer_token(&config).unwrap(), None);
}

#[test]
fn disconnect_openai_oauth_clears_profile() {
    let tmp = tempdir().unwrap();
    let config = test_config(&tmp);
    let store = AuthProfilesStore::new(tmp.path(), false);
    let profile = AuthProfile::new_oauth(
        OPENAI_PROVIDER_KEY,
        OPENAI_OAUTH_PROFILE_NAME,
        TokenSet {
            access_token: "oauth-access".into(),
            refresh_token: None,
            id_token: None,
            expires_at: None,
            token_type: Some("Bearer".into()),
            scope: None,
        },
    );
    store.upsert_profile(profile, true).unwrap();
    assert!(openai_oauth_status(&config).unwrap().connected);

    disconnect_openai_oauth(&config).unwrap();
    assert!(!openai_oauth_status(&config).unwrap().connected);
}
