use std::process::Command;

fn noa_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_noa"))
}

fn noa_server_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_noa-server"))
}

#[test]
fn test_noa_version_contains_details() {
    let output = noa_bin().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("noa "));
    assert!(stdout.contains("Authors:"));
    assert!(stdout.contains("License:"));
    assert!(stdout.contains("Repository:"));
    assert!(stdout.contains("https://github.com/celestia-island/noa"));
    assert!(stdout.contains("https://docs.rs/libnoa"));
}

#[test]
fn test_noa_no_args_shows_usage() {
    let output = noa_bin().output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Usage: noa"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("init"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("log"));
    assert!(stdout.contains("snapshot"));
    assert!(stdout.contains("workspace"));
    assert!(stdout.contains("push"));
    assert!(stdout.contains("pull"));
    assert!(stdout.contains("clone"));
}

#[test]
fn test_noa_help_shows_usage() {
    let output = noa_bin().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Usage: noa"));
    assert!(stdout.contains("AI-native distributed version control system"));
    assert!(stdout.contains("Run 'noa <COMMAND> --help'"));
}

#[test]
fn test_noa_server_version_contains_details() {
    let output = noa_server_bin().arg("--version").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("noa-server "));
    assert!(stdout.contains("Authors:"));
    assert!(stdout.contains("License:"));
    assert!(stdout.contains("Repository:"));
    assert!(stdout.contains("https://github.com/celestia-island/noa"));
    assert!(stdout.contains("https://docs.rs/libnoa"));
}

#[test]
fn test_noa_server_help_shows_usage() {
    let output = noa_server_bin().arg("--help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Usage: noa-server"));
    assert!(stdout.contains("--db-path"));
    assert!(stdout.contains("--port"));
    assert!(stdout.contains("Server for the noa distributed version control system"));
}

#[test]
fn test_noa_subcommand_help() {
    let output = noa_bin().args(["init", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("Usage: noa") && stdout.contains("init"));

    let output = noa_bin().args(["log", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("--workspace"));
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--tui"));
}
