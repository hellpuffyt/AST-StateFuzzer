# Coverage: what is measured, and what is not

## What `astfuzz` measures: execution features

The interpreter reports **semantic features** of each run (`src/coverage.rs`):

| Feature | Example |
|---|---|
| statement kind executed | an assignment ran |
| operator × result class | `%` produced a negative value; `/` raised an error |
| operator nesting | a `-` had a `-` as its right operand |
| branch direction | an `if` took its else branch |
| loop iteration bucket | a `while` ran 4–7 times |
| printed value class | a negative number was printed |
| terminating error | the run ended with `Overflow` |

A program that shows a feature not seen before is added to the corpus. This is cheap, portable and fully
deterministic, and it measurably helps: with operator-nesting features the corpus for seed 2 grows to 167
entries and 299 features in 50 000 iterations, versus 24 entries and 57 features without them (measured on this
repository's own CLI; see `docs/PRODUCTION_READINESS.md`).

## What it is **not**: control-flow-graph coverage

No compiler instrumentation is involved. `astfuzz` does **not** know which basic blocks or CFG edges of the
interpreter, optimizer or type checker executed. Two runs with identical features can take different paths through
the Rust code, and vice versa. Do not read the feature count as a CFG coverage percentage.

## Getting real edge coverage

1. **libFuzzer harness (included, experimental).** `fuzz/fuzz_targets/oracles.rs` lets libFuzzer's bytes drive the
   structure-aware generator through `ByteSource`. `cargo fuzz` compiles the crate with LLVM SanitizerCoverage, so
   libFuzzer's `cov:` counter *is* edge coverage of the compiled code, and it guides input selection. Measured here:
   `cov: 1577 ft: 7381` after 376 586 executions in 60 s (loaded Apple M5). With `ASTFUZZ_PLANT=opt-sub-assoc` the
   harness stops with a crash artifact for the planted bug.
2. **Coverage reports.** `cargo +nightly fuzz coverage oracles` replays a corpus under `-C instrument-coverage`;
   `llvm-cov` (from the `llvm-tools` rustup component) turns the profile into line/region reports. Not run; the
   test-suite measurement below uses the same instrumentation.
3. **In-process edge feedback for `astfuzz run`** would need SanitizerCoverage callbacks or a similar mechanism in
   the engine; it is on the roadmap, not implemented.

## Test-suite coverage (measured)

Source-based (LLVM `-C instrument-coverage`) coverage of the crate while `cargo test --release` runs, including the
`astfuzz` binary that the CLI tests launch. Measured on 2026-10-22 with `cargo llvm-cov --release --summary-only`
(cargo-llvm-cov 0.9.1, rustc 1.98.0, Apple M5):

| file | regions | lines |
|---|---|---|
| `bin/astfuzz.rs` | 89.3 % | 89.6 % |
| `coverage.rs` | 90.0 % | 87.5 % |
| `fuzz/corpus.rs` | 92.4 % | 96.9 % |
| `fuzz/engine.rs` | 94.7 % | 94.7 % |
| `fuzz/exec.rs` | 83.3 % | 88.0 % |
| `fuzz/generator.rs` | 99.7 % | 100 % |
| `fuzz/minimize.rs` | 98.6 % | 99.3 % |
| `fuzz/mutate.rs` | 98.6 % | 98.5 % |
| `fuzz/rng.rs` | 98.0 % | 98.2 % |
| `lang/*` (ast, interp, lexer, opt, parser, printer, typeck) | 90.5–100 % | 97.2–100 % |
| `planted.rs` | 91.7 % | 92.3 % |
| **total** | **95.8 %** (4 082 regions) | **96.9 %** (2 026 lines) |

Most uncovered regions in `fuzz/exec.rs` are the crash paths of the printer, parser, type checker and optimizer
stages: no planted bug panics there, so they are reachable only by a real bug. This measures what the *tests*
execute; it says nothing about what a fuzzing campaign explores (that is the execution-feature count above, or
libFuzzer's `cov:` for the harness).

### Sanitizers

- `RUSTFLAGS=-Zsanitizer=address cargo +nightly test --release --target aarch64-apple-darwin --tests`: all 58 test
  functions pass (`--tests`, so the one doctest is not included); the `astfuzz` binary used by the CLI tests is ASan-linked. The standard library was not rebuilt with
  ASan (`-Zbuild-std` not used); the crate itself has no `unsafe` (`forbid(unsafe_code)`).
- `cargo +nightly fuzz run -O <target> -- -max_total_time=60` (cargo-fuzz's default AddressSanitizer build):
  `oracles` 364 005 runs, `cov: 1578 ft: 7365`, no crash; `parse_roundtrip` 2 646 973 runs, `cov: 609 ft: 3276`,
  no crash.
- Not run: MemorySanitizer and ThreadSanitizer (need `-Zbuild-std` and, for MSan, Linux), Miri.
