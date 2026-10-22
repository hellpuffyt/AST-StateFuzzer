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
   `llvm-cov` (from the `llvm-tools` rustup component) turns the profile into line/region reports. This was not run
   for this release.
3. **In-process edge feedback for `astfuzz run`** would need SanitizerCoverage callbacks or a similar mechanism in
   the engine; it is on the roadmap, not implemented.
