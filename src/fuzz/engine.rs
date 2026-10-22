//! The fuzzing loop.
//!
//! Iteration `i` draws all of its randomness from `Rng::fork(seed, i)`, and
//! corpus admission depends only on earlier iterations, so a run is a pure
//! function of `(seed, config, initial corpus)` — the wall-clock limit is the
//! only exception, and it is set far above the fuel budget's typical cost.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use serde::Serialize;

use super::corpus::canonical_hash;
use super::exec::{ExecConfig, Failure, FailureClass, check};
use super::generator::{GenConfig, generate};
use super::minimize::{MinimizeStats, minimize};
use super::mutate::mutate;
use super::rng::{Rng, Source};
use crate::coverage::Coverage;
use crate::lang::ast::{Program, program_depth, program_size};
use crate::lang::printer::print_program;

/// Fuzzing campaign settings.
#[derive(Debug, Clone, Serialize)]
pub struct FuzzConfig {
    /// Master seed.
    pub seed: u64,
    /// Number of programs to execute.
    pub iterations: u64,
    /// Percent of iterations that generate from scratch (others mutate).
    pub generate_percent: u32,
    /// Programs larger than this many nodes are discarded before execution.
    pub max_program_size: usize,
    /// Programs nested deeper than this are discarded before execution.
    pub max_program_depth: usize,
    /// Minimize each new unique finding.
    pub minimize: bool,
    /// Oracle-run budget per minimization.
    pub minimize_budget: usize,
    /// Stop at the first finding.
    pub stop_on_first: bool,
    /// Generator knobs.
    pub gen_config: GenConfig,
}

impl Default for FuzzConfig {
    fn default() -> Self {
        FuzzConfig {
            seed: 0,
            iterations: 10_000,
            generate_percent: 20,
            max_program_size: 400,
            max_program_depth: 40,
            minimize: true,
            minimize_budget: 5_000,
            stop_on_first: false,
            gen_config: GenConfig::default(),
        }
    }
}

/// One unique finding.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Master seed of the campaign.
    pub seed: u64,
    /// Iteration that produced it (replay with the same seed and config).
    pub iteration: u64,
    /// The failure.
    pub failure: Failure,
    /// Reproducer source as found.
    pub reproducer: String,
    /// Node count of the reproducer.
    pub reproducer_size: usize,
    /// Minimized reproducer (if minimization ran).
    pub minimized: Option<String>,
    /// Minimization statistics.
    pub minimize_stats: Option<MinimizeStats>,
    /// How many more times this same signature was hit later.
    pub duplicates: u64,
}

/// Campaign statistics.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Stats {
    /// Programs executed.
    pub executions: u64,
    /// Generated from scratch.
    pub generated: u64,
    /// Produced by mutation.
    pub mutated: u64,
    /// Discarded for exceeding size/depth limits.
    pub discarded_oversize: u64,
    /// Runs that exhausted their budget (not counted as findings by default).
    pub timeouts: u64,
    /// Runs that failed some oracle (including duplicates).
    pub failing_runs: u64,
    /// Corpus size at the end.
    pub corpus_size: usize,
    /// Distinct execution features observed.
    pub features: usize,
    /// Wall-clock time of the campaign (not deterministic).
    pub elapsed_ms: u128,
}

/// Everything a campaign produced.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Tool name.
    pub tool: &'static str,
    /// Tool version.
    pub version: &'static str,
    /// Configuration used.
    pub config: FuzzConfig,
    /// Interpreter fuel per run.
    pub fuel: u64,
    /// Whether budget exhaustion counted as a failure.
    pub timeouts_are_failures: bool,
    /// Planted bugs that were enabled.
    pub planted: Vec<String>,
    /// Unique findings in discovery order.
    pub findings: Vec<Finding>,
    /// Statistics.
    pub stats: Stats,
    /// Canonical source of the final corpus (only kept in memory; the CLI
    /// writes it to `--corpus` if asked).
    #[serde(skip)]
    pub corpus: Vec<Program>,
}

/// Run a campaign.
pub fn run(cfg: &FuzzConfig, exec: &ExecConfig, seeds: &[Program]) -> Report {
    let start = Instant::now();
    let mut stats = Stats::default();
    let mut corpus: Vec<Program> = Vec::new();
    let mut known: BTreeSet<u64> = BTreeSet::new();
    let mut total = Coverage::default();
    let mut findings: Vec<Finding> = Vec::new();
    let mut by_sig: BTreeMap<String, usize> = BTreeMap::new();

    // Seed corpus: every seed is executed once and kept if it adds coverage.
    for s in seeds {
        let v = check(s, exec);
        stats.executions += 1;
        if total.has_new(&v.coverage) && known.insert(canonical_hash(s)) {
            total.merge(&v.coverage);
            corpus.push(s.clone());
        }
    }

    for i in 0..cfg.iterations {
        let mut rng = Rng::fork(cfg.seed, i);
        let program = if corpus.is_empty() || rng.chance(cfg.generate_percent) {
            stats.generated += 1;
            generate(&mut rng, &cfg.gen_config)
        } else {
            stats.mutated += 1;
            let mut p = corpus[rng.index(corpus.len())].clone();
            let donor = corpus[rng.index(corpus.len())].clone();
            // Stack 1-3 mutations.
            for _ in 0..1 + rng.below(3) {
                mutate(&mut p, &donor, &mut rng, &cfg.gen_config);
            }
            p
        };
        if program_size(&program) > cfg.max_program_size || program_depth(&program) > cfg.max_program_depth {
            stats.discarded_oversize += 1;
            continue;
        }

        let v = check(&program, exec);
        stats.executions += 1;
        if v.timed_out {
            stats.timeouts += 1;
        }
        if let Some(f) = v.failure {
            stats.failing_runs += 1;
            if let Some(&k) = by_sig.get(&f.signature) {
                findings[k].duplicates += 1;
                continue;
            }
            let (minimized, minimize_stats) = if cfg.minimize {
                let (m, st) = minimize(&program, &f.signature, exec, cfg.minimize_budget);
                (Some(print_program(&m)), Some(st))
            } else {
                (None, None)
            };
            by_sig.insert(f.signature.clone(), findings.len());
            findings.push(Finding {
                seed: cfg.seed,
                iteration: i,
                reproducer: print_program(&program),
                reproducer_size: program_size(&program),
                failure: f,
                minimized,
                minimize_stats,
                duplicates: 0,
            });
            if cfg.stop_on_first {
                break;
            }
            continue;
        }
        // Timeouts are kept out of the corpus: mutating them mostly yields
        // more timeouts.
        if !v.timed_out && total.has_new(&v.coverage) && known.insert(canonical_hash(&program)) {
            total.merge(&v.coverage);
            corpus.push(program);
        }
    }

    stats.corpus_size = corpus.len();
    stats.features = total.len();
    stats.elapsed_ms = start.elapsed().as_millis();
    Report {
        tool: "astfuzz",
        version: env!("CARGO_PKG_VERSION"),
        config: cfg.clone(),
        fuel: exec.limits.fuel,
        timeouts_are_failures: exec.timeouts_are_failures,
        planted: exec.bugs.list().iter().map(|b| b.name().to_string()).collect(),
        findings,
        stats,
        corpus,
    }
}

/// Count findings of one class.
pub fn count_class(r: &Report, class: FailureClass) -> usize {
    r.findings.iter().filter(|f| f.failure.class == class).count()
}
