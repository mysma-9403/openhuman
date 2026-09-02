use super::*;

#[test]
fn is_newer_detects_update() {
    assert!(is_newer("0.50.0", "0.49.17"));
    assert!(is_newer("1.0.0", "0.99.99"));
    assert!(is_newer("v0.50.0", "0.49.17"));
    assert!(!is_newer("0.49.17", "0.49.17"));
    assert!(!is_newer("0.49.16", "0.49.17"));
    assert!(!is_newer("0.49.17", "0.50.0"));
}

#[test]
fn current_version_is_not_empty() {
    assert!(!current_version().is_empty());
}

/// OPENHUMAN-TAURI-2F regression guard. A reqwest call to an unroutable
/// host (port 1 on TEST-NET-1, RFC 5737 documentation range — guaranteed
/// never to answer) must classify as a transport failure so the
/// `check_releases` / `download` call sites skip the Sentry report. If
/// reqwest ever changes its error taxonomy and connection failures stop
/// setting `is_connect` / `is_request` / `is_timeout`, this test breaks
/// and the call sites would silently start paging again — that's the
/// signal we want.
#[tokio::test]
async fn transport_failure_classifier_catches_unreachable_host() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(250))
        .no_proxy()
        .build()
        .expect("build reqwest client");
    let result = client.get("http://192.0.2.1:1/").send().await;
    let err = result.expect_err("connect to TEST-NET-1:1 must fail");
    assert!(
        is_transport_network_failure(&err),
        "unreachable-host reqwest error must classify as transport: {err}"
    );
}
