//! CLI smoke tests for PostgreSQL subcommands via the main warden binary
//! Ensures the warden CLI exposes and responds to PostgreSQL commands as expected.

use assert_cmd::Command;
use predicates::prelude::*;

fn warden_bin() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "-q", "-p", "warden", "--"]);
    cmd
}

#[test]
fn prints_postgres_help() {
    let mut cmd = warden_bin();
    cmd.args(["postgresql", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("backup").and(predicate::str::contains("restore")));
}

#[test]
fn rejects_postgres_unknown_command() {
    let mut cmd = warden_bin();
    cmd.args(["postgresql", "not-a-real-command"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
