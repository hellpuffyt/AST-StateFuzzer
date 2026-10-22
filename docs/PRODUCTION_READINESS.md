# Production readiness record

**Classification: Partially Verified.** Everything below was run on one macOS machine. GitHub Actions (Linux,
Windows, MSRV job, fuzz-acceptance job, libFuzzer job) has **not run yet**, so Linux and Windows behaviour is
unverified, and no third party has reviewed or audited the code. This file records evidence; it does not claim
production readiness.

- Tested commit: `c51e704688fb6755622307db843ca2a7a4fc5daf` (2026-10-22). The commit that adds this file only
  changes documentation.
- Machine: Apple M5, 10 cores, 16 GB RAM, macOS 27. The machine was heavily loaded by unrelated builds during all
  runs (load average 8–17), so every timing below is indicative only.
- Toolchains: rustc 1.98.0 stable, 1.85.0 (MSRV check), nightly 2026-09-16 (libFuzzer), cargo-fuzz 0.13.2.

## Scope and architecture

A structure-aware fuzzer for the Mini language defined in this repository: typed AST generation and mutation,
four oracles, crash/timeout/invariant classification, grammar-aware minimization, corpus management, JSON reports,
CLI, and an experimental libFuzzer harness. See `docs/ARCHITECTURE.md`. Out of scope: other languages, parallel or
persistent campaigns, CFG coverage in the main engine.

## Supported platforms and prerequisites

Rust 1.85+ (verified: 1.85.0 and 1.98.0 on macOS aarch64). Linux x86_64 and Windows x86_64 are targeted by CI
but unverified. The libFuzzer harness additionally needs nightly Rust and `cargo-fuzz`. No services, network,
credentials or special hardware are needed.

## Clean-clone setup (executed)

```sh
git clone <repo> <scratch dir outside the working tree> && cd <scratch dir>   # at c51e704
cargo build --release                                                          # exit 0
./target/release/astfuzz run --seed 1 --iterations 20000 --plant opt-sub-assoc --stop-on-first   # exit 1 (finding), output identical to README except ms
./target/release/astfuzz replay --plant opt-sub-assoc fixtures/regressions/opt-sub-assoc.mini  # exit 1, FAIL ... optimizer-equivalence
cargo test --release                                                           # 39 passed, 0 failed, 0 ignored
git status --short                                                             # empty
```

## Commands executed and outcomes

| Command | Outcome |
|---|---|
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | exit 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked` | exit 0 |
| `cargo test --all-features --locked` (debug) | 39 passed, 0 failed, 0 ignored |
| `cargo test --release --all-features --locked` | 39 passed, 0 failed, 0 ignored |
| `cargo +1.85 build --locked --all-targets` and `cargo +1.85 test --locked` | exit 0; 39 passed |
| `cargo deny check` | exit 0 (advisories, bans, licenses, sources ok) |
| `cargo audit` (RustSec DB, 1348 advisories) | exit 0, 60 crate dependencies, no vulnerabilities |
| `astfuzz run --seed 2026 --iterations 50000 --corpus corpus` (CI acceptance command) | exit 0, 0 findings, corpus 161, 299 features, 236 timeouts (not findings), 2583 ms |
| same for `--plant {opt-sub-assoc,print-sub-parens,typeck-eq-mixed,interp-rem-panic} --seed 1 --iterations 20000 --stop-on-first` | exit 1 for each (found) |
| `astfuzz run --seed {3..10} --iterations 50000 --max-depth 7 --max-stmts 8` | 0 findings for every seed |
| `cargo +nightly fuzz build -O` | exit 0 |
| `cargo +nightly fuzz run -O oracles -- -max_total_time=60` | 376 586 runs, `cov: 1577 ft: 7381`, no crash, peak RSS 579 MB |
| `cargo +nightly fuzz run -O parse_roundtrip -- -max_total_time=60` | 2 892 867 runs, `cov: 598 ft: 3190`, no crash |
| `ASTFUZZ_PLANT=opt-sub-assoc cargo +nightly fuzz run -O oracles -- -max_total_time=120` | exit 1, crash artifact written, panic message `invariant-violation:optimizer-equivalence` |

## Test results by kind

- **Unit / language (15)**: precedence, non-chaining comparisons, actionable parse errors with offsets, hostile
  nesting (10 000-deep parens, operator chains, unary chains, blocks) rejected without stack overflow, 1 MiB input
  cap, semantics (checked overflow, `i64::MIN / -1`, division by zero, truncation, remainder sign), short-circuit,
  scoping, fuel and wall-clock bounds, type checker accept/reject, optimizer never folds errors away, 500 generated
  programs round-trip and type-check, deepest accepted program survives every pass.
- **Property (2, 256 cases each)**: arbitrary bytes drive the generator to a program that round-trips; the parser
  never panics on arbitrary text.
- **Fuzzer behaviour (9)**: pinned RNG stream (cross-checked against an independent Python implementation of
  xoshiro256**/SplitMix64), same seed → same programs and identical campaign reports, no false positives over
  three 8 000-iteration seeds, planted-bug acceptance, crash capture with location and normalized signature,
  timeout classification (fuel and wall clock), minimizer signature preservation and budget, every mutation
  yields a parseable program.
- **Regression fixtures (1 test, 6 fixtures)**: each planted bug's minimized reproducer plus two "must pass"
  guards (short-circuit over a division by zero; `i64::MIN` folding).
- **Corpus (4)**: canonical dedup, malformed and oversized files reported, idempotent canonical save, behavioural
  dedup, shipped seed corpus loads.
- **CLI end to end (7)**: exit codes (0/1/2), JSON report structure, reproducer files, replay, minimize refusing a
  passing program, actionable errors, `corpus dedup` refusing to write into its input, deterministic `gen`.
- **Doc test (1)**: the library example finds the optimizer bug.
- **Environment-gated**: the libFuzzer targets are not part of `cargo test`; they live in a separate CI workflow
  (`libfuzzer.yml`) and were run manually here (results above). Nothing is skipped or counted as passed silently.

## CI status

Not yet run. Workflows: `ci.yml` (lint, 3-OS test matrix, MSRV 1.85, fixed-seed fuzz acceptance, cargo-deny),
`libfuzzer.yml` (weekly / manual; nightly + cargo-fuzz), `release.yml` (tags; builds binaries, draft release).
Action references are pinned to commit SHAs; the pinned `dtolnay/rust-toolchain` commit was checked to declare a
`toolchain` input (default `stable`), so the `1.85` and `nightly` overrides are honoured.

## Performance methodology

Not a benchmark project; numbers are the campaign summaries printed by the CLI (`elapsed_ms` from
`Instant::now()`), single runs, release profile, on a heavily loaded machine. Example: 50 000 iterations of the
clean implementation take 1.7–7.6 s across seeds here; that spread is mostly machine load. Do not compare these
numbers across machines.

## Bugs found and fixed during development

| Issue (how found) | Fix | Regression test |
|---|---|---|
| (Self-review while writing the parser, before any test ran.) Parser counted only recursive descent toward the nesting limit; a 10 000-term `1 + 1 + ...` chain was accepted and would have produced a 10 000-deep tree for later recursive passes. | Each left-associative fold now counts against the nesting budget. | `hostile_nesting_is_rejected_not_stack_overflowed` |
| (Observed in campaigns.) Coverage feedback saturated at 57 features, so the corpus stopped growing early. | Added operator-nesting features. | `correct_implementation_has_no_findings` asserts the corpus grows (> 10 entries) |
| (Observed in minimized output.) Minimizer could not replace variables with literals (equal node count), leaving unused `let`s. | Cost now includes variable references. | `minimizer_preserves_signature_and_respects_budget`, planted acceptance size bounds |
| (Self-review.) Corpus loader read whole files before applying the 1 MiB limit. | Size checked from metadata first. | `canonical_duplicates_collapse_and_bad_files_are_reported` |

No unplanted bug in the Mini implementation was found by any campaign.

## Security review

- Untrusted programs run only in the in-process interpreter, which has no file, network, process or environment
  access. Inputs are capped (1 MiB, nesting 200); depth over 40 is discarded before execution; every run is
  fuel-bounded with a wall-clock backstop.
- File handling: the corpus loader reads only `*.mini` regular files in the given directory (non-recursive), checks
  size before reading, never deletes; `corpus dedup` refuses `--out` equal to the input; outputs are written only
  under paths the user passes.
- No network listeners, no secrets, no logging of sensitive data; the project has no configuration file and reads
  one optional environment variable (`ASTFUZZ_PLANT`, libFuzzer target only).
- Secret/privacy scan of all committed files: no tokens, keys, private paths or e-mail addresses.
- `cargo audit`: no known vulnerabilities; `cargo deny`: licenses MIT/Apache-2.0/BSD/ISC/Unicode-3.0 only.

## Known limitations and unresolved risks

See `docs/LIMITATIONS.md`. Most important: one target language; semantic (not CFG) coverage; wall-clock timeouts
are the only nondeterministic element; CI has never run; Linux/Windows unverified; no external review.

## What further verification needs

GitHub Actions runners (free for public repositories) for Linux/Windows/MSRV/libFuzzer jobs. No other external
service, permission or hardware.

## Acceptance checklist

| # | Item | Expected behaviour | Command / test | Result | Actual | Reference |
|---|---|---|---|---|---|---|
| 1 | Documented target language | Grammar, types and semantics specified and implemented | `docs/LANGUAGE.md`; `tests/lang.rs` | pass | 17 tests in `tests/lang.rs` (15 unit + 2 property) pass | `src/lang/` |
| 2 | AST generation | Valid, mostly well-typed programs under budgets | `generated_programs_round_trip_and_type_check_mostly` | pass | 500/500 round-trip and type-check with ill-typed share 0 | `src/fuzz/generator.rs` |
| 3 | AST mutation | Every mutant is a valid AST | `every_mutation_yields_a_printable_parseable_program` | pass | 1 500 mutations, all parse back | `src/fuzz/mutate.rs` |
| 4 | Configurable depth and budgets | `--max-depth`, `--max-stmts`, `--fuel`, `--iterations`, `--minimize-budget` | CLI runs above | pass | budgets applied; oversize programs counted | `FuzzConfig`, `GenConfig`, `Limits` |
| 5 | Seeded reproducibility | Same seed → same programs and report | `same_seed_same_programs`, `same_seed_same_campaign`, `rng_stream_is_pinned`, `gen_is_deterministic` | pass | identical JSON (minus `elapsed_ms`) | `src/fuzz/rng.rs` |
| 6 | Seed corpus management | Load, save canonically, dedup | `tests/corpus.rs` | pass | 4/4 | `src/fuzz/corpus.rs` |
| 7 | Crash detection | Panics caught, classified with location | `crash_is_captured_with_location_and_normalized_signature` | pass | `crash:interp:src/lang/interp.rs:<line>:...` | `src/fuzz/exec.rs` |
| 8 | Timeout detection | Fuel and wall clock, opt-in as findings | `timeouts_are_classified_and_opt_in`, `fuel_and_wall_clock_bound_infinite_loops` | pass | `timeout:fuel`, `timeout:wall-clock` | `src/lang/interp.rs` |
| 9 | Invariant violations | Round trip, type soundness, optimizer equivalence | planted acceptance | pass | each oracle fires on its planted bug | `src/fuzz/exec.rs` |
| 10 | Planted bug found from fixed seed | Found within 20 000 iterations from seed 1 | `planted_bugs_are_found_and_minimized`; CI `fuzz-acceptance` | pass (local) | all four found | `src/planted.rs` |
| 11 | Minimization | Smaller reproducer, same signature, passes without the bug | same test; `minimizer_preserves_signature_and_respects_budget` | pass | 52 → 8, 58 → 6, 33 → 4, 57 → 7 nodes | `src/fuzz/minimize.rs` |
| 12 | Structured report | JSON with reproducer, seed, iteration, class, signature, minimized program, stats | `run_with_planted_bug_exits_1_and_writes_a_structured_report` | pass | fields asserted | `src/fuzz/engine.rs` |
| 13 | No false positives | Correct implementation yields no findings | `correct_implementation_has_no_findings`; 8 × 50 000-iteration campaigns | pass | 0 findings | — |
| 14 | Regression fixtures | Each fixture replays to its declared result | `regression_fixtures_replay_to_their_expected_signature` | pass | 6 fixtures | `fixtures/regressions/` |
| 15 | Coverage honesty | Execution coverage documented as not CFG | `docs/COVERAGE.md` | pass | documented | `src/coverage.rs` |
| 16 | Hostile input safety | No stack overflow / OOM from crafted input | `hostile_nesting_is_rejected_not_stack_overflowed`, `oversized_source_is_rejected`, `parser_never_panics` | pass | rejected with messages | `src/lang/parser.rs` |
| 17 | libFuzzer harness | Builds, runs clean, detects a planted bug | manual commands above; `libfuzzer.yml` | pass (local, manual) | 376 586 clean runs; planted bug → crash artifact | `fuzz/` |
| 18 | MSRV | Builds and tests on Rust 1.85 | `cargo +1.85 test --locked` | pass (local) | 39 passed | `Cargo.toml` |
| 19 | Clean clone | README quick start works from a fresh clone | clean-clone commands above | pass | identical output, 39/39, clean tree | `README.md` |
| 20 | Linux / Windows | Tests pass | CI `test` matrix | not yet run | — | `.github/workflows/ci.yml` |
| 21 | CI green | All jobs pass on GitHub | Actions | not yet run | — | `.github/workflows/` |
