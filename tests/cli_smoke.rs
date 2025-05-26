//! Smoke tests for the main warden CLI binary
//! These tests check that the CLI parses arguments and responds to help/version commands.

use assert_cmd::Command;
use predicates::prelude::*;

/// Returns the path to the CLI binary (builds if needed)
fn cli_bin() -> Command {
    Command::cargo_bin("warden").expect("binary should build")
}

#[test]
fn prints_help() {
    let mut cmd = cli_bin();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn prints_version() {
    let mut cmd = cli_bin();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("warden"));
}

#[test]
fn rejects_unknown_command() {
    let mut cmd = cli_bin();
    cmd.arg("not-a-real-command");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
