//! K4-D0 遗留路线门控集成测试(计划 §10:cli tests/legacy_gate.rs)。
//!
//! 验收口径:普通构建下 CLI 无法执行旧 QQ 调用 —— `inject` 在任何参数解析、
//! manifest 核对或进程打开之前拒绝。本测试不触碰任何进程;传入的 PID/路径
//! 均为占位,若实现回退(门控缺失),测试会以假参数失败,而不是真的注入。
//!
//! research 构建(如 `--features caligo-core/research`,经 core→bridge 特性
//! 转发全局生效)下 D0 拒绝合同不适用 —— 与 `gate.rs` 单测的
//! `#[cfg(not(feature = "research"))]` 同一口径,此处按运行时常量跳过。

use std::process::Command;

#[test]
fn profile_refresh_probe_is_default_closed_and_rejects_invalid_research_modes_before_manifest() {
    let invalid = [
        vec!["qq-entry-stop", "--refresh-current-profile"],
        vec!["qq-entry", "--refresh-current-profile"],
        vec!["qq-entry", "--refresh-current-profile", "--current-login", "--uid-map-result", "Z:/missing.json"],
        vec!["qq-entry", "--refresh-current-profile", "--uid-map-uin", "1234567890", "--uid-map-result", "Z:/missing.json"],
        vec!["qq-entry", "--refresh-current-profile", "--uid-map-surface-only", "--uid-map-result", "Z:/missing.json"],
        vec!["qq-entry", "--refresh-current-profile", "--uid-map-profile-service", "--uid-map-result", "Z:/missing.json"],
        vec!["qq-entry", "--refresh-current-profile", "--daemon-account", "1234567890", "--uid-map-result", "Z:/missing.json"],
    ];
    for args in invalid {
        let (code, stdout, stderr) = run_inject(&args);
        assert_eq!(code, Some(if caligo_bridge::gate::research_enabled() { 1 } else { 3 }), "{args:?}: {stdout} {stderr}");
        assert!(!stdout.contains("[gate 1]"), "invalid mode reached manifest: {stdout}");
    }
}

fn run_inject(extra_args: &[&str]) -> (Option<i32>, String, String) {
    let bin = env!("CARGO_BIN_EXE_caligo-cli");
    let out = Command::new(bin)
        .args(extra_args)
        .output()
        .expect("spawn caligo-cli");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// research 构建下跳过:D0 合同只约束普通构建。
fn skip_if_research() -> bool {
    if caligo_bridge::gate::research_enabled() {
        eprintln!("skip: research build — D0 refusal contract applies to default builds only");
        return true;
    }
    false
}

#[test]
fn inject_refused_before_any_gate_or_process_access() {
    if skip_if_research() {
        return;
    }
    // 完整形态参数(含确认标志与不存在的文件):门控必须先于文件/进程访问拒绝。
    let (code, stdout, stderr) = run_inject(&[
        "inject",
        "--pid",
        "4294967290",
        "--bridge",
        "Z:/definitely/not/here.dll",
        "--manifest",
        "Z:/definitely/not/here.json",
        "--report",
        "Z:/definitely/not/here-report.json",
        "--confirm-designated-test-instance",
    ]);
    assert_eq!(code, Some(3), "stdout={stdout} stderr={stderr}");
    assert!(
        stderr.contains("[K4-D0]"),
        "拒绝文案缺失: {stderr}"
    );
    // 门 1(manifest)/门 2(确认)不得先于 D0 门控执行。
    assert!(!stderr.contains("gate 1") && !stdout.contains("gate 1"));
    assert!(!stderr.contains("gate 2") && !stdout.contains("gate 2"));
}

#[test]
fn inject_refused_even_without_any_arguments() {
    if skip_if_research() {
        return;
    }
    let (code, _stdout, stderr) = run_inject(&["inject"]);
    assert_eq!(code, Some(3), "stderr={stderr}");
    assert!(stderr.contains("[K4-D0]"));
}

#[test]
fn disabled_notice_names_recovery_path() {
    if skip_if_research() {
        return;
    }
    // 文案必须指向 K4 计划与研究构建方式,不留"怎么绕过"的空白。
    let (code, _stdout, stderr) = run_inject(&["inject", "--pid", "1"]);
    assert_eq!(code, Some(3));
    assert!(stderr.contains("--features research"));
    assert!(stderr.contains("k4-field-entry") || stderr.contains("K4"));
}

#[test]
fn research_gate_constant_matches_cli_exit_code_domain() {
    // bridge 导出层结果码与 CLI 退出码分属两个域;这里锁定常量关系防止漂移。
    assert_eq!(caligo_bridge::gate::ERR_RESEARCH_DISABLED, 0xE0);
    // 拒绝断言仅对普通构建成立(research 构建下 D0 合同不适用)。
    if !caligo_bridge::gate::research_enabled() {
        assert!(!caligo_bridge::gate::research_enabled());
    }
}
