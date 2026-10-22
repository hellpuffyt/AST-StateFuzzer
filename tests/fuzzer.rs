#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Fuzzer behaviour: determinism, oracles, crash capture, minimization and the
//! planted-bug acceptance tests.

use astfuzz::fuzz::engine::{FuzzConfig, Report, run};
use astfuzz::fuzz::exec::{ExecConfig, FailureClass, check};
use astfuzz::fuzz::generator::{GenConfig, generate};
use astfuzz::fuzz::minimize::minimize;
use astfuzz::fuzz::rng::{Rng, Source};
use astfuzz::lang::ast::program_size;
use astfuzz::lang::interp::Limits;
use astfuzz::lang::parser::parse;
use astfuzz::lang::printer::print_program;
use astfuzz::planted::{Bugs, PlantedBug};

fn exec_with(b: PlantedBug) -> ExecConfig {
    ExecConfig { bugs: Bugs::NONE.with(b), ..ExecConfig::default() }
}

fn campaign(seed: u64, iterations: u64, exec: &ExecConfig) -> Report {
    let cfg = FuzzConfig { seed, iterations, stop_on_first: true, ..FuzzConfig::default() };
    run(&cfg, exec, &[])
}

#[test]
fn rng_stream_is_pinned() {
    // If these change, every published seed stops reproducing: treat as a breaking change.
    let mut r = Rng::new(0);
    let first: Vec<u64> = (0..3).map(|_| r.next_u64()).collect();
    assert_eq!(first, vec![0x99EC_5F36_CB75_F2B4, 0xBF6E_1F78_4956_452A, 0x1A5F_849D_4933_E6E0]);
}

#[test]
fn same_seed_same_programs() {
    let cfg = GenConfig::default();
    for i in 0..50 {
        let a = print_program(&generate(&mut Rng::fork(9, i), &cfg));
        let b = print_program(&generate(&mut Rng::fork(9, i), &cfg));
        assert_eq!(a, b);
    }
    let a = print_program(&generate(&mut Rng::fork(9, 0), &cfg));
    let c = print_program(&generate(&mut Rng::fork(10, 0), &cfg));
    assert_ne!(a, c, "different seeds should differ");
}

#[test]
fn same_seed_same_campaign() {
    let exec = exec_with(PlantedBug::OptSubAssoc);
    let cfg = FuzzConfig { seed: 5, iterations: 3_000, ..FuzzConfig::default() };
    let a = run(&cfg, &exec, &[]);
    let b = run(&cfg, &exec, &[]);
    let strip = |r: &Report| {
        let mut v = serde_json::to_value(r).unwrap();
        v["stats"]["elapsed_ms"] = serde_json::Value::Null;
        v
    };
    assert_eq!(strip(&a), strip(&b));
    assert!(!a.findings.is_empty());
}

#[test]
fn correct_implementation_has_no_findings() {
    // The oracles must not cry wolf: the unmodified implementation passes.
    for seed in [11, 12, 13] {
        let cfg = FuzzConfig { seed, iterations: 8_000, ..FuzzConfig::default() };
        let r = run(&cfg, &ExecConfig::default(), &[]);
        assert!(r.findings.is_empty(), "seed {seed}: unexpected {:#?}", r.findings[0]);
        assert!(r.stats.corpus_size > 10, "coverage feedback should grow a corpus");
    }
}

/// The acceptance test: each planted bug is found from a fixed seed within a
/// bounded budget, classified correctly, and minimized to a small reproducer
/// that still fails the same way.
#[test]
fn planted_bugs_are_found_and_minimized() {
    let cases = [
        (PlantedBug::OptSubAssoc, "invariant-violation:optimizer-equivalence", 10),
        (PlantedBug::PrintSubParens, "invariant-violation:roundtrip-ast", 8),
        (PlantedBug::TypeckEqMixed, "invariant-violation:type-soundness", 6),
        (PlantedBug::InterpRemPanic, "crash:interp:", 10),
    ];
    for (bug, signature, max_nodes) in cases {
        let exec = exec_with(bug);
        let r = campaign(1, 20_000, &exec);
        let f = r.findings.first().unwrap_or_else(|| panic!("{bug} not found within budget"));
        assert!(f.failure.signature.starts_with(signature), "{bug}: got {}", f.failure.signature);
        let min = f.minimized.as_ref().unwrap();
        let st = f.minimize_stats.as_ref().unwrap();
        assert!(st.minimized_size <= max_nodes, "{bug}: minimized to {} nodes:\n{min}", st.minimized_size);
        assert!(st.minimized_size < st.original_size || st.original_size <= max_nodes);
        // The minimized program replays to the same signature.
        let v = check(&parse(min).unwrap(), &exec);
        assert_eq!(v.failure.unwrap().signature, f.failure.signature, "{bug}: minimized program diverged");
        // ...and passes with the bug switched off (so it is about that bug).
        assert!(check(&parse(min).unwrap(), &ExecConfig::default()).failure.is_none());
    }
}

#[test]
fn crash_is_captured_with_location_and_normalized_signature() {
    let exec = exec_with(PlantedBug::InterpRemPanic);
    let a = check(&parse("print -7 % 3;").unwrap(), &exec).failure.unwrap();
    let b = check(&parse("print -100 % 7;").unwrap(), &exec).failure.unwrap();
    assert_eq!(a.class, FailureClass::Crash);
    assert!(a.signature.contains("src/lang/interp.rs:"), "{}", a.signature);
    assert!(a.detail.contains("index out of bounds"));
    assert_eq!(a.signature, b.signature, "different indices must share a signature");
}

#[test]
fn timeouts_are_classified_and_opt_in() {
    let p = parse("while true { }").unwrap();
    let quiet = check(&p, &ExecConfig::default());
    assert!(quiet.failure.is_none() && quiet.timed_out);
    let loud = ExecConfig { timeouts_are_failures: true, ..ExecConfig::default() };
    let f = check(&p, &loud).failure.unwrap();
    assert_eq!((f.class, f.signature.as_str()), (FailureClass::Timeout, "timeout:fuel"));
    let wall = ExecConfig {
        limits: Limits { fuel: u64::MAX, wall: Some(std::time::Duration::from_millis(10)) },
        timeouts_are_failures: true,
        ..ExecConfig::default()
    };
    assert_eq!(check(&p, &wall).failure.unwrap().signature, "timeout:wall-clock");
}

#[test]
fn minimizer_preserves_signature_and_respects_budget() {
    let exec = exec_with(PlantedBug::OptSubAssoc);
    let src = "let a = 5; let b = 7; print 1; if a < b { print a - (b - a); } print 2;";
    let p = parse(src).unwrap();
    let sig = check(&p, &exec).failure.unwrap().signature;
    let (m, st) = minimize(&p, &sig, &exec, 10_000);
    assert_eq!(check(&m, &exec).failure.unwrap().signature, sig);
    assert!(program_size(&m) < program_size(&p));
    assert!(!st.budget_exhausted);
    let (_, st) = minimize(&p, &sig, &exec, 3);
    assert!(st.checks <= 3 && st.budget_exhausted);
}

#[test]
fn every_mutation_yields_a_printable_parseable_program() {
    use astfuzz::fuzz::mutate::mutate;
    let cfg = GenConfig::default();
    for i in 0..300 {
        let mut rng = Rng::fork(77, i);
        let mut p = generate(&mut rng, &cfg);
        let donor = generate(&mut rng, &cfg);
        for _ in 0..5 {
            mutate(&mut p, &donor, &mut rng, &cfg);
            assert_eq!(parse(&print_program(&p)).unwrap(), p);
        }
    }
}
