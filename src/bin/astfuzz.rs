//! `astfuzz` command-line interface.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use astfuzz::fuzz::corpus::{self, dedup_by_behavior};
use astfuzz::fuzz::engine::{self, FuzzConfig};
use astfuzz::fuzz::exec::{ExecConfig, check};
use astfuzz::fuzz::generator::{GenConfig, generate};
use astfuzz::fuzz::minimize::minimize;
use astfuzz::fuzz::rng::Rng;
use astfuzz::lang::ast::program_size;
use astfuzz::lang::interp::Limits;
use astfuzz::lang::parser::parse;
use astfuzz::lang::printer::print_program;
use astfuzz::planted::{Bugs, PlantedBug};
use clap::{Args, Parser, Subcommand};

/// Structure-aware AST fuzzer for the Mini language.
#[derive(Parser)]
#[command(name = "astfuzz", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a fuzzing campaign (exit 0: no findings, 1: findings, 2: error).
    Run(RunArgs),
    /// Run one program through every oracle (exit 0: pass, 1: failure).
    Replay(ReplayArgs),
    /// Reduce a failing program while it keeps failing the same way.
    Minimize(MinimizeArgs),
    /// Inspect or deduplicate a corpus directory (never modifies it).
    Corpus {
        #[command(subcommand)]
        cmd: CorpusCmd,
    },
    /// Print generated programs (shows that a seed fully determines output).
    Gen {
        /// Seed.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// How many programs.
        #[arg(long, default_value_t = 1)]
        count: u64,
    },
}

#[derive(Args)]
struct Common {
    /// Enable a deliberately planted bug (repeatable): opt-sub-assoc,
    /// print-sub-parens, typeck-eq-mixed, interp-rem-panic.
    #[arg(long = "plant", value_name = "BUG")]
    plant: Vec<PlantedBug>,
    /// Interpreter step budget per run.
    #[arg(long, default_value_t = 20_000)]
    fuel: u64,
    /// Report budget exhaustion as a finding.
    #[arg(long)]
    timeouts_are_failures: bool,
}

impl Common {
    fn exec(&self) -> ExecConfig {
        ExecConfig {
            limits: Limits { fuel: self.fuel, ..Limits::default() },
            bugs: self.plant.iter().copied().collect::<Bugs>(),
            timeouts_are_failures: self.timeouts_are_failures,
        }
    }
}

#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    common: Common,
    /// Master seed.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Programs to execute.
    #[arg(long, default_value_t = 10_000)]
    iterations: u64,
    /// Seed corpus directory (`*.mini`); new coverage is saved back only with --save-corpus.
    #[arg(long)]
    corpus: Option<PathBuf>,
    /// Write programs that added coverage into --corpus.
    #[arg(long, requires = "corpus")]
    save_corpus: bool,
    /// Directory for report.json and reproducer files.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Skip minimization of findings.
    #[arg(long)]
    no_minimize: bool,
    /// Oracle-run budget per minimization.
    #[arg(long, default_value_t = 5_000)]
    minimize_budget: usize,
    /// Stop after the first finding.
    #[arg(long)]
    stop_on_first: bool,
    /// Maximum expression depth for generation.
    #[arg(long, default_value_t = 5)]
    max_depth: u32,
    /// Maximum statements per block for generation.
    #[arg(long, default_value_t = 6)]
    max_stmts: u32,
    /// Print the JSON report to stdout instead of a summary.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct ReplayArgs {
    #[command(flatten)]
    common: Common,
    /// Program file.
    file: PathBuf,
}

#[derive(Args)]
struct MinimizeArgs {
    #[command(flatten)]
    common: Common,
    /// Failing program file.
    file: PathBuf,
    /// Oracle-run budget.
    #[arg(long, default_value_t = 5_000)]
    budget: usize,
    /// Write the minimized program here (default: stdout only).
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum CorpusCmd {
    /// Count entries, rejects and canonical duplicates.
    Stats {
        /// Corpus directory.
        dir: PathBuf,
    },
    /// Write a deduplicated copy (canonical + behavioural) into OUT.
    Dedup {
        /// Corpus directory.
        dir: PathBuf,
        /// Output directory (must differ from DIR).
        #[arg(long)]
        out: PathBuf,
        #[command(flatten)]
        common: Common,
    },
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn real_main() -> Result<ExitCode> {
    match Cli::parse().cmd {
        Cmd::Run(a) => cmd_run(&a),
        Cmd::Replay(a) => cmd_replay(&a),
        Cmd::Minimize(a) => cmd_minimize(&a),
        Cmd::Corpus { cmd } => cmd_corpus(cmd),
        Cmd::Gen { seed, count } => {
            let cfg = GenConfig::default();
            for i in 0..count {
                println!("# seed {seed} program {i}");
                print!("{}", print_program(&generate(&mut Rng::fork(seed, i), &cfg)));
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn read_program(path: &Path) -> Result<astfuzz::lang::ast::Program> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text).with_context(|| format!("parsing {}", path.display()))
}

fn cmd_run(a: &RunArgs) -> Result<ExitCode> {
    let exec = a.common.exec();
    let cfg = FuzzConfig {
        seed: a.seed,
        iterations: a.iterations,
        minimize: !a.no_minimize,
        minimize_budget: a.minimize_budget,
        stop_on_first: a.stop_on_first,
        gen_config: GenConfig { max_expr_depth: a.max_depth, max_stmts: a.max_stmts, ..GenConfig::default() },
        ..FuzzConfig::default()
    };
    let seeds = match &a.corpus {
        Some(dir) if dir.exists() => {
            let loaded = corpus::load(dir).with_context(|| format!("loading corpus {}", dir.display()))?;
            for (p, why) in &loaded.rejected {
                eprintln!("warning: skipping {}: {why}", p.display());
            }
            loaded.entries.into_iter().map(|e| e.program).collect()
        }
        _ => Vec::new(),
    };
    let report = engine::run(&cfg, &exec, &seeds);

    if let (Some(dir), true) = (&a.corpus, a.save_corpus) {
        for p in &report.corpus {
            corpus::save(dir, p)?;
        }
    }
    if let Some(out) = &a.out {
        write_outputs(out, &report)?;
    }
    if a.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_summary(&report);
    }
    Ok(if report.findings.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn write_outputs(out: &Path, report: &engine::Report) -> Result<()> {
    fs::create_dir_all(out)?;
    fs::write(out.join("report.json"), serde_json::to_string_pretty(report)?)?;
    for (i, f) in report.findings.iter().enumerate() {
        fs::write(out.join(format!("finding-{i}.mini")), &f.reproducer)?;
        if let Some(m) = &f.minimized {
            fs::write(out.join(format!("finding-{i}.min.mini")), m)?;
        }
    }
    Ok(())
}

fn print_summary(r: &engine::Report) {
    let s = &r.stats;
    println!(
        "seed {} | {} executions ({} generated, {} mutated, {} oversize skipped) | corpus {} | features {} | timeouts {} | {} ms",
        r.config.seed,
        s.executions,
        s.generated,
        s.mutated,
        s.discarded_oversize,
        s.corpus_size,
        s.features,
        s.timeouts,
        s.elapsed_ms
    );
    if !r.planted.is_empty() {
        println!("planted bugs enabled: {}", r.planted.join(", "));
    }
    println!("{} unique finding(s)", r.findings.len());
    for (i, f) in r.findings.iter().enumerate() {
        println!("\n[{i}] {} (iteration {}, +{} duplicates)", f.failure.signature, f.iteration, f.duplicates);
        println!("    {}", f.failure.detail.lines().next().unwrap_or_default());
        if let (Some(m), Some(st)) = (&f.minimized, &f.minimize_stats) {
            println!(
                "    minimized {} -> {} nodes in {} checks:",
                st.original_size, st.minimized_size, st.checks
            );
            for line in m.lines() {
                println!("      {line}");
            }
        }
    }
}

fn cmd_replay(a: &ReplayArgs) -> Result<ExitCode> {
    let p = read_program(&a.file)?;
    let v = check(&p, &a.common.exec());
    if let Some(o) = &v.outcome {
        println!("output: {:?}", o.output);
        println!("result: {:?} after {} steps", o.result, o.steps);
    }
    match v.failure {
        Some(f) => {
            println!("FAIL {}\n{}", f.signature, f.detail);
            Ok(ExitCode::from(1))
        }
        None => {
            println!("PASS (all oracles agree{})", if v.timed_out { "; run hit its budget" } else { "" });
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn cmd_minimize(a: &MinimizeArgs) -> Result<ExitCode> {
    let p = read_program(&a.file)?;
    let exec = a.common.exec();
    let Some(f) = check(&p, &exec).failure else {
        bail!("{} does not fail any oracle with these settings; nothing to minimize", a.file.display());
    };
    let (m, st) = minimize(&p, &f.signature, &exec, a.budget);
    let text = print_program(&m);
    eprintln!(
        "{}: {} -> {} nodes in {} checks{}",
        f.signature,
        st.original_size,
        program_size(&m),
        st.checks,
        if st.budget_exhausted { " (budget exhausted)" } else { "" }
    );
    print!("{text}");
    if let Some(out) = &a.out {
        fs::write(out, &text)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_corpus(cmd: CorpusCmd) -> Result<ExitCode> {
    match cmd {
        CorpusCmd::Stats { dir } => {
            let l = corpus::load(&dir)?;
            println!(
                "{}: {} unique, {} canonical duplicates, {} rejected",
                dir.display(),
                l.entries.len(),
                l.duplicates.len(),
                l.rejected.len()
            );
            for (p, why) in &l.rejected {
                println!("  rejected {}: {why}", p.display());
            }
        }
        CorpusCmd::Dedup { dir, out, common } => {
            if dir.canonicalize().ok() == out.canonicalize().ok() {
                bail!("--out must be a different directory from the input corpus");
            }
            let l = corpus::load(&dir)?;
            let (keep, dropped) = dedup_by_behavior(&l.entries, &common.exec());
            for e in &keep {
                corpus::save(&out, &e.program)?;
            }
            println!(
                "kept {} of {} ({} canonical duplicates, {} behavioural duplicates, {} rejected) -> {}",
                keep.len(),
                l.entries.len() + l.duplicates.len(),
                l.duplicates.len(),
                dropped.len(),
                l.rejected.len(),
                out.display()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}
