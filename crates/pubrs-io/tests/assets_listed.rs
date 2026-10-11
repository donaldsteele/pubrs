//! Locked Decision 5: every bundled data file under `assets/` and every
//! test file under `tests/corpus/` is listed in `ASSETS.md` with its
//! SHA-256, either in the row itself or through a `SHA256SUMS` file the
//! row names. The hashes are verified.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sha256(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return; // The folder does not exist (yet).
    };
    for e in rd {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

struct Row {
    path: String,
    sha: String,
}

fn rows() -> Vec<Row> {
    let text = std::fs::read_to_string(root().join("ASSETS.md")).unwrap();
    let cell = |s: &str| s.trim().trim_matches('`').to_string();
    text.lines()
        .filter(|l| l.starts_with('|'))
        .filter_map(|l| {
            let c: Vec<&str> = l.trim_matches('|').split('|').collect();
            (c.len() >= 4).then(|| Row {
                path: cell(c[0]),
                sha: cell(c[c.len() - 1]),
            })
        })
        .collect()
}

/// `hash  relative/path` lines of a SHA256SUMS file, keyed by absolute path.
fn sums(file: &Path) -> HashMap<PathBuf, String> {
    let dir = file.parent().unwrap().to_path_buf();
    std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let (h, p) = l.split_once(char::is_whitespace)?;
            Some((dir.join(p.trim().trim_start_matches('*')), h.to_string()))
        })
        .collect()
}

#[test]
fn every_asset_and_corpus_file_is_listed_with_its_hash() {
    let rows = rows();
    let mut files = Vec::new();
    files_under(&root().join("assets"), &mut files);
    files_under(&root().join("tests/corpus"), &mut files);
    let mut checked = 0;
    for f in files {
        let rel = f
            .strip_prefix(root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if f.file_name().is_some_and(|n| n == "SHA256SUMS") {
            continue; // A manifest cannot list its own hash.
        }
        let row = rows
            .iter()
            .filter(|r| r.path == rel || (r.path.ends_with('/') && rel.starts_with(&r.path)))
            .max_by_key(|r| r.path.len())
            .unwrap_or_else(|| panic!("{rel} is not listed in ASSETS.md"));
        let want = if row.sha.len() == 64 && row.sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            assert_eq!(row.path, rel, "a bare hash belongs to one file: {rel}");
            row.sha.clone()
        } else {
            let manifest = root().join(&row.sha);
            assert!(manifest.is_file(), "{rel}: {} is not a file", row.sha);
            sums(&manifest)
                .remove(&f)
                .unwrap_or_else(|| panic!("{rel} is not in {}", row.sha))
        };
        assert_eq!(sha256(&f), want, "{rel}: SHA-256 differs from ASSETS.md");
        checked += 1;
    }
    eprintln!("{checked} files checked");
}
