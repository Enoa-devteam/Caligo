use std::process::Command;

#[test]
fn release_entry_routes_each_command() {
    for (args, code, expected) in [
        (&[][..], 0, "caligo.exe daemon"),
        (&["--help"][..], 0, "caligo.exe manager"),
        (
            &["manage", "--help"][..],
            0,
            "caligo.exe manage [--terminal]",
        ),
        (
            &["manage", "--root", "relative"][..],
            2,
            "--root must be an absolute path",
        ),
        (&["daemon"][..], 2, "caligo.exe daemon --pipe-prefix"),
        (
            &["manager", "--root", "relative"][..],
            2,
            "--root must be an absolute path",
        ),
        (&["media", "--help"][..], 2, "caligo.exe media --config"),
        (&["cli", "--help"][..], 0, "caligo.exe cli verify-manifest"),
        (&["cli", "exports"][..], 1, "caligo.exe cli exports"),
        (&["onebotd"][..], 1, "Unknown command: onebotd"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_caligo"))
            .args(args)
            .output()
            .expect("run caligo executable");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(code), "{args:?}: {stdout}{stderr}");
        assert!(
            stdout.contains(expected) || stderr.contains(expected),
            "{args:?}: expected {expected:?} in {stdout}{stderr}"
        );
    }
}

#[test]
fn managed_release_attach_rejects_unconfirmed_and_stale_targets_before_loading() {
    let root = std::env::temp_dir().join(format!("caligo-attach-gate-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let manifest = root.join("manifest.json");
    let auth = root.join("auth.token");
    let bridge = root.join("caligo_bridge.dll");
    let report = root.join("report.jsonl");
    std::fs::write(&manifest, r#"{"baseline_id":"lab","modules":[]}"#).unwrap();
    std::fs::write(&auth, "a".repeat(64)).unwrap();

    let run = |confirmed: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_caligo"));
        command.args([
            "cli", "qq-entry", "--pid", &std::process::id().to_string(),
            "--created-filetime", "1", "--bridge",
        ]);
        command.arg(&bridge).arg("--manifest").arg(&manifest)
            .arg("--report").arg(&report).arg("--daemon-auth-file").arg(&auth)
            .args(["--daemon-pipe", r"\\.\pipe\caligo-attach-test-bridge", "--daemon-account", "10001"]);
        if confirmed {
            command.arg("--confirm-designated-test-instance");
        }
        let out = command.output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(!report.exists());
        String::from_utf8(out.stderr).unwrap()
    };
    assert!(run(false).contains("缺少 --confirm-designated-test-instance"));
    assert!(run(true).contains("creation time changed or unavailable"));
    std::fs::remove_file(auth).unwrap();
    std::fs::remove_file(manifest).unwrap();
    std::fs::remove_dir(root).unwrap();
}
