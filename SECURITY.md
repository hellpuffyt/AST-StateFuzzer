# Security policy

## Supported versions

Only the latest release on `main` receives fixes. This is experimental 0.x software.

## Reporting a vulnerability

Please **do not** open a public issue. Report privately through GitHub's private vulnerability reporting:
https://github.com/hellpuffyt/AST-StateFuzzer/security/advisories/new

Include the version or commit, the exact command, and a reproducer. You can expect an acknowledgement within
7 days. Please allow time for a fix before public disclosure.

## Scope and threat model

`astfuzz` parses and executes **untrusted Mini programs** (corpus files, replay inputs, generated programs). The
relevant guarantees are:

- Programs only run inside the in-process Mini interpreter. Nothing is passed to a shell, compiled to native code,
  or given access to files, the network, or the environment. Mini has no I/O besides `print`.
- Every run is bounded by a step budget (fuel) and a wall-clock check; the parser rejects inputs over 1 MiB and
  nesting deeper than 200, so hostile input cannot exhaust the stack of later recursive passes.
- `astfuzz corpus dedup` never writes into its input directory; `run --save-corpus` only adds files (atomically, up to 10 000 per directory) and never deletes.

Out of scope: denial of service through very large `--iterations` or `--fuel` values you choose yourself, and the
behaviour of other targets you wire into the harness yourself (see "Fuzzing other targets safely" in
`docs/ARCHITECTURE.md`).
