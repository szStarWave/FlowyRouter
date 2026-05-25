use std::process::Command;

#[test]
fn gateway_subcommands_are_registered() {
    let out = Command::new(env!("CARGO_BIN_EXE_flowy"))
        .args(["gateway", "--help"])
        .output()
        .expect("run flowy gateway --help");

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout);
    for cmd in ["start", "stop", "status", "restart", "run"] {
        assert!(help.contains(cmd), "missing subcommand `{cmd}` in:\n{help}");
    }
}

#[test]
fn stats_is_registered() {
    let out = Command::new(env!("CARGO_BIN_EXE_flowy"))
        .args(["stats", "--help"])
        .output()
        .expect("run flowy stats --help");

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("json"), "missing --json in:\n{help}");
    assert!(help.contains("global"), "missing --global in:\n{help}");
    assert!(help.contains("lang"), "missing --lang in:\n{help}");
}

#[test]
fn env_prints_paths() {
    let out = Command::new(env!("CARGO_BIN_EXE_flowy"))
        .args(["env"])
        .output()
        .expect("run flowy env");

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    for key in ["user_home:", "config_file:", "gateway_log:", "gateway_url:"] {
        assert!(text.contains(key), "missing `{key}` in:\n{text}");
    }
}
