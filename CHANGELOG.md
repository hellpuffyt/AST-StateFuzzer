# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
