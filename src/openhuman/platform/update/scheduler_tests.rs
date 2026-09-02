use super::*;

#[test]
fn min_interval_is_at_least_ten_minutes() {
    // GitHub's API rate-limits unauthenticated callers — anything
    // shorter than ~10 minutes will trip the rate limit on a busy
    // machine. Lock in the floor so a future "let users tick every
    // minute" change doesn't silently break update visibility.
    assert!(MIN_INTERVAL_MINUTES >= 10);
}

#[tokio::test]
async fn run_returns_immediately_when_disabled() {
    // Even with `interval_minutes = 0` the disabled config must
    // short-circuit before the loop. Using tokio's pause/advance
    // would also work, but a direct .await is enough — if the
    // function doesn't return promptly the test will hang and
    // surface the regression.
    let cfg = UpdateConfig {
        enabled: false,
        interval_minutes: 0,
        ..UpdateConfig::default()
    };
    run(cfg).await;
}

// NOTE: We deliberately do NOT unit-test `tick()` directly. It calls
// `update_core::check_available()` which performs a real HTTPS request
// to api.github.com — running that from the unit suite makes the test
// flaky (offline CI runners, rate limits, DNS hiccups). Coverage of
// the HTTP + JSON-parse path is better handled via an integration test
// that uses an HTTP mock (e.g. `httpmock`) around a refactored
// `check_available_with_url(base_url)`. For now the surrounding
// properties are locked down by:
//   - `min_interval_is_at_least_ten_minutes` (rate-limit floor)
//   - `run_returns_immediately_when_disabled` (disabled short-circuit)
