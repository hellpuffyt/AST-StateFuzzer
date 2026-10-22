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

#[test]
fn garbage_and_truncated_files_are_rejected_one_by_one() {
    // One bad file must never abort loading the rest of the corpus.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a-good.mini"), "print 1;").unwrap();
    fs::write(dir.path().join("b-binary.mini"), [0xff, 0xfe, 0x00, 0x9f, 0x92, 0x96]).unwrap();
    fs::write(dir.path().join("c-truncated.mini"), "let x = 1;\nwhile x < 3 { x = x +").unwrap();
    fs::write(dir.path().join("d-empty.mini"), "").unwrap();
    fs::create_dir(dir.path().join("e-dir.mini")).unwrap();
    fs::write(dir.path().join("f-good.mini"), "print 2;").unwrap();
    let l = load(dir.path()).unwrap();
    let names: Vec<_> =
        l.entries.iter().map(|e| e.path.file_name().unwrap().to_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["a-good.mini", "d-empty.mini", "f-good.mini"]);
    let rejected: Vec<_> =
        l.rejected.iter().map(|(p, _)| p.file_name().unwrap().to_str().unwrap().to_owned()).collect();
    assert_eq!(rejected, ["b-binary.mini", "c-truncated.mini"]);
    assert!(l.rejected[0].1.contains("UTF-8"), "{}", l.rejected[0].1);
}

#[test]
fn save_repairs_a_corrupted_file_under_the_canonical_name() {
    // A crash during an earlier (non-atomic) write could leave a truncated
    // file under the program's canonical name; saving again must fix it, not
    // trust it because the name exists.
    let dir = tempfile::tempdir().unwrap();
    let p = parse("let x = 1; print x;").unwrap();
    let path = dir.path().join(file_name(&p));
    fs::write(&path, "let x = 1;\n").unwrap();
    save(dir.path(), &p).unwrap();
    assert_eq!(parse(&fs::read_to_string(&path).unwrap()).unwrap(), p);
    let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "no temporary files left behind: {names:?}");
}

#[test]
fn corpus_growth_on_disk_is_capped() {
    use astfuzz::fuzz::corpus::save_all;
    let dir = tempfile::tempdir().unwrap();
    let programs: Vec<_> = (0..10).map(|i| parse(&format!("print {i};")).unwrap()).collect();
    let s = save_all(dir.path(), &programs[..6], 8).unwrap();
    assert_eq!((s.written, s.already_present, s.skipped_over_limit), (6, 0, 0));
    // Re-saving present programs costs nothing; new ones stop at the cap.
    let s = save_all(dir.path(), &programs, 8).unwrap();
    assert_eq!((s.written, s.already_present, s.skipped_over_limit), (2, 6, 2));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 8);
}

#[test]
fn loading_is_capped_and_reports_what_was_skipped() {
    use astfuzz::fuzz::corpus::load_with_limit;
    let dir = tempfile::tempdir().unwrap();
    for i in 0..5 {
        fs::write(dir.path().join(format!("{i}.mini")), format!("print {i};")).unwrap();
    }
    let l = load_with_limit(dir.path(), 3).unwrap();
    assert_eq!(l.entries.len(), 3);
    assert_eq!(l.over_limit.len(), 2);
    assert_eq!(l.over_limit[0].file_name().unwrap(), "3.mini");
}
