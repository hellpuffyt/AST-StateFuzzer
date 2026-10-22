# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- One non-UTF-8 or unreadable file in a corpus directory aborted `corpus::load` (and `astfuzz run --corpus`
  exited 2); bad files are now rejected one by one with the reason.
- `corpus::save` wrote in place and skipped any existing file under the canonical name, so a truncated file from an
  interrupted write was kept forever; writes are now atomic and a non-canonical file is replaced.

### Added
- Corpus growth cap: `corpus::MAX_CORPUS_FILES` (10 000), `load_with_limit`, `Loaded::over_limit`, `save_all`
  with `SaveSummary`; `run --save-corpus` and `corpus stats` report the cap.
- Property tests that generated and mutated programs satisfy the grammar and the generator budgets for arbitrary
  seeds and settings; classification, minimizer-signature and reproducibility tests; measured test-suite coverage
  and AddressSanitizer runs (`docs/COVERAGE.md`).

### Changed
- Documented the exact `--max-stmts` bound (a bounded loop adds its counter `let` and increment).

## [0.1.0] - 2026-10-22

### Added
- Mini target language: lexer, parser (1 MiB / nesting-200 limits), canonical printer, type checker,
  fuel- and wall-clock-bounded interpreter, constant-folding optimizer.
- Type-aware generator, eight AST mutation operators, pinned xoshiro256** RNG and byte-driven source.
- Oracles: round trip (parse + AST), type soundness, optimizer equivalence; crash capture with panic location;
  opt-in timeout findings.
- Execution-feature coverage feedback (explicitly not CFG coverage).
- Grammar-aware, signature-preserving minimizer.
- On-disk corpus with canonical and behavioural deduplication.
- `astfuzz` CLI: `run`, `replay`, `minimize`, `corpus stats|dedup`, `gen`; JSON reports.
- Four planted bugs with acceptance tests and regression fixtures.
- Experimental libFuzzer harness (`fuzz/`) with `ASTFUZZ_PLANT` support.
