#[cfg(unix)]
#[test]
fn hook_adds_openhuman_trailer_without_disabling_repository_hook() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.com"]);
    let own_hook = repo.join(".git/hooks/prepare-commit-msg");
    std::fs::write(
        &own_hook,
        "#!/bin/sh\nprintf 'repo-hook-ran\\n' >> \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&own_hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(repo.join("a"), "a").unwrap();
    git(&["add", "a"]);

    let hook_env = super::hook::test_hook_env(Some(std::ffi::OsStr::new(
        "'test.openhuman-inherited'='kept' 'core.hooksPath'='/definitely-not-the-openhuman-hook'",
    )));
    let output = Command::new("git")
        .args(["commit", "-q", "-m", "subject"])
        .current_dir(&repo)
        .envs(&hook_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new("git")
        .args(["log", "-1", "--format=%B"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let message = String::from_utf8(output.stdout).unwrap();
    assert!(message.contains("repo-hook-ran"), "{message:?}");
    assert!(message.contains(super::hook::TRAILER), "{message:?}");

    let inherited = Command::new("git")
        .args(["config", "--get", "test.openhuman-inherited"])
        .current_dir(&repo)
        .envs(&hook_env)
        .output()
        .unwrap();
    assert!(inherited.status.success());
    assert_eq!(String::from_utf8(inherited.stdout).unwrap().trim(), "kept");
}

#[cfg(unix)]
#[test]
fn hook_env_does_not_drop_inherited_parameters_containing_non_utf8() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let inherited_bytes = b"'test.openhuman-inherited'='before-\xff-after' 'test.second'='kept'";
    let inherited = std::ffi::OsStr::from_bytes(inherited_bytes);
    let hook_env = super::hook::test_hook_env(Some(inherited));
    let parameters = hook_env
        .get(std::ffi::OsStr::new("GIT_CONFIG_PARAMETERS"))
        .unwrap()
        .clone()
        .into_vec();

    assert!(parameters.starts_with(inherited_bytes));
    assert_eq!(parameters[inherited_bytes.len()], b' ');
    assert!(parameters[inherited_bytes.len() + 1..].starts_with(b"'core.hooksPath'='"));
    assert!(parameters.contains(&0xff));

    let output = std::process::Command::new("git")
        .args(["config", "--get", "test.openhuman-inherited"])
        .envs(&hook_env)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"before-\xff-after\n");
}
