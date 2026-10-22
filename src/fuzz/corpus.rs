//! On-disk seed corpus.
//!
//! A corpus is a directory of `*.mini` files. Each file is stored under the
//! FNV-1a-64 hash of its **canonical** pretty-printed form, so two programs
//! that differ only in whitespace, comments or redundant parentheses dedupe to
//! one file. [`dedup_by_behavior`] additionally collapses programs whose runs
//! are indistinguishable (same output, same result, same coverage features).

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
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

/// Most `*.mini` files [`load`] reads and [`save_all`] lets a corpus
/// directory grow to. The in-memory corpus of a campaign is bounded
/// separately: a program joins it only if it adds an execution feature, so it
/// never exceeds the (finite) feature space.
pub const MAX_CORPUS_FILES: usize = 10_000;

/// Result of loading a corpus directory.
#[derive(Debug, Default)]
pub struct Loaded {
    /// Successfully parsed, canonically unique entries, sorted by file name.
    pub entries: Vec<Entry>,
    /// Files that could not be used (unreadable, not UTF-8, too large, or not
    /// a valid program), with the reason.
    pub rejected: Vec<(PathBuf, String)>,
    /// Files skipped as canonical duplicates of an earlier entry.
    pub duplicates: Vec<PathBuf>,
    /// Files not read because the directory holds more than the limit
    /// (the last ones in name order).
    pub over_limit: Vec<PathBuf>,
}

/// Load every `*.mini` file in `dir` (non-recursive, sorted by name so the
/// order — and therefore every fuzzing run — is deterministic), reading at
/// most [`MAX_CORPUS_FILES`] files.
pub fn load(dir: &Path) -> io::Result<Loaded> {
    load_with_limit(dir, MAX_CORPUS_FILES)
}

/// [`load`] with an explicit file limit. Only an unreadable *directory* is an
/// error; every problem with an individual file (permissions, invalid UTF-8,
/// truncation, size, a directory named `*.mini`) rejects just that file.
pub fn load_with_limit(dir: &Path, max_files: usize) -> io::Result<Loaded> {
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mini") && p.is_file())
        .collect();
    paths.sort();
    let mut out = Loaded::default();
    if paths.len() > max_files {
        out.over_limit = paths.split_off(max_files);
    }
    let mut seen = BTreeSet::new();
    for path in paths {
        let text = match read_source(&path) {
            Ok(t) => t,
            Err(why) => {
                out.rejected.push((path, why));
                continue;
            }
        };
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

/// Read one corpus file, refusing oversized and non-UTF-8 files without
/// reading more than the size limit.
fn read_source(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("cannot read: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read: {e}"))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!("file is larger than {MAX_SOURCE_BYTES} bytes"));
    }
    String::from_utf8(bytes).map_err(|e| format!("not valid UTF-8 ({})", e.utf8_error()))
}

/// File name a program is stored under.
pub fn file_name(p: &Program) -> String {
    format!("{:016x}.mini", canonical_hash(p))
}

/// Save a program in canonical form; returns its path. Idempotent.
///
/// The file is written to a temporary name and renamed into place, so a
/// crash never leaves a partial file under a canonical name. A file that
/// already exists under the canonical name is kept only if it holds exactly
/// the canonical text; anything else (e.g. a truncated file from an older,
/// interrupted write) is replaced.
pub fn save(dir: &Path, p: &Program) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(file_name(p));
    let text = print_program(p);
    if fs::read(&path).is_ok_and(|existing| existing == text.as_bytes()) {
        return Ok(path);
    }
    let tmp = dir.join(format!(".{}.tmp", file_name(p)));
    let written = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map(|()| path)
}

/// What [`save_all`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveSummary {
    /// New files written.
    pub written: usize,
    /// Programs whose canonical file was already present (and correct).
    pub already_present: usize,
    /// Programs not saved because the directory reached the file limit.
    pub skipped_over_limit: usize,
}

/// Save `programs`, never letting `dir` hold more than `max_files` `*.mini`
/// files. Programs already present cost nothing; new ones are written in
/// order until the limit is reached and the rest are counted as skipped.
pub fn save_all(dir: &Path, programs: &[Program], max_files: usize) -> io::Result<SaveSummary> {
    fs::create_dir_all(dir)?;
    let mut count = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "mini"))
        .count();
    let mut s = SaveSummary::default();
    for p in programs {
        if dir.join(file_name(p)).is_file() {
            save(dir, p)?;
            s.already_present += 1;
        } else if count < max_files {
            save(dir, p)?;
            count += 1;
            s.written += 1;
        } else {
            s.skipped_over_limit += 1;
        }
    }
    Ok(s)
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
