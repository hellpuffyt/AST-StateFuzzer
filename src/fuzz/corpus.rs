//! On-disk seed corpus.
//!
//! A corpus is a directory of `*.mini` files. Each file is stored under the
//! FNV-1a-64 hash of its **canonical** pretty-printed form, so two programs
//! that differ only in whitespace, comments or redundant parentheses dedupe to
//! one file. [`dedup_by_behavior`] additionally collapses programs whose runs
//! are indistinguishable (same output, same result, same coverage features).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::exec::{ExecConfig, check};
use crate::lang::ast::Program;
use crate::lang::lexer::MAX_SOURCE_BYTES;
use crate::lang::parser::parse;
use crate::lang::printer::print_program;

/// 64-bit FNV-1a: tiny, stable across platforms and releases.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Hash of the canonical text of a program.
pub fn canonical_hash(p: &Program) -> u64 {
    fnv1a(print_program(p).as_bytes())
}

/// A program loaded from disk.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Where it came from.
    pub path: PathBuf,
    /// Parsed program.
    pub program: Program,
}

/// Result of loading a corpus directory.
#[derive(Debug, Default)]
pub struct Loaded {
    /// Successfully parsed, canonically unique entries, sorted by file name.
    pub entries: Vec<Entry>,
    /// Files that failed to parse, with the reason.
    pub rejected: Vec<(PathBuf, String)>,
    /// Files skipped as canonical duplicates of an earlier entry.
    pub duplicates: Vec<PathBuf>,
}

/// Load every `*.mini` file in `dir` (non-recursive, sorted by name so the
/// order — and therefore every fuzzing run — is deterministic).
pub fn load(dir: &Path) -> io::Result<Loaded> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mini") && p.is_file())
        .collect();
    paths.sort();
    let mut out = Loaded::default();
    let mut seen = BTreeSet::new();
    for path in paths {
        if fs::metadata(&path)?.len() > MAX_SOURCE_BYTES as u64 {
            out.rejected.push((path, format!("file is larger than {MAX_SOURCE_BYTES} bytes")));
            continue;
        }
        let text = fs::read_to_string(&path)?;
        match parse(&text) {
            Ok(program) => {
                if seen.insert(canonical_hash(&program)) {
                    out.entries.push(Entry { path, program });
                } else {
                    out.duplicates.push(path);
                }
            }
            Err(e) => out.rejected.push((path, e.to_string())),
        }
    }
    Ok(out)
}

/// File name a program is stored under.
pub fn file_name(p: &Program) -> String {
    format!("{:016x}.mini", canonical_hash(p))
}

/// Save a program in canonical form; returns its path. Idempotent.
pub fn save(dir: &Path, p: &Program) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(file_name(p));
    if !path.exists() {
        fs::write(&path, print_program(p))?;
    }
    Ok(path)
}

/// Behaviour signature: output, final result and coverage features.
pub fn behavior_hash(p: &Program, cfg: &ExecConfig) -> u64 {
    let v = check(p, cfg);
    let mut s = String::new();
    if let Some(o) = &v.outcome {
        s.push_str(&format!("{:?}|{:?}|", o.output, o.result));
    }
    if let Some(f) = &v.failure {
        s.push_str(&f.signature);
    }
    s.push_str(&format!("{:?}", v.coverage.codes()));
    fnv1a(s.as_bytes())
}

/// Keep the first program (in input order) of each behaviour class.
pub fn dedup_by_behavior(entries: &[Entry], cfg: &ExecConfig) -> (Vec<Entry>, Vec<PathBuf>) {
    let mut seen = BTreeSet::new();
    let mut keep = Vec::new();
    let mut dropped = Vec::new();
    for e in entries {
        if seen.insert(behavior_hash(&e.program, cfg)) {
            keep.push(e.clone());
        } else {
            dropped.push(e.path.clone());
        }
    }
    (keep, dropped)
}
