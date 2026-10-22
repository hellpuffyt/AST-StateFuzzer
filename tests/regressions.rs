#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Every file in `fixtures/regressions/` declares, in header comments, which
//! planted bug to enable and which signature to expect (`pass` = no failure).

use std::fs;

use astfuzz::fuzz::exec::{ExecConfig, check};
use astfuzz::lang::parser::parse;
use astfuzz::planted::{Bugs, PlantedBug};

fn header<'a>(text: &'a str, key: &str) -> &'a str {
    let prefix = format!("# {key}: ");
    text.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing `{prefix}` header"))
        .trim()
}

#[test]
fn regression_fixtures_replay_to_their_expected_signature() {
    let mut n = 0;
    let mut paths: Vec<_> =
        fs::read_dir("fixtures/regressions").unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let text = fs::read_to_string(&path).unwrap();
        let plant = header(&text, "plant");
        let expect = header(&text, "expect");
        let bugs =
            if plant == "none" { Bugs::NONE } else { Bugs::NONE.with(plant.parse::<PlantedBug>().unwrap()) };
        let exec = ExecConfig { bugs, ..ExecConfig::default() };
        let v = check(&parse(&text).unwrap(), &exec);
        match (expect, v.failure) {
            ("pass", None) => {}
            ("pass", Some(f)) => panic!("{}: expected pass, got {}", path.display(), f.signature),
            (sig, Some(f)) => {
                assert!(f.signature.starts_with(sig), "{}: got {}", path.display(), f.signature)
            }
            (sig, None) => panic!("{}: expected {sig}, but it passed", path.display()),
        }
        n += 1;
    }
    assert!(n >= 6, "expected at least 6 regression fixtures, found {n}");
}
