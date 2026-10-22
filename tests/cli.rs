#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! End-to-end tests of the `astfuzz` binary.

use std::fs;
use std::process::{Command, Output};

fn astfuzz(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_astfuzz")).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn run_with_planted_bug_exits_1_and_writes_a_structured_report() {
    let out = tempfile::tempdir().unwrap();
    let o = astfuzz(&[
        "run",
        "--seed",
        "1",
        "--iterations",
        "20000",
        "--plant",
        "opt-sub-assoc",
        "--stop-on-first",
        "--out",
        out.path().to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.path().join("report.json")).unwrap()).unwrap();
    let f = &report["findings"][0];
    assert_eq!(f["failure"]["class"], "invariant-violation");
    assert_eq!(f["failure"]["signature"], "invariant-violation:optimizer-equivalence");
    assert_eq!(f["seed"], 1);
    assert!(f["iteration"].is_u64());
    assert!(f["reproducer"].as_str().unwrap().contains("print"));
    assert!(f["minimize_stats"]["minimized_size"].as_u64().unwrap() <= 10);
    assert_eq!(report["planted"][0], "opt-sub-assoc");
    assert!(out.path().join("finding-0.mini").exists());
    assert!(out.path().join("finding-0.min.mini").exists());
}

#[test]
fn clean_run_exits_0() {
    let o = astfuzz(&["run", "--seed", "3", "--iterations", "2000"]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert!(stdout(&o).contains("0 unique finding(s)"));
}

#[test]
fn replay_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("p.mini");
    fs::write(&f, "print 0 - (0 - 1);").unwrap();
    let f = f.to_str().unwrap();
    let ok = astfuzz(&["replay", f]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(stdout(&ok).contains("output: [Int(1)]"));
    let bad =
        astfuzz(&["replay", "--plant", "interp-rem-panic", "fixtures/regressions/interp-rem-panic.mini"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(stdout(&bad).contains("FAIL crash:interp:"));
}

#[test]
fn minimize_refuses_a_passing_program_and_shrinks_a_failing_one() {
    let dir = tempfile::tempdir().unwrap();
    let pass = dir.path().join("pass.mini");
    fs::write(&pass, "print 1;").unwrap();
    let o = astfuzz(&["minimize", pass.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("does not fail"));

    let fail = dir.path().join("fail.mini");
    fs::write(&fail, "let a = 5; print 1; print a - (a - 3); print 2;").unwrap();
    let o = astfuzz(&["minimize", "--plant", "opt-sub-assoc", fail.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0));
    let min = stdout(&o);
    assert!(!min.contains("print 1;") && !min.contains("print 2;"), "{min}");
}

#[test]
fn errors_are_actionable_and_exit_2() {
    let o = astfuzz(&["replay", "/definitely/not/here.mini"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("reading /definitely/not/here.mini"));
    let o = astfuzz(&["run", "--plant", "nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("expected one of"));
}

#[test]
fn corpus_dedup_never_writes_into_its_input() {
    let o = astfuzz(&["corpus", "dedup", "corpus", "--out", "corpus"]);
    assert_eq!(o.status.code(), Some(2));
    let out = tempfile::tempdir().unwrap();
    let o = astfuzz(&["corpus", "dedup", "corpus", "--out", out.path().to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 3);
}

#[test]
fn gen_is_deterministic() {
    let a = stdout(&astfuzz(&["gen", "--seed", "4", "--count", "3"]));
    let b = stdout(&astfuzz(&["gen", "--seed", "4", "--count", "3"]));
    assert_eq!(a, b);
    assert!(a.contains("# seed 4 program 2"));
}
