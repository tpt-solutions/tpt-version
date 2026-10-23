//! The tpt binary, run as a process against a scratch store.
#![cfg(feature = "cli")]

use std::path::PathBuf;
use std::process::{Command, Output};

fn tpt(dir: &PathBuf, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tpt"))
        .arg("--dir")
        .arg(dir)
        .args(args)
        .output()
        .expect("tpt runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn commit_get_and_diff_round_trip() {
    let dir = std::env::temp_dir().join(format!("tpt-cli-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    assert!(tpt(&dir, &["init"]).status.success());
    assert!(
        !tpt(&dir, &["init"]).status.success(),
        "second init refuses"
    );
    assert!(tpt(&dir, &["commit", "one", "a=1", "b=2"]).status.success());
    assert!(tpt(&dir, &["branch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "two", "a=5"]).status.success());
    assert!(tpt(&dir, &["switch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "alt", "c=3", "b=9"]).status.success());

    assert_eq!(stdout(&tpt(&dir, &["get", "b"])), "9\n");

    let diff = tpt(&dir, &["diff", "main", "alt"]);
    let text = stdout(&diff);
    assert!(text.contains("added c=3"), "{text}");
    assert!(text.contains("changed b value: \"2\" -> \"9\""), "{text}");
    assert!(text.contains("changed a value: \"5\" -> \"1\""), "{text}");

    let unknown = tpt(&dir, &["diff", "main", "nosuch"]);
    assert!(!unknown.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}
