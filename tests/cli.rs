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

#[test]
fn conflicted_merge_survives_between_runs() {
    let dir = std::env::temp_dir().join(format!("tpt-cli-merge-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    assert!(tpt(&dir, &["init"]).status.success());
    assert!(
        tpt(&dir, &["commit", "base", "a=1", "b=1"])
            .status
            .success()
    );
    assert!(tpt(&dir, &["branch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "main side", "a=2"]).status.success());
    assert!(tpt(&dir, &["switch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "alt side", "a=3"]).status.success());

    let merge = tpt(&dir, &["merge", "main", "merged"]);
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(stdout(&merge).contains("merge left 1 conflict(s) pending"));

    // Nothing may be committed while the conflict is pending, in this run or the next.
    assert!(!tpt(&dir, &["commit", "too soon", "b=2"]).status.success());
    assert!(!tpt(&dir, &["finish-merge", "too soon"]).status.success());

    // Garbage collection must keep the pending merge's objects.
    assert!(tpt(&dir, &["gc"]).status.success());
    let listed = stdout(&tpt(&dir, &["conflicts"]));
    assert!(listed.contains("a: ours \"3\" | theirs \"2\""), "{listed}");

    assert!(tpt(&dir, &["resolve", "a", "theirs"]).status.success());
    assert!(!tpt(&dir, &["resolve", "b", "ours"]).status.success());
    assert!(tpt(&dir, &["finish-merge", "merged"]).status.success());

    assert_eq!(stdout(&tpt(&dir, &["get", "a"])), "2\n");
    assert!(stdout(&tpt(&dir, &["log"])).contains("merged"));
    assert!(!stdout(&tpt(&dir, &["branches"])).contains("merges"));
    assert!(!tpt(&dir, &["finish-merge", "again"]).status.success());
    assert!(!tpt(&dir, &["branch", "merges/alt"]).status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefer_flag_settles_every_conflict_in_one_run() {
    let dir = std::env::temp_dir().join(format!("tpt-cli-prefer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    assert!(tpt(&dir, &["init"]).status.success());
    assert!(tpt(&dir, &["commit", "base", "a=1"]).status.success());
    assert!(tpt(&dir, &["branch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "main side", "a=2"]).status.success());
    assert!(tpt(&dir, &["switch", "alt"]).status.success());
    assert!(tpt(&dir, &["commit", "alt side", "a=3"]).status.success());

    assert!(
        tpt(&dir, &["merge", "main", "ours", "kept"])
            .status
            .success()
    );
    assert_eq!(stdout(&tpt(&dir, &["get", "a"])), "3\n");
    assert!(stdout(&tpt(&dir, &["conflicts"])).contains("no conflicts"));

    let _ = std::fs::remove_dir_all(&dir);
}
