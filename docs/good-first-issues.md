# Good first issues

Each of these is self-contained, has a clear test, and touches one or two files.

1. **Add a `--max-program-size` flag** (`src/bin/astfuzz.rs`, `FuzzConfig::max_program_size`). Today the 400-node
   limit is only configurable from the library. Test: a CLI test that a tiny limit yields `oversize skipped > 0`.
2. **Minimizer pass: inline single-use `let`s** (`src/fuzz/minimize.rs`). `let v0 = 1; print 0 - (0 - v0);`
   could become `print 0 - (0 - 1);` when that still fails the same way. Test: a new case in
   `tests/fuzzer.rs` showing a smaller result than today for a hand-written program.
3. **New planted bug: `interp-shortcircuit`** (`src/planted.rs`, `src/lang/interp.rs`): evaluate both sides of
   `&&`. It should be found by the `optimizer-equivalence` or a new oracle — or show that it is *not* found and
   document why (that is a useful result too). Add a regression fixture.
4. **`corpus stats --json`** (`src/bin/astfuzz.rs`): machine-readable corpus statistics. Test in `tests/cli.rs`.
5. **Feature: count of distinct signatures over time** in `Stats` (`src/fuzz/engine.rs`), e.g. iteration of first
   discovery per signature, to plot time-to-bug. Test: deterministic across two runs (`same_seed_same_campaign`).
