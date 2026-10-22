//! libFuzzer drives the *structure-aware generator*: its bytes become the
//! generator's choices (`ByteSource`), so every input is a valid Mini program,
//! and libFuzzer's own SanitizerCoverage edge feedback (real CFG-edge coverage
//! of the compiled fuzzer, including the interpreter) steers the search.
//! Any oracle failure is turned into a panic so libFuzzer records the input.
//!
//! `ASTFUZZ_PLANT=<bug>[,<bug>]` enables planted bugs, to check that the
//! harness really detects them (see docs/COVERAGE.md).
#![no_main]

use std::sync::OnceLock;

use astfuzz::fuzz::exec::{ExecConfig, check};
use astfuzz::fuzz::generator::{GenConfig, generate};
use astfuzz::fuzz::rng::ByteSource;
use astfuzz::lang::printer::print_program;
use astfuzz::planted::{Bugs, PlantedBug};
use libfuzzer_sys::fuzz_target;

fn exec_config() -> ExecConfig {
    static CFG: OnceLock<ExecConfig> = OnceLock::new();
    *CFG.get_or_init(|| {
        let bugs = std::env::var("ASTFUZZ_PLANT")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<PlantedBug>().unwrap_or_else(|e| panic!("ASTFUZZ_PLANT: {e}")))
            .collect::<Bugs>();
        ExecConfig { bugs, ..ExecConfig::default() }
    })
}

fuzz_target!(|data: &[u8]| {
    let program = generate(&mut ByteSource::new(data), &GenConfig::default());
    if let Some(f) = check(&program, &exec_config()).failure {
        panic!("{}\n{}\n--- program ---\n{}", f.signature, f.detail, print_program(&program));
    }
});
