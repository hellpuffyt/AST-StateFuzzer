# Known limitations

## Scope
- **One target language.** Only Mini is supported. The engine's pieces (generator, mutators, oracles, minimizer)
  are written against Mini's AST; there is no plug-in interface for other grammars yet.
- **Mini is small by design**: no functions, arrays, strings or I/O beyond `print`. Bugs that need those
  constructs cannot exist here, so they cannot be found here.
- **Planted bugs are the evidence.** The acceptance tests prove the fuzzer finds four deliberately introduced bug
  classes. No unplanted bug has been found in the shipped Mini implementation (50 000-iteration campaigns over
  several seeds report zero findings), which is evidence for the implementation, not a proof.

## Coverage
- Feedback is **execution-feature coverage, not CFG coverage** (see `docs/COVERAGE.md`). The feature space is
  small (a few hundred features), so coverage plateaus quickly.
- Test-suite line/region coverage is measured (96.9 % lines, `docs/COVERAGE.md`); campaign CFG coverage is not.

## Determinism
- Runs are deterministic for a given `(seed, config, seed corpus)` **except** for wall-clock timeouts (default 2 s
  per run, rarely reached because fuel ends runs first) and `stats.elapsed_ms`.
- Changing the generator, mutators or RNG changes what a seed produces; `rng_stream_is_pinned` only guards the RNG.

## Classification
- Four outcomes are distinguished: **crash** (a panic in any stage, signature = stage + location + normalized
  message), **timeout** (fuel or wall clock, split into `timeout:fuel` / `timeout:wall-clock`; findings only with
  `--timeouts-are-failures`), **invariant violation** (named by oracle), and **expected program errors**
  (`Overflow`, `DivByZero`, and `Type`/`Unbound` in programs the type checker rejects), which are correct behaviour
  and never findings. Tested in `tests/classification.rs`.
- **Hangs outside the interpreter are not detected.** Fuel and the wall-clock check bound only interpreter runs.
  The printer, parser, type checker and optimizer are structurally recursive over a finite AST and terminate for
  Mini, so this is not reachable today; a target with non-terminating passes needs an out-of-process watchdog.
- **A timeout of the optimized program is not a finding.** If the original run finishes but the optimized one
  runs out of budget, the optimizer-equivalence oracle skips the comparison. Mini's optimizer only removes
  nodes, so it cannot need more fuel than the original; an optimizer bug that introduces non-termination would
  still be missed.

## Corpus
- Writes are atomic (temp file + rename, fsync) and an existing file under a canonical name is rewritten unless it
  holds exactly the canonical text. Unreadable, non-UTF-8, truncated and oversized files are rejected one by one.
- At most 10 000 `*.mini` files are loaded (`corpus::MAX_CORPUS_FILES`, the first in name order) and
  `--save-corpus` stops adding files at that count, with a warning. The in-memory corpus of a campaign is bounded
  by the finite feature space (a program joins only if it adds a feature). The tool never deletes corpus files;
  compact with `astfuzz corpus dedup`.

## Minimization
- Passes: statement ddmin, statement unwrapping, expression hoisting, literal shrinking, variable → literal. It does
  not rename variables, merge statements or inline `let`s, so results are small but not always globally minimal.
- The minimizer re-runs every oracle per candidate; for large programs it can take thousands of checks (budget
  default 5 000).

## Execution
- Programs run **in process**. That is safe for Mini (no side effects) but a stack overflow or abort in a target
  would kill the fuzzer; depth limits make this unreachable for Mini. Adapting the engine to a target that runs
  real code requires out-of-process isolation (see `docs/ARCHITECTURE.md`).
- Single-threaded; no parallel workers, no persistent campaign state or resume.

## Tooling and platforms
- The libFuzzer harness needs nightly Rust and `cargo-fuzz`; it is not built by the portable CI jobs.
- Verified locally on macOS (Apple M5). Linux and Windows are covered only by CI, which has not run yet at the
  time of writing.

# Roadmap

1. A `Target` trait (generate / print / parse / run / oracles) so a second language can be added without touching
   the engine. Second target candidate: a JSON-like data language or a small stack VM.
2. Persistent campaigns: save/restore RNG position, corpus and findings.
3. Parallel workers with per-worker streams (`Rng::fork` already makes this deterministic).
4. In-process edge-coverage feedback and `llvm-cov` reports in CI.
5. Smarter reduction: variable renaming, `let` inlining, type-directed literal choice.
