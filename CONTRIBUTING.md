# Contributing to AST-StateFuzzer

Thanks for helping! Small, focused pull requests are easiest to review.

## Setup

```sh
git clone https://github.com/hellpuffyt/AST-StateFuzzer && cd AST-StateFuzzer
cargo test --locked          # Rust 1.85 or newer
```

Optional, for the libFuzzer harness: a nightly toolchain and `cargo install cargo-fuzz`.

## Ground rules

- **Every fix needs a regression test**, usually a minimized `.mini` file in `fixtures/regressions/` with
  `# plant:` and `# expect:` headers (see the existing files).
- **New oracles, mutators and reduction passes must prove they work**: add or reuse a planted bug that only they
  can find or reduce, and assert it in `tests/fuzzer.rs`.
- **Seeds are a contract.** If you change the RNG, the generator's choice order or the mutation weights, existing
  `(seed, iteration)` reproducers stop reproducing. Say so in the PR and the changelog.
- **No false positives.** `correct_implementation_has_no_findings` must stay green: an oracle that fires on the
  correct implementation is a bug in the oracle.
- Keep claims honest: coverage here is execution-feature coverage, not CFG coverage (see `docs/COVERAGE.md`).

## Before opening a PR

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
```

Good starting points are listed in [`docs/good-first-issues.md`](docs/good-first-issues.md).

By contributing you agree that your work is dual-licensed under MIT OR Apache-2.0, like the rest of the project.
