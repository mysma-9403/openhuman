//! Stable English fallback copy for terminal turn-stop reasons.

/// The user-facing text for one `turn_stop.*` localization key.
pub(crate) fn stop_summary_copy(key: &str) -> &'static str {
    match key {
        "turn_stop.transient" => {
            "a service I needed kept timing out or was temporarily unavailable"
        }
        "turn_stop.uncertain_side_effect" => {
            "an action timed out and I could not confirm whether it went through, so I did not repeat it"
        }
        "turn_stop.authentication" => "a service rejected my credentials",
        "turn_stop.permission" => "I don't have permission for a step this needs",
        "turn_stop.policy" => "a step this needs is blocked by your security settings",
        "turn_stop.missing_program" => {
            "a program or feature this needs is not available on this device"
        }
        "turn_stop.not_found" => "something this needs could not be found",
        "turn_stop.invalid_calls" => "a tool kept rejecting my requests as invalid",
        "turn_stop.unavailable" => "a tool this needs is not available right now",
        "turn_stop.site_refused" => "a website kept refusing my requests",
        "turn_stop.repeated_failure" => "the same step kept failing",
        "turn_stop.varied_failures" => "several different tool calls in a row failed",
        "turn_stop.repeating_step" => "I kept repeating the same step without getting anything new",
        "turn_stop.out_of_credits" => {
            "the account is out of inference credits; add credits, then ask me to try again"
        }
        "turn_stop.provider_config" => {
            "the configured model or provider rejected the request; check the model and API key in Connections"
        }
        "turn_stop.missing_connection" => {
            "a service this needs isn't connected; connect it in Connections, then ask me to try again"
        }
        "turn_stop.generic" => "my tool calls were not getting anywhere",
        _ => "my tool calls were not getting anywhere",
    }
}
