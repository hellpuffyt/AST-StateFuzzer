//! # AST-StateFuzzer
//!
//! A structure-aware fuzzer that generates and mutates **typed syntax trees**
//! instead of bytes, runs them through differential **oracles**, classifies
//! crashes, timeouts and invariant violations, and reduces every finding to a
//! small reproducer with grammar-aware minimization.
//!
//! The initial target is *Mini*, a small language defined in [`lang`] with a
//! parser, printer, type checker, interpreter and optimizer. Four deliberately
//! planted bugs ([`planted`]) prove end to end that the fuzzer finds and
//! minimizes real defect classes from a fixed seed.
//!
//! ```
//! use astfuzz::fuzz::engine::{FuzzConfig, run};
//! use astfuzz::fuzz::exec::ExecConfig;
//! use astfuzz::planted::{Bugs, PlantedBug};
//!
//! let cfg = FuzzConfig { seed: 1, iterations: 3_000, stop_on_first: true, ..FuzzConfig::default() };
//! let exec = ExecConfig { bugs: Bugs::NONE.with(PlantedBug::OptSubAssoc), ..ExecConfig::default() };
//! let report = run(&cfg, &exec, &[]);
//! let f = &report.findings[0];
//! assert_eq!(f.failure.signature, "invariant-violation:optimizer-equivalence");
//! println!("{}", f.minimized.as_deref().unwrap_or_default());
//! ```

pub mod coverage;
pub mod fuzz;
pub mod lang;
pub mod planted;
