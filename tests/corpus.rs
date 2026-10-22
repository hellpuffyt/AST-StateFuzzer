#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Corpus storage, canonical deduplication and behavioural deduplication.

use std::fs;

use astfuzz::fuzz::corpus::{dedup_by_behavior, file_name, load, save};
use astfuzz::fuzz::exec::ExecConfig;
use astfuzz::lang::lexer::MAX_SOURCE_BYTES;
use astfuzz::lang::parser::parse;

#[test]
fn canonical_duplicates_collapse_and_bad_files_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.mini"), "print 1+2;").unwrap();
    fs::write(dir.path().join("b.mini"), "# same program, other formatting\nprint (1) + 2 ;").unwrap();
    fs::write(dir.path().join("c.mini"), "print 3;").unwrap();
    fs::write(dir.path().join("d.mini"), "print (;").unwrap();
    fs::write(dir.path().join("e.mini"), "#".repeat(MAX_SOURCE_BYTES + 1)).unwrap();
    fs::write(dir.path().join("ignored.txt"), "print 4;").unwrap();
    let l = load(dir.path()).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert_eq!(l.duplicates.len(), 1);
    assert_eq!(l.rejected.len(), 2);
    assert!(l.rejected.iter().any(|(_, why)| why.contains("larger than")));
}

#[test]
fn save_is_canonical_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let p = parse("print   (1)+2;").unwrap();
    let a = save(dir.path(), &p).unwrap();
    let b = save(dir.path(), &p).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.file_name().unwrap().to_str().unwrap(), file_name(&p));
    assert_eq!(fs::read_to_string(&a).unwrap(), "print 1 + 2;\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn behavioural_dedup_keeps_one_per_behaviour() {
    let dir = tempfile::tempdir().unwrap();
    for (n, src) in [("1", "print 1 + 2;"), ("2", "print 2 + 1;"), ("3", "print 1 - 2;")] {
        fs::write(dir.path().join(format!("{n}.mini")), src).unwrap();
    }
    let l = load(dir.path()).unwrap();
    let (keep, dropped) = dedup_by_behavior(&l.entries, &ExecConfig::default());
    // `1 + 2` and `2 + 1` print the same value with the same features.
    assert_eq!((keep.len(), dropped.len()), (2, 1));
}

#[test]
fn shipped_seed_corpus_loads_cleanly() {
    let l = load(std::path::Path::new("corpus")).unwrap();
    assert!(l.rejected.is_empty(), "{:?}", l.rejected);
    assert!(l.entries.len() >= 3);
}
