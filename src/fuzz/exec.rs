//! Running one program through every oracle, with crash capture.
//!
//! Oracles, in order:
//! 1. `roundtrip-parse` / `roundtrip-ast`: `parse(print(p))` succeeds and equals `p`.
//! 2. reference run: a panic anywhere is a **crash**; running out of fuel or
//!    wall-clock time is a **timeout**.
//! 3. `type-soundness`: a program the type checker accepts must not fail with
//!    a runtime type or unbound-variable error.
//! 4. `optimizer-equivalence`: the optimized program prints the same values
//!    and ends with the same error (or success) as the original.

use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

use serde::Serialize;

use crate::coverage::Coverage;
use crate::lang::ast::Program;
use crate::lang::interp::{Limits, Outcome, RuntimeError, run_with};
use crate::lang::opt::optimize_with;
use crate::lang::parser::parse;
use crate::lang::printer::print_program_with;
use crate::lang::typeck::check_with;
use crate::planted::Bugs;

/// Coarse failure classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureClass {
    /// A panic inside the system under test.
    Crash,
    /// The run exceeded its fuel or wall-clock budget.
    Timeout,
    /// An oracle (round trip, type soundness, optimizer equivalence) disagreed.
    InvariantViolation,
}

/// A detected failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// Class of failure.
    pub class: FailureClass,
    /// Stable identity used for deduplication and minimization, e.g.
    /// `invariant-violation:optimizer-equivalence` or
    /// `crash:interp:src/lang/interp.rs:123`.
    pub signature: String,
    /// Human-readable details (panic message, the two diverging outcomes, ...).
    pub detail: String,
}

/// Configuration of one execution.
#[derive(Debug, Clone, Copy)]
pub struct ExecConfig {
    /// Interpreter limits.
    pub limits: Limits,
    /// Planted bugs to enable (empty for the correct implementation).
    pub bugs: Bugs,
    /// Report budget exhaustion as a failure (off by default: generated
    /// programs may legitimately loop forever).
    pub timeouts_are_failures: bool,
}

impl Default for ExecConfig {
    fn default() -> Self {
        ExecConfig { limits: Limits::default(), bugs: Bugs::NONE, timeouts_are_failures: false }
    }
}

/// Result of checking one program.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// The failure, if any oracle fired.
    pub failure: Option<Failure>,
    /// Coverage of the reference run (empty if it crashed).
    pub coverage: Coverage,
    /// Reference outcome (absent if it crashed).
    pub outcome: Option<Outcome>,
    /// Did the reference run hit its budget?
    pub timed_out: bool,
}

thread_local! {
    static CAPTURING: RefCell<bool> = const { RefCell::new(false) };
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}
static HOOK: Once = Once::new();

/// Install (once) a panic hook that, on threads currently checking a
/// program, records the panic message and location silently instead of
/// printing them. Other panics go to the previous hook unchanged.
fn install_hook() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if CAPTURING.with(|c| *c.borrow()) {
                let msg = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "non-string panic payload".into());
                let loc = info
                    .location()
                    .map(|l| format!("{}:{}", l.file(), l.line()))
                    .unwrap_or_else(|| "unknown".into());
                LAST_PANIC.with(|p| *p.borrow_mut() = Some((msg, loc)));
            } else {
                previous(info);
            }
        }));
    });
}

/// Run `f`, converting a panic into a `crash` failure tagged with `stage`.
fn guarded<T>(stage: &str, f: impl FnOnce() -> T) -> Result<T, Failure> {
    install_hook();
    CAPTURING.with(|c| *c.borrow_mut() = true);
    LAST_PANIC.with(|p| *p.borrow_mut() = None);
    let r = panic::catch_unwind(AssertUnwindSafe(f));
    CAPTURING.with(|c| *c.borrow_mut() = false);
    r.map_err(|_| {
        let (msg, loc) = LAST_PANIC
            .with(|p| p.borrow_mut().take())
            .unwrap_or_else(|| ("unknown panic".into(), "unknown".into()));
        Failure {
            class: FailureClass::Crash,
            signature: format!("crash:{stage}:{loc}:{}", normalize(&msg)),
            detail: format!("panic in {stage} at {loc}: {msg}"),
        }
    })
}

/// Strip digits so `index out of bounds: the len is 8 but the index is 1844..`
/// and the same panic with another index share one signature.
fn normalize(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut last_digit = false;
    for ch in msg.chars().take(120) {
        if ch.is_ascii_digit() {
            if !last_digit {
                out.push('N');
            }
            last_digit = true;
        } else {
            out.push(ch);
            last_digit = false;
        }
    }
    out
}

fn invariant(oracle: &str, detail: String) -> Failure {
    Failure {
        class: FailureClass::InvariantViolation,
        signature: format!("invariant-violation:{oracle}"),
        detail,
    }
}

fn fail(failure: Failure, coverage: Coverage, outcome: Option<Outcome>, timed_out: bool) -> Verdict {
    Verdict { failure: Some(failure), coverage, outcome, timed_out }
}

fn describe(o: &Outcome) -> String {
    format!("output={:?} result={:?}", o.output, o.result)
}

/// Check one program against every oracle.
pub fn check(p: &Program, cfg: &ExecConfig) -> Verdict {
    let bugs = cfg.bugs;
    let empty = Coverage::default();

    // 1. Round trip through the printer and parser.
    match guarded("printer+parser", || {
        let text = print_program_with(p, bugs);
        (parse(&text), text)
    }) {
        Err(f) => return fail(f, empty, None, false),
        Ok((Err(e), text)) => {
            let d = format!("printed text does not parse: {e}\n--- printed ---\n{text}");
            return fail(invariant("roundtrip-parse", d), empty, None, false);
        }
        Ok((Ok(back), text)) if back != *p => {
            let d = format!("parse(print(p)) differs from p\n--- printed ---\n{text}");
            return fail(invariant("roundtrip-ast", d), empty, None, false);
        }
        Ok(_) => {}
    }

    // 2. Reference execution.
    let (outcome, coverage) = match guarded("interp", || run_with(p, cfg.limits, bugs)) {
        Ok(r) => r,
        Err(f) => return fail(f, empty, None, false),
    };
    let timed_out = matches!(outcome.result, Err(e) if e.is_timeout());
    if timed_out {
        if cfg.timeouts_are_failures {
            let kind = if outcome.result == Err(RuntimeError::WallTimeout) { "wall-clock" } else { "fuel" };
            let f = Failure {
                class: FailureClass::Timeout,
                signature: format!("timeout:{kind}"),
                detail: format!("budget exhausted after {} steps", outcome.steps),
            };
            return fail(f, coverage, Some(outcome), true);
        }
        // A partial run says nothing reliable about the other oracles.
        return Verdict { failure: None, coverage, outcome: Some(outcome), timed_out };
    }

    // 3. Type soundness.
    let typed = match guarded("typeck", || check_with(p, bugs)) {
        Ok(r) => r.is_ok(),
        Err(f) => return fail(f, coverage, Some(outcome), false),
    };
    if typed && matches!(outcome.result, Err(RuntimeError::Type | RuntimeError::Unbound)) {
        let d =
            format!("type checker accepted the program but it failed at run time: {}", describe(&outcome));
        return fail(invariant("type-soundness", d), coverage, Some(outcome), false);
    }

    // 4. Optimizer equivalence.
    let optimized = match guarded("optimizer", || optimize_with(p, bugs)) {
        Ok(o) => o,
        Err(f) => return fail(f, coverage, Some(outcome), false),
    };
    let (opt_outcome, _) = match guarded("interp", || run_with(&optimized, cfg.limits, bugs)) {
        Ok(r) => r,
        Err(f) => return fail(f, coverage, Some(outcome), false),
    };
    let opt_timed_out = matches!(opt_outcome.result, Err(e) if e.is_timeout());
    if !opt_timed_out && (opt_outcome.output != outcome.output || opt_outcome.result != outcome.result) {
        let d = format!("original:  {}\noptimized: {}", describe(&outcome), describe(&opt_outcome));
        return fail(invariant("optimizer-equivalence", d), coverage, Some(outcome), false);
    }

    Verdict { failure: None, coverage, outcome: Some(outcome), timed_out: false }
}
