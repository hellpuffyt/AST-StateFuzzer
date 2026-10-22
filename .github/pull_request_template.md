## What and why

<!-- One logical change. Link the issue if there is one. -->

## Checklist

- [ ] `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings` are clean
- [ ] `cargo test --all-features --locked` passes
- [ ] New oracles/mutators/reducers come with a test (ideally a planted bug they find)
- [ ] Changes to the generator or RNG: explain whether existing seeds still reproduce (`rng_stream_is_pinned`)
- [ ] Every fixed bug has a regression fixture in `fixtures/regressions/`
- [ ] Docs (`README.md`, `docs/`) updated if behaviour or flags changed
