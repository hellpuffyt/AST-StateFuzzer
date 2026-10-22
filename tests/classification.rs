#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Failure classification (crash / timeout / invariant violation / expected
//! program errors), minimizer signature preservation across every planted
//! bug, and campaign reproducibility.

use astfuzz::fuzz::engine::{FuzzConfig, Report, run};
use astfuzz::fuzz::exec::{ExecConfig, FailureClass, check};
use astfuzz::fuzz::minimize::minimize;
use astfuzz::lang::ast::program_size;
use astfuzz::lang::interp::{Limits, RuntimeError};
use astfuzz::lang::parser::parse;
use astfuzz::planted::{Bugs, PlantedBug};

fn with(bugs: &[PlantedBug]) -> ExecConfig {
    ExecConfig { bugs: bugs.iter().copied().collect(), ..ExecConfig::default() }
}

/// What `check` concluded, in one comparable value.
fn verdict(src: &str, exec: &ExecConfig) -> (Option<FailureClass>, Option<String>, bool) {
    let v = check(&parse(src).unwrap(), exec);
    (v.failure.as_ref().map(|f| f.class), v.failure.map(|f| f.signature), v.timed_out)
}

#[test]
fn expected_program_errors_are_not_findings() {
    // Runtime errors the language defines are correct behaviour, not bugs.
    for (src, err) in [
        ("print 1 / 0;", RuntimeError::DivByZero),
        ("print 5 % 0;", RuntimeError::DivByZero),
        ("print 9223372036854775807 + 1;", RuntimeError::Overflow),
        // Rejected by the type checker, so a dynamic type error is expected.
        ("print 1 + true;", RuntimeError::Type),
        ("print nope;", RuntimeError::Unbound),
    ] {
        let v = check(&parse(src).unwrap(), &ExecConfig::default());
        assert!(v.failure.is_none(), "{src}: {:?}", v.failure);
        assert!(!v.timed_out, "{src}");
        assert_eq!(v.outcome.unwrap().result, Err(err), "{src}");
    }
}

/// Program, configuration, expected class, expected signature prefix, timed out.
type Case = (&'static str, ExecConfig, Option<FailureClass>, Option<&'static str>, bool);

#[test]
fn each_failure_class_is_distinguished() {
    let loud = ExecConfig { timeouts_are_failures: true, ..ExecConfig::default() };
    let wall = ExecConfig {
        limits: Limits { fuel: u64::MAX, wall: Some(std::time::Duration::from_millis(20)) },
        timeouts_are_failures: true,
        ..ExecConfig::default()
    };
    let cases: [Case; 8] = [
        ("print 1 + 2;", ExecConfig::default(), None, None, false),
        // A non-terminating program: a timeout, not a finding by default...
        ("while true { }", ExecConfig::default(), None, None, true),
        // ...a timeout finding when asked, split by budget kind.
        ("while true { }", loud, Some(FailureClass::Timeout), Some("timeout:fuel"), true),
        ("while true { }", wall, Some(FailureClass::Timeout), Some("timeout:wall-clock"), true),
        // A panic in the system under test is a crash.
        (
            "print -7 % 3;",
            with(&[PlantedBug::InterpRemPanic]),
            Some(FailureClass::Crash),
            Some("crash:interp:"),
            false,
        ),
        // Wrong answers are invariant violations, named by oracle.
        (
            "let a = 1; print 0 - (0 - a);",
            with(&[PlantedBug::OptSubAssoc]),
            Some(FailureClass::InvariantViolation),
            Some("invariant-violation:optimizer-equivalence"),
            false,
        ),
        (
            "print 1 - (2 - 3);",
            with(&[PlantedBug::PrintSubParens]),
            Some(FailureClass::InvariantViolation),
            Some("invariant-violation:roundtrip-ast"),
            false,
        ),
        (
            "print 1 == true;",
            with(&[PlantedBug::TypeckEqMixed]),
            Some(FailureClass::InvariantViolation),
            Some("invariant-violation:type-soundness"),
            false,
        ),
    ];
    for (src, exec, class, sig, timed_out) in cases {
        let (c, s, t) = verdict(src, &exec);
        assert_eq!(c, class, "{src}");
        match sig {
            Some(prefix) => assert!(s.as_deref().unwrap().starts_with(prefix), "{src}: {s:?}"),
            None => assert!(s.is_none(), "{src}: {s:?}"),
        }
        assert_eq!(t, timed_out, "{src}");
    }
}

#[test]
fn a_timeout_is_never_reported_as_a_wrong_answer() {
    // The type-soundness bug would fire after the loop, but the loop never
    // ends: a partial run says nothing about later oracles.
    let src = "while true { } print 1 == true;";
    let exec = with(&[PlantedBug::TypeckEqMixed]);
    assert_eq!(verdict(src, &exec), (None, None, true));
    let loud = ExecConfig { timeouts_are_failures: true, ..exec };
    assert_eq!(verdict(src, &loud).0, Some(FailureClass::Timeout));
}

#[test]
fn minimized_findings_keep_their_exact_signature_for_every_planted_bug() {
    // Each bug alone, then all four at once: the minimizer must never trade
    // one failure for another (a smaller program failing differently is
    // rejected). With all four planted, `print-sub-parens` fires first on
    // every `a - (b - c)`, so the optimizer bug is only reachable alone.
    let mut configs: Vec<ExecConfig> = PlantedBug::ALL.iter().map(|b| with(&[*b])).collect();
    configs.push(with(&PlantedBug::ALL));
    let mut seen = std::collections::BTreeSet::new();
    let mut checked = 0;
    for exec in &configs {
        for seed in 1..=3 {
            let cfg = FuzzConfig { seed, iterations: 5_000, minimize_budget: 3_000, ..FuzzConfig::default() };
            let r = run(&cfg, exec, &[]);
            for f in &r.findings {
                let original = parse(&f.reproducer).unwrap();
                assert_eq!(check(&original, exec).failure.unwrap().signature, f.failure.signature);
                let min = parse(f.minimized.as_ref().unwrap()).unwrap();
                let replayed = check(&min, exec).failure.expect("minimized program must still fail");
                assert_eq!(replayed.signature, f.failure.signature, "seed {seed}");
                assert_eq!(replayed.class, f.failure.class);
                assert!(program_size(&min) <= program_size(&original));
                seen.insert(f.failure.signature.split(':').take(2).collect::<Vec<_>>().join(":"));
                checked += 1;
            }
        }
    }
    for want in [
        "invariant-violation:optimizer-equivalence",
        "invariant-violation:roundtrip-ast",
        "invariant-violation:type-soundness",
        "crash:interp",
    ] {
        assert!(seen.contains(want), "{want} not exercised; saw {seen:?}");
    }
    assert!(checked >= 10, "only {checked} findings checked");
}

#[test]
fn minimizing_with_a_different_signature_changes_nothing() {
    // Asking for a signature the program does not have keeps it unchanged.
    let exec = with(&[PlantedBug::OptSubAssoc]);
    let p = parse("let a = 5; let b = 7; print a - (b - a); print 1;").unwrap();
    let (m, st) = minimize(&p, "crash:interp:somewhere", &exec, 1_000);
    assert_eq!(m, p);
    assert_eq!(st.minimized_size, st.original_size);
}

fn strip(r: &Report) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    v["stats"]["elapsed_ms"] = serde_json::Value::Null;
    v
}

#[test]
fn a_campaign_is_a_prefix_of_any_longer_campaign() {
    // Iteration i depends only on (seed, i) and earlier iterations, so
    // running fewer iterations gives exactly the earlier findings.
    let exec = with(&PlantedBug::ALL);
    let long = run(&FuzzConfig { seed: 3, iterations: 4_000, ..FuzzConfig::default() }, &exec, &[]);
    let short = run(&FuzzConfig { seed: 3, iterations: 1_500, ..FuzzConfig::default() }, &exec, &[]);
    let early: Vec<_> = long.findings.iter().filter(|f| f.iteration < 1_500).collect();
    assert_eq!(short.findings.len(), early.len());
    for (a, b) in short.findings.iter().zip(early) {
        assert_eq!(
            (a.iteration, &a.failure, &a.reproducer, &a.minimized),
            (b.iteration, &b.failure, &b.reproducer, &b.minimized)
        );
    }
    // And stop-on-first returns the long run's first finding.
    let first = run(
        &FuzzConfig { seed: 3, iterations: 4_000, stop_on_first: true, ..FuzzConfig::default() },
        &exec,
        &[],
    );
    assert_eq!(first.findings.len(), 1);
    assert_eq!(first.findings[0].reproducer, long.findings[0].reproducer);
}

#[test]
fn config_is_part_of_the_reproduction_key() {
    let exec = ExecConfig::default();
    let base = FuzzConfig { seed: 8, iterations: 400, ..FuzzConfig::default() };
    let a = run(&base, &exec, &[]);
    assert_eq!(strip(&a), strip(&run(&base, &exec, &[])));
    // The report records the full configuration, and changing it changes the run.
    let v = strip(&a);
    assert_eq!(v["config"]["seed"], 8);
    assert_eq!(v["config"]["gen_config"]["max_stmts"], 6);
    let mut other = base.clone();
    other.gen_config.max_stmts = 3;
    assert_ne!(strip(&run(&other, &exec, &[]))["stats"], v["stats"]);
}

#[test]
fn corpus_is_bounded_by_the_feature_space() {
    // A program joins the corpus only if it adds a feature, so the in-memory
    // corpus can never outgrow the (finite) feature set.
    for seed in [1, 2] {
        let r = run(
            &FuzzConfig { seed, iterations: 20_000, ..FuzzConfig::default() },
            &ExecConfig::default(),
            &[],
        );
        assert!(r.stats.corpus_size <= r.stats.features, "{} > {}", r.stats.corpus_size, r.stats.features);
    }
}

#[test]
fn bugs_collect_from_an_iterator() {
    let b: Bugs = PlantedBug::ALL.iter().copied().collect();
    assert_eq!(b.list().len(), 4);
}
