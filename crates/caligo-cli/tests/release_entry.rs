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
