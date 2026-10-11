//! Policy probes (leak a): a user's model asks for host file and shell tools
//! aimed outside the user's sandbox. The deployment opts users into
//! `host_files` only, so `shell` must not even be offered, and every file
//! call that leaves `users/<id>/sandbox/` must be refused under the
//! *profile's* policy. The operator's policy is off (the default), so it
//! would have read the host file below.

use super::mock_llm::{mock_llm, PROBE};
use super::world::{canary, credential, USERS};
use super::*;

/// What became of a probe's tool call, read from the turn's tool timeline.
#[derive(Debug)]
struct Outcome {
    status: String,
    class: String,
    output: String,
}

/// Run one probe as alice on its own thread and wait for its tool call to
/// finish.
fn probe(node: &Node, tag: &str, tool: &str, args: Value) -> Outcome {
    let thread = format!("probe-{tag}");
    let message = format!("{thread} {PROBE} {tool} {args}");
    let (status, _, body) = call(
        node,
        USERS[0],
        "openhuman.channel_web_chat",
        json!({ "client_id": "c1", "thread_id": thread, "message": message }),
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.get("result").is_some(), "probe {tag}: {body}");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (_, _, body) = call(
            node,
            USERS[0],
            "openhuman.threads_turn_state_get",
            json!({ "thread_id": thread }),
        );
        let state = find_key(&body, "turnState").cloned().unwrap_or(Value::Null);
        let done = !matches!(
            state["lifecycle"].as_str(),
            None | Some("started" | "streaming")
        );
        let call = state["toolTimeline"]
            .as_array()
            .and_then(|t| t.iter().find(|c| c["id"] == "call_probe"))
            .filter(|c| c["status"] != "running");
        if let (true, Some(call)) = (done, call) {
            return Outcome {
                status: call["status"].as_str().unwrap_or_default().to_string(),
                class: call
                    .pointer("/failure/class")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                output: call["output"].as_str().unwrap_or_default().to_string(),
            };
        }
        assert!(
            Instant::now() < deadline,
            "probe {tag} never finished: {body}\n{}",
            node.log_tail()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn host_tool_probes_are_refused_under_the_profile_policy() {
    let started = Instant::now();
    let d = deployment(true);
    let llm = mock_llm();
    let node = start_node(
        &d,
        "1",
        None,
        "tool_allowlist = [\"host_files\"]\n",
        Some(llm.port),
    );
    for (user, name) in USERS.iter().enumerate().take(2) {
        provision(&client(), &node.base, name);
        let body = operator(
            &node,
            "openhuman.profiles_set_credential",
            json!({ "profile_id": name, "kind": "session", "token": credential(user) }),
        );
        assert!(body.get("result").is_some(), "{body}");
    }
    // Keyring secrets are namespaced by their store's directory name: a user
    // named like this node's operator directory would share its slots.
    let (_, body) = rpc_with(
        &client(),
        &node.base,
        Some(BEARER),
        "openhuman.profiles_provision",
        json!({ "user_id": "operator-1" }),
    );
    assert!(
        body.get("error").is_some() && !d.root.join("users/operator-1").exists(),
        "a profile named like the operator dir is refused: {body}"
    );
    // Bob's data, the operator's and the host's: everything alice must not read.
    let (_, _, body) = call(
        &node,
        USERS[1],
        "openhuman.threads_upsert",
        json!({
            "id": "bob-private", "title": canary(1, "title"), "created_at": "2026-10-10T00:00:00Z"
        }),
    );
    assert!(body.get("result").is_some(), "{body}");
    let bob_threads = d
        .root
        .join("users/bob/workspace/memory/conversations/threads.jsonl");
    assert!(
        std::fs::read_to_string(&bob_threads).is_ok_and(|t| t.contains("CANARY-bob-")),
        "bob's thread index at {}",
        bob_threads.display()
    );
    let host_secret = d.tmp.path().join("host-secret.txt");
    std::fs::write(&host_secret, "HOST-SECRET-6c1f").unwrap();
    let operator_secret = d.tmp.path().join("operator-1/operator-secret.txt");
    std::fs::write(&operator_secret, "OPERATOR-SECRET-9a2e").unwrap();
    let alice_config = d.root.join("users/alice/config.toml");
    let config_before = std::fs::read(&alice_config).ok();

    // Positive control: inside her sandbox the file tools work.
    let wrote = probe(
        &node,
        "own-write",
        "file_write",
        json!({ "path": "note.txt", "content": canary(0, "note") }),
    );
    let offered: Vec<String> = llm
        .recorded()
        .iter()
        .filter(|r| r.is_inference() && r.auth.contains(&credential(0)))
        .flat_map(|r| r.offered_tools())
        .collect();
    assert!(!offered.is_empty(), "alice's turns offer some tools");
    assert!(
        !offered.iter().any(|t| t == "shell"),
        "`host_shell` is not allowlisted, so `shell` is never offered: {offered:?}"
    );
    for denied in [
        "install_tool",
        "git_operations",
        "delegate",
        "curl",
        "node_exec",
    ] {
        assert!(
            !offered.iter().any(|t| t == denied),
            "{denied} is hard-denied: {offered:?}"
        );
    }
    let note = d.root.join("users/alice/sandbox/note.txt");
    assert!(
        std::fs::read_to_string(&note).is_ok_and(|t| t.contains("CANARY-alice-note")),
        "file_write lands in alice's sandbox: {wrote:?}"
    );
    let read = probe(
        &node,
        "own-read",
        "file_read",
        json!({ "path": "note.txt" }),
    );
    assert!(
        read.output.contains("CANARY-alice-note"),
        "file_read reads her sandbox: {read:?}"
    );

    // Hostile probes, in parallel: each on its own thread.
    let pwned_host = d.tmp.path().join("pwned-host.txt");
    let shell_marker = d.tmp.path().join("shell-pwned");
    let pwned_patch = d.tmp.path().join("patched-host.txt");
    // A link in alice's sandbox to bob's tree: the path policy must judge
    // where a path lands, not how it is spelled.
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        d.root.join("users/bob"),
        d.root.join("users/alice/sandbox/link-to-bob"),
    )
    .unwrap();
    let mut probes: Vec<(&str, &str, Value)> = vec![
        ("host-read", "file_read", json!({ "path": host_secret })),
        (
            "operator-read",
            "file_read",
            json!({ "path": operator_secret }),
        ),
        ("bob-abs-read", "file_read", json!({ "path": bob_threads })),
        (
            "bob-rel-read",
            "file_read",
            json!({ "path": "../../bob/workspace/memory/conversations/threads.jsonl" }),
        ),
        (
            "own-ws-read",
            "file_read",
            json!({ "path": "../workspace/memory/conversations/threads.jsonl" }),
        ),
        (
            "bob-grep",
            "grep",
            json!({ "pattern": "CANARY", "path": d.root.join("users/bob") }),
        ),
        ("root-list", "list", json!({ "path": d.root.join("users") })),
        (
            "host-write",
            "file_write",
            json!({ "path": pwned_host, "content": canary(0, "pwn") }),
        ),
        (
            "bob-write",
            "file_write",
            json!({ "path": "../../bob/sandbox/pwned.txt", "content": canary(0, "pwn") }),
        ),
        (
            "config-write",
            "file_write",
            json!({ "path": "../config.toml", "content": "[autonomy]\nenabled = false\n" }),
        ),
        (
            "bob-patch",
            "apply_patch",
            json!({ "edits": [{
            "path": "../../bob/sandbox/patched.txt", "old_string": "", "new_string": canary(0, "pwn")
        }] }),
        ),
        (
            "host-patch",
            "apply_patch",
            json!({ "edits": [{
            "path": pwned_patch, "old_string": "", "new_string": canary(0, "pwn")
        }] }),
        ),
        (
            "shell",
            "shell",
            json!({ "command": format!("touch {}", shell_marker.display()) }),
        ),
    ];
    // The link above only exists on Unix, so only Unix probes through it.
    #[cfg(unix)]
    probes.push((
        "symlink-read",
        "file_read",
        json!({ "path": "link-to-bob/workspace/memory/conversations/threads.jsonl" }),
    ));
    let results: Vec<(&str, &str, Outcome)> = std::thread::scope(|s| {
        let handles: Vec<_> = probes
            .iter()
            .map(|(tag, tool, args)| {
                let node = &node;
                s.spawn(move || (*tag, *tool, probe(node, tag, tool, args.clone())))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut report = Vec::new();
    for (tag, tool, outcome) in &results {
        report.push(format!(
            "  {tag}: {} {} {}",
            outcome.status,
            outcome.class,
            outcome.output.chars().take(110).collect::<String>()
        ));
        for secret in ["HOST-SECRET", "OPERATOR-SECRET", "CANARY-bob-"] {
            assert!(
                !outcome.output.contains(secret),
                "probe {tag} read {secret}: {outcome:?}"
            );
        }
        assert_eq!(
            outcome.status, "error",
            "probe {tag} was not refused: {outcome:?}"
        );
        if offered.iter().any(|t| t == tool)
            || ["file_read", "file_write", "apply_patch"].contains(tool)
        {
            // A tool the user has: refused by the profile's path policy.
            // (`apply_patch` reports the same path check per edit.)
            assert!(
                outcome.class == "BlockedByPolicy" || outcome.output.contains("path not allowed"),
                "probe {tag} was not refused by the path policy: {outcome:?}"
            );
        } else {
            // A tool the user does not have: it does not exist for them.
            assert!(
                outcome.output.contains("unknown tool"),
                "probe {tag}: {outcome:?}"
            );
        }
    }
    eprintln!("[isolation] offered to alice: {offered:?}");
    eprintln!("[isolation] probe outcomes:\n{}", report.join("\n"));
    assert!(!pwned_host.exists(), "a user wrote a host file");
    assert!(
        !d.root.join("users/bob/sandbox/pwned.txt").exists(),
        "alice wrote into bob's sandbox"
    );
    assert!(!shell_marker.exists(), "a user ran a host shell command");
    assert!(!pwned_patch.exists(), "a user patched a host file");
    assert!(
        !d.root.join("users/bob/sandbox/patched.txt").exists(),
        "alice patched bob's sandbox"
    );
    assert_eq!(
        std::fs::read(&alice_config).ok(),
        config_before,
        "a user rewrote their own profile config"
    );
    let (violations, _) = scan::audit_backend(&llm.recorded());
    assert!(violations.is_empty(), "{}", violations.join("\n"));
    eprintln!(
        "[isolation] probes took {:.1}s",
        started.elapsed().as_secs_f64()
    );
}
