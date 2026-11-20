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
fn observation_stop_rejects_bad_flags_before_process_access() {
    let run=|args:&[&str]|{
        let out=Command::new(env!("CARGO_BIN_EXE_caligo")).args(args).output().unwrap();
        (out.status.code(),String::from_utf8_lossy(&out.stderr).into_owned(),String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let valid=["cli","qq-entry-stop","--created-filetime","1","--stop-observe-ms","100","--wait-ms","1000"];
    let (code,err,out)=run(&valid);
    if cfg!(feature="research") { assert_eq!(code,Some(1),"{err}{out}"); }
    else { assert_eq!(code,Some(3),"{err}{out}"); }
    assert!(!out.contains("[gate 1]"));
    for flags in [vec!["--stop-observe-ms"],vec!["--stop-observe-ms","-1"],
        vec!["--stop-observe-ms","100","--stop-observe-ms","100"],
        vec!["--stop-observe-ms","100","--wait-ms","bad"]] {
        let mut args=vec!["cli","qq-entry-stop","--created-filetime","1"];args.extend(flags);
        let (code,err,out)=run(&args);assert_eq!(code,Some(1),"{err}{out}");assert!(!out.contains("[gate 1]"));
    }
    if cfg!(feature="research") {
        for (duration,wait) in [("0","1000"),("5001","10000"),("100","349"),("100","30001")] {
            let (code,err,out)=run(&["cli","qq-entry-stop","--created-filetime","1","--stop-observe-ms",duration,"--wait-ms",wait]);
            assert_eq!(code,Some(1),"{err}{out}");assert!(err.contains("requires managed research stop"));assert!(!out.contains("[gate 1]"));
        }
        let (code,err,out)=run(&["cli","qq-entry","--created-filetime","1","--stop-observe-ms","100"]);
        assert_eq!(code,Some(1),"{err}{out}");assert!(!out.contains("[gate 1]"));
        let (code,err,out)=run(&["cli","qq-entry-stop","--stop-observe-ms","100"]);
        assert_eq!(code,Some(1),"{err}{out}");assert!(err.contains("requires managed research stop"));
        let (code,err,out)=run(&["cli","qq-entry-stop","--report","--created-filetime","--pid","1",
            "--bridge","unused","--manifest","unused","--stop-observe-ms","100","--wait-ms","1000"]);
        assert_eq!(code,Some(1),"{err}{out}");assert!(err.contains("requires managed research stop"),"{err}{out}");
        assert!(!out.contains("[gate 1]"),"consumed FILETIME token cannot grant managed admission");
    }
    let (code,err,out)=run(&["cli","qq-status","--stop-observe-ms","100"]);
    assert_eq!(code,Some(1),"{err}{out}");assert!(err.contains("only valid for qq-entry-stop"));
}

#[test]
fn observation_stop_requires_confirmation_and_exact_process_generation() {
    if !cfg!(feature="research") { return; }
    let root=std::env::temp_dir().join(format!("caligo-observe-gate-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();let manifest=root.join("manifest.json");let report=root.join("entry.jsonl");
    std::fs::write(&manifest,r#"{"baseline_id":"lab","modules":[]}"#).unwrap();
    for confirmed in [false,true] {
        let mut command=Command::new(env!("CARGO_BIN_EXE_caligo"));
        command.args(["cli","qq-entry-stop","--pid",&std::process::id().to_string(),"--created-filetime","1",
            "--stop-observe-ms","100","--wait-ms","1000","--bridge"])
            .arg(root.join("caligo_bridge.dll")).arg("--manifest").arg(&manifest).arg("--report").arg(&report);
        if confirmed { command.arg("--confirm-designated-test-instance"); }
        let out=command.output().unwrap();assert_eq!(out.status.code(),Some(2));
        let err=String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(if confirmed {"creation time changed or unavailable"} else {"缺少 --confirm-designated-test-instance"}),"{err}");
        assert!(!report.exists());
    }
    std::fs::remove_file(manifest).unwrap();std::fs::remove_dir(root).unwrap();
}

#[cfg(feature = "research")]
#[test]
fn observed_stop_parallel_manifest_matches_serial_checks_before_confirmation() {
    use serde_json::json;
    use std::{fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!("caligo-stop-manifest-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let manifest = root.join("manifest.json");
    let report = root.join("entry.jsonl");
    let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let mut paths = Vec::new();
    let checked: Vec<_> = (0..5).map(|index| {
        let path = root.join(format!("part-{index}.bin"));
        fs::write(&path, b"abc").unwrap();
        paths.push(path.clone());
        json!({"role":format!("checked-{index}"),"path":path,"sha256":hash,"size_bytes":3})
    }).collect();
    let mut modules = checked;
    modules.insert(1, json!({"role":"skip","path":root.join("unchecked.bin"),"sha256":"","size_bytes":0,"hash_checked":false}));
    let reports = |stdout: String| stdout.lines().filter(|line| {
        ["[PASS]", "[FAIL]", "[MISSING]", "[SKIP   ]"].iter().any(|prefix| line.starts_with(prefix))
    }).map(str::to_owned).collect::<Vec<_>>();

    let verify = |modules: &[serde_json::Value], passes: bool| {
        fs::write(&manifest, json!({"baseline_id":"lab","modules":modules}).to_string()).unwrap();
        let serial = Command::new(env!("CARGO_BIN_EXE_caligo"))
            .args(["cli", "verify-manifest"]).arg(&manifest).output().unwrap();
        let parallel = Command::new(env!("CARGO_BIN_EXE_caligo")).args([
            "cli", "qq-entry-stop", "--pid", "1", "--created-filetime", "1",
            "--stop-observe-ms", "100", "--wait-ms", "1000", "--bridge",
        ]).arg(root.join("caligo_bridge.dll")).arg("--manifest").arg(&manifest)
            .arg("--report").arg(&report).output().unwrap();
        let err = String::from_utf8(parallel.stderr).unwrap();
        let out = String::from_utf8(parallel.stdout).unwrap();
        assert_eq!(serial.status.code(), Some(if passes { 0 } else { 2 }));
        assert_eq!(parallel.status.code(), Some(2));
        assert_eq!(reports(String::from_utf8(serial.stdout).unwrap()), reports(out.clone()));
        assert!(out.contains("[stop manifest] elapsed_us="));
        assert!(err.contains(if passes { "缺少 --confirm-designated-test-instance" } else { "[gate 1] REJECT" }), "{err}");
        assert!(!report.exists());
    };
    verify(&modules, true);
    for (field, value) in [("sha256", json!("bad")), ("size_bytes", json!(4)),
                          ("path", json!(root.join("missing.bin")))] {
        let mut rejected = modules.clone();
        rejected[5][field] = value;
        verify(&rejected, false);
    }
    for path in paths.into_iter().chain([PathBuf::from(manifest)]) {
        fs::remove_file(path).unwrap();
    }
    fs::remove_dir(root).unwrap();
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
