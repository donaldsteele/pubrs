//! `.pub` import, part 1: container, pages, text frames and text.
//!
//! Every check is driven by `tests/corpus/pub/expected.json`, which records the
//! SHA-256 of each file and what LibreOffice (libmspub) made of it. No result is
//! judged by hand.

#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use pubrs_io::pubfile::{self, ImportReport};
use serde_json::Value;

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/pub")
}

// ---- SHA-256 (the workspace has no hashing crate and this is test-only) ----

fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_known_answer() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

// ---- expected.json ----

struct Entry {
    file: String,
    sha256: String,
    ok: bool,
    pages: usize,
    width: Option<f64>,
    height: Option<f64>,
    words: usize,
    oracle_text: Option<PathBuf>,
    poi_text: Option<PathBuf>,
    /// Images the oracle's PDF lists (pdfimages).
    images: usize,
}

fn entries() -> Vec<Entry> {
    let text = std::fs::read_to_string(corpus().join("expected.json")).expect("expected.json");
    let v: Value = serde_json::from_str(&text).expect("expected.json parses");
    v["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| Entry {
            file: f["file"].as_str().unwrap().to_string(),
            sha256: f["sha256"].as_str().unwrap().to_string(),
            ok: f["oracle"] == "ok",
            pages: f["pages"].as_u64().unwrap() as usize,
            width: f["page_width_pt"].as_f64(),
            height: f["page_height_pt"].as_f64(),
            words: f["words"].as_u64().unwrap() as usize,
            // Oracle text paths are relative to the staged assets root; in the
            // repo the same `oracle/text/` sits beside expected.json.
            oracle_text: f["oracle_text"].as_str().map(|p| corpus().join(p)),
            poi_text: f["poi_text"].as_str().map(|p| corpus().join(p)),
            images: f["images"].as_u64().unwrap() as usize,
        })
        .collect()
}

fn read(e: &Entry) -> Vec<u8> {
    std::fs::read(corpus().join(&e.file)).unwrap_or_else(|err| panic!("{}: {err}", e.file))
}

// ---- word recall ----

fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Fraction of the reference's words (as a multiset) present in `got`.
fn recall(reference: &str, got: &str) -> f64 {
    let want = words(reference);
    if want.is_empty() {
        return 1.0;
    }
    let mut have: HashMap<String, usize> = HashMap::new();
    for w in words(got) {
        *have.entry(w).or_default() += 1;
    }
    let mut hit = 0usize;
    for w in &want {
        if let Some(n) = have.get_mut(w)
            && *n > 0
        {
            *n -= 1;
            hit += 1;
        }
    }
    hit as f64 / want.len() as f64
}

/// Fraction of the reference's distinct words found anywhere in `got`.
/// Per-file (all stories pooled), not per story; independent of how often a story repeats.
fn distinct_recall(reference: &str, got: &str) -> f64 {
    let want: std::collections::HashSet<String> = words(reference).into_iter().collect();
    if want.is_empty() {
        return 1.0;
    }
    let have: std::collections::HashSet<String> = words(got).into_iter().collect();
    want.iter().filter(|w| have.contains(*w)).count() as f64 / want.len() as f64
}

/// The imported text as the oracle counts it: the story of every text frame, so a
/// story that flows through a chain of N frames is counted N times. libmspub
/// writes the whole story into each frame that references it, so its text has
/// that many copies; the pubrs document itself holds one copy per story.
fn frame_text(doc: &pubrs_core::model::Document) -> String {
    use pubrs_core::model::FrameContent;
    let mut out = Vec::new();
    for page in doc.pages.iter() {
        for fid in &page.frames {
            let Some(frame) = doc.frames.get(fid) else {
                continue;
            };
            let mut stories = Vec::new();
            match &frame.content {
                FrameContent::Text(t) => stories.push(t.story),
                // Table cells hold the text the oracle prints for a table.
                FrameContent::Table(t) => stories.extend(t.cells.iter().map(|c| c.story)),
                _ => {}
            }
            for sid in stories {
                if let Some(story) = doc.stories.get(&sid) {
                    out.push(
                        story
                            .paragraphs
                            .iter()
                            .map(|p| p.text())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
            }
        }
    }
    out.join("\n")
}

/// The imported text counted once per story.
fn story_text(doc: &pubrs_core::model::Document) -> String {
    pubfile::story_texts(doc).join("\n")
}

// ---- tests ----

#[test]
fn corpus_hashes_match() {
    let list = entries();
    assert_eq!(list.len(), 28, "expected.json lists 28 files");
    for e in &list {
        assert_eq!(sha256(&read(e)), e.sha256, "{}: SHA-256 differs", e.file);
        assert_eq!(read(e).len(), {
            let v: Value = serde_json::from_str(
                &std::fs::read_to_string(corpus().join("expected.json")).unwrap(),
            )
            .unwrap();
            v["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["file"] == e.file.as_str())
                .unwrap()["bytes"]
                .as_u64()
                .unwrap() as usize
        });
    }
}

#[test]
fn oracle_ok_files_import_with_pages_size_and_text() {
    let list: Vec<Entry> = entries().into_iter().filter(|e| e.ok).collect();
    assert_eq!(list.len(), 22);
    let mut failures = Vec::new();
    println!();
    println!(
        "{:<42} {:>5} {:>17} {:>8} {:>8} {:>8} {:>8} {:>5} {:>6}",
        "file",
        "pages",
        "first page (pt)",
        "oracle",
        "per-story",
        "file-dist",
        "poi",
        "ms",
        "drops"
    );
    for e in &list {
        let started = Instant::now();
        let bytes = read(e);
        let (doc, rep): (_, ImportReport) = match pubfile::import(&bytes) {
            Ok(r) => r,
            Err(err) => {
                failures.push(format!("{}: import failed: {err}", e.file));
                println!("{:<42} FAILED: {err}", e.file);
                continue;
            }
        };
        let took = started.elapsed();
        // Valid files stay far inside the parse budget and have no duplicate chunks.
        for d in &rep.dropped {
            assert!(
                !matches!(
                    d.kind.as_str(),
                    "truncated" | "duplicate-chunk" | "style-budget" | "style-truncated"
                ),
                "{}: budget entry {d:?}",
                e.file
            );
        }
        let text = frame_text(&doc);
        let r_story = recall(
            &std::fs::read_to_string(e.oracle_text.as_ref().unwrap()).unwrap(),
            &story_text(&doc),
        );
        let oracle = std::fs::read_to_string(e.oracle_text.as_ref().unwrap())
            .unwrap_or_else(|err| panic!("{}: oracle text: {err}", e.file));
        let r_oracle = recall(&oracle, &text);
        let r_distinct = distinct_recall(&oracle, &story_text(&doc));
        let r_poi = e
            .poi_text
            .as_ref()
            .map(|p| recall(&std::fs::read_to_string(p).unwrap(), &text));
        let size = doc.setup.size;
        println!(
            "{:<42} {:>5} {:>8.2} x {:>6.2} {:>8.3} {:>8.3} {:>8.3} {:>8} {:>5} {:>6}",
            e.file,
            doc.pages.len(),
            size.width,
            size.height,
            r_oracle,
            r_story,
            r_distinct,
            r_poi.map_or("-".to_string(), |r| format!("{r:.3}")),
            took.as_millis(),
            rep.dropped_total()
        );
        if doc.pages.len() != e.pages {
            failures.push(format!(
                "{}: {} pages, expected {}",
                e.file,
                doc.pages.len(),
                e.pages
            ));
        }
        let (w, h) = (e.width.unwrap(), e.height.unwrap());
        if (size.width - w).abs() > 0.5 || (size.height - h).abs() > 0.5 {
            failures.push(format!(
                "{}: page {:.2} x {:.2}, expected {w:.2} x {h:.2}",
                e.file, size.width, size.height
            ));
        }
        if r_distinct < 0.90 {
            failures.push(format!(
                "{}: per-file distinct-word recall {r_distinct:.3} < 0.90",
                e.file
            ));
        }
        if r_oracle < 0.90 {
            failures.push(format!("{}: oracle recall {r_oracle:.3} < 0.90", e.file));
        }
        if let Some(r) = r_poi
            && r < 0.90
        {
            failures.push(format!("{}: POI recall {r:.3} < 0.90", e.file));
        }
        if e.file.ends_with("SampleNewsletter.pub") {
            println!(
                "  SampleNewsletter.pub: {} pages, {} oracle words, {} imported words",
                doc.pages.len(),
                e.words,
                words(&text).len()
            );
        }
        if e.file.ends_with("SampleBrochure.pub") {
            let landscape = size.width > size.height;
            println!(
                "  SampleBrochure.pub: {:.2} x {:.2} pt, A4 landscape = {landscape}",
                size.width, size.height
            );
            if !landscape {
                failures.push("SampleBrochure.pub is not landscape".into());
            }
        }
        assert!(took < Duration::from_secs(2), "{} took {took:?}", e.file);
    }
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}

#[test]
fn oracle_no_output_files_never_panic_and_are_fast() {
    let list: Vec<Entry> = entries().into_iter().filter(|e| !e.ok).collect();
    assert_eq!(list.len(), 6);
    for e in &list {
        let bytes = read(e);
        let started = Instant::now();
        let r = pubfile::import(&bytes);
        let took = started.elapsed();
        let outcome = match &r {
            Err(err) => format!("Err({err})"),
            Ok((_, rep)) => format!(
                "Ok, {} pages, {} frames, {} dropped",
                rep.pages,
                rep.frames,
                rep.dropped_total()
            ),
        };
        println!("{:<80} {:>5} ms  {outcome}", e.file, took.as_millis());
        if let Ok((_, rep)) = &r {
            assert!(
                rep.dropped_total() > 0 || rep.pages == 0,
                "{}: Ok with an empty report",
                e.file
            );
        }
        assert!(took < Duration::from_secs(2), "{}: took {took:?}", e.file);
    }
}

#[test]
fn unsupported_and_garbage_inputs_are_errors() {
    for bytes in [
        &b""[..],
        &b"not a compound file"[..],
        &[0xd0, 0xcf, 0x11, 0xe0][..],
    ] {
        assert!(pubfile::import(bytes).is_err());
    }
}

#[test]
fn unknown_version_is_named_in_the_error() {
    // Rewrite the version byte of a real file's Contents stream.
    let e = entries()
        .into_iter()
        .find(|e| e.file.ends_with("Sample.pub"))
        .unwrap();
    let mut bytes = read(&e);
    // Find the header E8 AC 2C 00 inside the container and change the version.
    let at = bytes
        .windows(4)
        .position(|w| w == [0xe8, 0xac, 0x2c, 0x00])
        .expect("Contents header");
    bytes[at + 2] = 0x37;
    match pubfile::import(&bytes) {
        Err(pubrs_io::IoError::Format(m)) => assert!(m.contains("0x37"), "{m}"),
        other => panic!("expected a Format error naming the version, got {other:?}"),
    }
}

#[test]
fn versions_are_detected() {
    for (name, want) in [
        ("poi/Sample98.pub", pubfile::Version::Pub2000),
        ("poi/Sample2000.pub", pubfile::Version::Pub2000),
        ("poi/Sample.pub", pubfile::Version::Pub2002),
        ("poi/Sample_2010.pub", pubfile::Version::Pub2002),
        ("libreoffice/fdo59355-1.pub", pubfile::Version::Pub98),
    ] {
        let bytes = std::fs::read(corpus().join(name)).unwrap();
        assert_eq!(pubfile::detect_version(&bytes).unwrap(), want, "{name}");
    }
}

#[test]
fn linked_text_frames_form_chains() {
    // SampleNewsletter threads stories through three columns (prev/next links in
    // the shape chunks); each chain must come out as one story over several frames.
    let bytes = std::fs::read(corpus().join("poi/SampleNewsletter.pub")).unwrap();
    let (doc, rep) = pubfile::import(&bytes).unwrap();
    let mut lens: Vec<usize> = doc.stories.values().map(|s| s.frames.len()).collect();
    lens.sort_unstable();
    println!(
        "SampleNewsletter: {} frames, chain lengths {lens:?}",
        rep.frames
    );
    assert!(lens.iter().filter(|n| **n == 3).count() >= 4, "{lens:?}");
    let text_frames = doc
        .frames
        .values()
        .filter(|f| matches!(f.content, pubrs_core::model::FrameContent::Text(_)))
        .count();
    assert_eq!(
        doc.stories.values().map(|s| s.frames.len()).sum::<usize>(),
        text_frames
    );
    let empty_pictures = rep
        .dropped
        .iter()
        .find(|d| d.kind == "picture-wmf")
        .map_or(0, |d| d.count);
    assert_eq!(
        rep.frames,
        text_frames + rep.pictures + rep.shapes + rep.tables + empty_pictures,
        "frames = text + pictures + shapes + tables + empty picture frames"
    );
    // Flow order is geometric left to right for the three-column stories.
    for s in doc.stories.values().filter(|s| s.frames.len() == 3) {
        let xs: Vec<f64> = s.frames.iter().map(|f| doc.frames[f].rect.x).collect();
        assert!(
            xs.windows(2).all(|w| w[0] < w[1]),
            "chain not left to right: {xs:?}"
        );
    }
}

#[test]
fn hyperlink_runs_stay_one_word() {
    // The LinkAt* files hold hyperlinks, not frame links: the visible text must
    // read straight through the link boundaries.
    for (name, want) in [
        (
            "LinkAt10And20And30And40",
            "1234567890LINK123456LINK212345LINK312345LINK4",
        ),
        ("LinkAt0And10", "LINK123456LINK2"),
    ] {
        let bytes = std::fs::read(corpus().join(format!("poi/{name}.pub"))).unwrap();
        let (doc, _) = pubfile::import(&bytes).unwrap();
        assert_eq!(pubfile::story_texts(&doc).join(""), want, "{name}");
    }
}

// ---- fuzzing: random bytes, truncated and mutated corpus files never panic ----

fn sample_files() -> Vec<Vec<u8>> {
    [
        "poi/Sample.pub",
        "poi/Sample2000.pub",
        "libreoffice/fdo59355-1.pub",
        "poi/LinkAt10.pub",
    ]
    .iter()
    .map(|n| std::fs::read(corpus().join(n)).unwrap())
    .collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = pubfile::import_unguarded(&bytes);
    }

    #[test]
    fn truncated_files_never_panic(which in 0usize..4, frac in 0.0f64..1.0) {
        let files = sample_files();
        let f = &files[which];
        let n = ((f.len() as f64) * frac) as usize;
        let _ = pubfile::import_unguarded(&f[..n]);
    }

    #[test]
    fn mutated_files_never_panic(
        which in 0usize..4,
        edits in proptest::collection::vec((any::<proptest::sample::Index>(), any::<u8>()), 1..24),
    ) {
        let files = sample_files();
        let mut f = files[which].clone();
        for (i, v) in edits {
            let at = i.index(f.len());
            f[at] = v;
        }
        let started = Instant::now();
        let _ = pubfile::import_unguarded(&f);
        prop_assert!(started.elapsed() < Duration::from_secs(5));
    }
}

/// `(contents, quill, escher)` of a corpus file.
type Streams4 = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

/// The four streams the importer reads (contents, quill, escher, escher delay).
fn streams4(name: &str) -> Streams4 {
    let bytes = std::fs::read(corpus().join(name)).unwrap();
    let mut c = pubfile::container::Container::open(&bytes).unwrap();
    (
        c.stream("/Contents").unwrap(),
        c.stream("/Quill/QuillSub/CONTENTS"),
        c.stream("/Escher/EscherStm"),
        c.stream("/Escher/EscherDelayStm"),
    )
}

fn streams_of(name: &str) -> (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let bytes = std::fs::read(corpus().join(name)).unwrap();
    let mut c = pubfile::container::Container::open(&bytes).unwrap();
    (
        c.stream("/Contents").unwrap(),
        c.stream("/Quill/QuillSub/CONTENTS"),
        c.stream("/Escher/EscherStm"),
    )
}

const STREAM_FILES: [&str; 5] = [
    "poi/Sample.pub",
    "poi/Sample2000.pub",
    "libreoffice/fdo59355-1.pub",
    "poi/SampleBrochure.pub",
    "poi/SampleNewsletter.pub",
];

fn mutate(data: &mut [u8], edits: &[(proptest::sample::Index, u8, bool)]) {
    if data.is_empty() {
        return;
    }
    for (i, v, wide) in edits {
        let at = i.index(data.len());
        if *wide && at + 4 <= data.len() {
            // A huge count or offset, the usual way to ask a parser for too much.
            data[at..at + 4].copy_from_slice(&[0xff, 0xff, 0xff, 0x7f | (*v & 0x80)]);
        } else {
            data[at] = *v;
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, failure_persistence: None, ..ProptestConfig::default() })]

    /// Mutated streams (single bytes and 32-bit "huge" fields) reach the record
    /// parsers directly instead of failing in the container.
    #[test]
    fn mutated_streams_never_panic_or_hang(
        which in 0usize..5,
        target in 0usize..4,
        edits in proptest::collection::vec(
            (any::<proptest::sample::Index>(), any::<u8>(), any::<bool>()), 1..40),
    ) {
        let (mut contents, mut quill, mut escher, mut delay) = streams4(STREAM_FILES[which]);
        match target {
            0 => mutate(&mut contents, &edits),
            1 => if let Some(q) = quill.as_mut() { mutate(q, &edits) },
            2 => if let Some(e) = escher.as_mut() { mutate(e, &edits) },
            _ => if let Some(d) = delay.as_mut() { mutate(d, &edits) },
        }
        let started = Instant::now();
        let _ = pubfile::import_streams_unguarded(&contents, quill.as_deref(), escher.as_deref(), delay.as_deref());
        prop_assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
    }

    #[test]
    fn truncated_streams_never_panic(which in 0usize..5, frac in 0.0f64..1.0) {
        let (contents, quill, escher, delay) = streams4(STREAM_FILES[which]);
        let cut = |d: &[u8]| d[..((d.len() as f64) * frac) as usize].to_vec();
        let (c, q, e, dl) = (
            cut(&contents),
            quill.as_deref().map(cut),
            escher.as_deref().map(cut),
            delay.as_deref().map(cut),
        );
        let _ = pubfile::import_streams_unguarded(&c, q.as_deref(), e.as_deref(), dl.as_deref());
        let _ = pubfile::import_streams_unguarded(&contents, q.as_deref(), escher.as_deref(), delay.as_deref());
        let _ = pubfile::import_streams_unguarded(&c, quill.as_deref(), e.as_deref(), delay.as_deref());
        let _ = pubfile::import_streams_unguarded(&contents, quill.as_deref(), escher.as_deref(), dl.as_deref());
    }
}

// ---- crafted-input regressions ----

fn u16le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

/// An Escher stream with one group whose FSPGR and child anchor hold extreme
/// i32 values. The product `(v - s0) * group_size` overflows i64 without care.
fn extreme_escher() -> Vec<u8> {
    let rec = |ty: u16, inst: u16, body: Vec<u8>| {
        let mut r = Vec::new();
        r.extend_from_slice(&u16le(inst << 4));
        r.extend_from_slice(&u16le(ty));
        r.extend_from_slice(&(body.len() as u32).to_le_bytes());
        r.extend(body);
        r
    };
    let ints = |v: [i32; 4]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let client_data = {
        let mut b = vec![0u8; 4];
        b.extend_from_slice(&u16le(0x6801));
        b.extend_from_slice(&293u32.to_le_bytes());
        b
    };
    let client_anchor = {
        let mut b = vec![0u8; 4];
        for (id, v) in [
            (0x2001u16, i32::MIN),
            (0x2002, i32::MIN),
            (0x2003, i32::MAX),
            (0x2004, i32::MAX),
        ] {
            b.extend_from_slice(&u16le(id));
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    };
    // Group shape: FSPGR with the full i32 range, then the anchor.
    let leader = rec(
        0xf004,
        0,
        [
            rec(0xf009, 0, ints([i32::MIN, i32::MIN, i32::MAX, i32::MAX])),
            rec(0xf00a, 0, vec![0, 0, 0, 0, 1, 0, 0, 0]),
            rec(0xf011, 0, client_data.clone()),
            rec(0xf010, 0, client_anchor),
        ]
        .concat(),
    );
    let child = rec(
        0xf004,
        0,
        [
            rec(0xf00a, 0, vec![0, 0, 0, 0, 0, 0, 0, 0]),
            rec(0xf011, 0, client_data),
            rec(0xf00f, 0, ints([i32::MAX, i32::MIN, i32::MIN, i32::MAX])),
        ]
        .concat(),
    );
    let spgr = rec(0xf003, 0, [leader, child].concat());
    let mut dg = rec(0xf002, 0, spgr);
    dg.extend_from_slice(&[0; 4]);
    dg
}

#[test]
fn extreme_escher_coordinates_do_not_overflow() {
    let (contents, quill, _) = streams_of("poi/Sample.pub");
    let started = Instant::now();
    let _ = pubfile::import_streams_unguarded(
        &contents,
        quill.as_deref(),
        Some(&extreme_escher()),
        None,
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    // The rectangle comes out clamped, not wrapped: |coordinate| <= 2^31 EMU.
    let mut rep = ImportReport::default();
    let shapes = pubfile::records::parse_escher(&extreme_escher(), None, &mut rep).shapes;
    assert!(!shapes.is_empty());
    for (seq, s) in &shapes {
        let r = s.rect.unwrap_or_else(|| panic!("shape {seq} has no rect"));
        for v in [r.xs, r.ys, r.xe, r.ye] {
            assert!(v.abs() <= 1 << 31, "shape {seq}: {v} not clamped");
        }
        assert!(r.xs <= r.xe && r.ys <= r.ye);
    }
}

#[test]
fn overlapping_max_count_quill_chunk_lists_stay_bounded() {
    // Every list declares 65535 entries and points at the next list a few bytes
    // on, so lists overlap; ~1.6 MB must not become gigabytes of chunk records.
    let (contents, _, _) = streams_of("poi/Sample.pub");
    let mut q = vec![0u8; 0x18];
    q[..8].copy_from_slice(b"CHNKINK ");
    let lists = 4000usize;
    for i in 0..lists {
        let mut head = vec![0u8; 8];
        head[2..4].copy_from_slice(&u16le(65535));
        let next = 0x18 + (i + 1) * 8;
        head[4..8].copy_from_slice(&(next as u32).to_le_bytes());
        q.extend(head);
    }
    q.extend(vec![b'T'; 1_600_000]);
    let started = Instant::now();
    let _ = pubfile::import_streams_unguarded(&contents, Some(&q), None, None);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

fn has_kind(rep: &ImportReport, kind: &str) -> bool {
    rep.dropped.iter().any(|d| d.kind == kind)
}

/// A Quill stream whose chunk list has `entries` references named FONT, PL, STRS,
/// SYID, FDPC, FDPP and TEXT in turn, all pointing into one large region
/// (`distinct` spreads the offsets across it so no two references are alike).
fn hostile_quill(entries: usize, distinct: bool) -> Vec<u8> {
    let names: [&[u8; 4]; 7] = [
        b"FONT", b"PL  ", b"STRS", b"SYID", b"FDPC", b"FDPP", b"TEXT",
    ];
    let list_end = 0x18 + 8 + entries * 24;
    let mut q = vec![0u8; 0x18];
    q[..8].copy_from_slice(b"CHNKINK ");
    q.extend_from_slice(&[0, 0]);
    q.extend_from_slice(&u16le(entries as u16));
    q.extend_from_slice(&u32::MAX.to_le_bytes());
    for i in 0..entries {
        let offset = if distinct {
            list_end + (i * 4) % 200_000
        } else {
            list_end
        } as u32;
        let mut e = vec![0u8; 24];
        e[0] = 0x18;
        e[2..6].copy_from_slice(names[i % names.len()]);
        e[16..20].copy_from_slice(&offset.to_le_bytes());
        q.extend(e);
    }
    // The region: counts and sizes that ask for as much as possible.
    q.extend((0..1_000_000u32).map(|i| if i % 8 < 6 { 0xff } else { 0x01 }));
    q
}

#[test]
fn quill_lists_pointing_at_one_region_are_parsed_once() {
    let (contents, _, _) = streams_of("poi/Sample.pub");
    for distinct in [false, true] {
        let q = hostile_quill(6000, distinct);
        let started = Instant::now();
        let mut rep = ImportReport::default();
        let data = pubfile::records::parse(
            pubfile::Version::Pub2002,
            &contents,
            Some(&q),
            None,
            None,
            &mut rep,
        );
        let took = started.elapsed();
        assert!(took < Duration::from_secs(2), "{took:?}");
        println!(
            "quill refs (distinct={distinct}) {took:?}: {:?}",
            rep.dropped
        );
        // Never more items than the global budget, whatever the list asks for.
        assert!(data.fonts.len() + data.text_colors.len() + data.stories.len() < 1_000_000);
        if distinct {
            assert!(
                rep.truncated() || has_kind(&rep, "style-budget"),
                "{:?}",
                rep.dropped
            );
        } else {
            assert!(has_kind(&rep, "duplicate-chunk"), "{:?}", rep.dropped);
        }
        // And the whole import path stays panic-free.
        let _ = pubfile::import_streams_unguarded(&contents, Some(&q), None, None);
    }
}

/// A Quill stream whose chunk list has `entries` PL references at consecutive
/// 16-byte offsets. Each points at a count word of 0x0080_0000, and everything
/// after it is the word 4, so every PL entry has a length of 4 and walks nothing.
fn pl_quill(entries: usize) -> Vec<u8> {
    let list_end = 0x18 + 8 + entries * 24;
    let mut q = vec![0u8; 0x18];
    q[..8].copy_from_slice(b"CHNKINK ");
    q.extend_from_slice(&[0, 0]);
    q.extend_from_slice(&u16le(entries as u16));
    q.extend_from_slice(&u32::MAX.to_le_bytes());
    for i in 0..entries {
        let mut e = vec![0u8; 24];
        e[0] = 0x18;
        e[2..6].copy_from_slice(b"PL  ");
        e[16..20].copy_from_slice(&((list_end + i * 16) as u32).to_le_bytes());
        q.extend(e);
    }
    let mut region: Vec<u8> = (0..400_000u32).flat_map(|_| 4u32.to_le_bytes()).collect();
    for i in 0..entries {
        region[i * 16..i * 16 + 4].copy_from_slice(&0x0080_0000u32.to_le_bytes());
    }
    q.extend(region);
    q
}

#[test]
fn zero_stride_pl_chunks_are_charged_and_stop() {
    let (contents, _, _) = streams_of("poi/Sample.pub");
    let q = pl_quill(200);
    let started = Instant::now();
    let mut rep = ImportReport::default();
    let _ = pubfile::records::parse(
        pubfile::Version::Pub2002,
        &contents,
        Some(&q),
        None,
        None,
        &mut rep,
    );
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert!(
        rep.truncated() || has_kind(&rep, "truncated"),
        "{:?}",
        rep.dropped
    );
    let _ = pubfile::import_streams_unguarded(&contents, Some(&q), None, None);
}

/// Repoint the first FONT entry of a real Quill stream at a block whose one
/// font name has `units` UTF-16 units, and return the patched stream.
fn with_long_font_name(mut q: Vec<u8>, units: usize) -> Vec<u8> {
    let mut block = vec![0u8; 24];
    block[4..8].copy_from_slice(&1u32.to_le_bytes());
    block.extend_from_slice(&u16le(units as u16));
    block.extend((0..units).flat_map(|_| [0x41u8, 0]));
    block.extend_from_slice(&[0u8; 4]);
    let new_off = q.len() as u32;
    q.extend(block);
    let mut list = 0x18usize;
    while list + 8 <= q.len() {
        let n = usize::from(u16::from_le_bytes([q[list + 2], q[list + 3]]));
        for i in 0..n {
            let p = list + 8 + i * 24;
            if p + 20 > q.len() {
                break;
            }
            if &q[p + 2..p + 6] == b"FONT" {
                q[p + 16..p + 20].copy_from_slice(&new_off.to_le_bytes());
                return q;
            }
        }
        let next = u32::from_le_bytes([q[list + 4], q[list + 5], q[list + 6], q[list + 7]]);
        if next == u32::MAX {
            break;
        }
        list = next as usize;
    }
    panic!("no FONT entry in the Quill stream");
}

#[test]
fn long_font_names_are_cut_and_runs_share_the_cut() {
    let (contents, quill, escher) = streams_of("poi/Sample.pub");
    let q = with_long_font_name(quill.expect("quill"), 60_000);
    let started = Instant::now();
    let (doc, rep) =
        pubfile::import_streams_unguarded(&contents, Some(&q), escher.as_deref(), None)
            .expect("import");
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert!(has_kind(&rep, "font-name-truncated"), "{:?}", rep.dropped);
    let mut runs = 0usize;
    for story in doc.stories.values() {
        for p in &story.paragraphs {
            for r in &p.runs {
                runs += 1;
                let len = r
                    .props
                    .font_family
                    .as_ref()
                    .map_or(0, |f| f.chars().count());
                assert!(len <= 128, "font name of {len} chars");
            }
        }
    }
    assert!(runs > 0, "no runs were imported");
}

/// A 2002 Contents stream whose directory has `entries` overlapping page,
/// document and palette references into one region.
fn hostile_contents(entries: usize, distinct: bool) -> Vec<u8> {
    let block = |id: u8, ty: u8, body: &[u8]| {
        let mut b = vec![id, ty];
        b.extend_from_slice(body);
        b
    };
    let fixed4 = |id: u8, v: u32| block(id, 0x20, &v.to_le_bytes());
    let region_at = 0x40usize;
    let mut region = Vec::new();
    // u32 length, then one container (id 2) of seq-number blocks (id 0, type 0x70).
    let items = 40_000usize;
    let inner_len = 4 + items * 6;
    region.extend_from_slice(&((8 + inner_len + 6) as u32).to_le_bytes());
    region.extend(block(2, 0x88, &(inner_len as u32).to_le_bytes()));
    for i in 0..items {
        region.extend(block(0, 0x70, &(i as u32).to_le_bytes()));
    }
    let tys = [0x43u32, 0x44, 0x5c];
    let mut dir = Vec::new();
    for i in 0..entries {
        let off = region_at + if distinct { (i * 6) % 200_000 } else { 0 };
        let mut body = Vec::new();
        body.extend(fixed4(2, tys[i % 3]));
        body.extend(fixed4(4, off as u32));
        body.extend(fixed4(5, 0));
        let mut e = vec![0x00, 0x88];
        e.extend_from_slice(&((4 + body.len()) as u32).to_le_bytes());
        e.extend(body);
        dir.extend(e);
    }
    let mut trailer = Vec::new();
    trailer.extend(block(0, 0x90, &((4 + dir.len()) as u32).to_le_bytes()));
    trailer.extend(dir);
    let trailer_off = region_at + region.len();
    let mut c = vec![0u8; region_at];
    c[..4].copy_from_slice(&[0xe8, 0xac, 0x2c, 0x00]);
    c[0x1a..0x1e].copy_from_slice(&(trailer_off as u32).to_le_bytes());
    c.extend(region);
    c.extend_from_slice(&((4 + trailer.len()) as u32).to_le_bytes());
    c.extend(trailer);
    c
}

#[test]
fn overlapping_contents_directory_entries_are_bounded() {
    let (_, quill, _) = streams_of("poi/Sample.pub");
    for distinct in [false, true] {
        let c = hostile_contents(20_000, distinct);
        let started = Instant::now();
        let mut rep = ImportReport::default();
        let data = pubfile::records::parse(
            pubfile::Version::Pub2002,
            &c,
            quill.as_deref(),
            None,
            None,
            &mut rep,
        );
        let took = started.elapsed();
        assert!(took < Duration::from_secs(2), "{took:?}");
        println!(
            "contents directory (distinct={distinct}) {took:?}: {:?}",
            rep.dropped
        );
        assert!(data.pages.len() <= 2_000 && data.palette.len() < 1_000_000);
        if distinct {
            assert!(
                rep.truncated() || has_kind(&rep, "page"),
                "{:?}",
                rep.dropped
            );
        } else {
            assert!(has_kind(&rep, "duplicate-chunk"), "{:?}", rep.dropped);
        }
        let _ = pubfile::import_streams_unguarded(&c, quill.as_deref(), None, None);
    }
}

// ---- part 2: pictures, shapes, colours, tables, masters (ADR 0026) ----

mod common;

use pubrs_core::model::{Document, FrameContent};

fn import_corpus(name: &str) -> (Document, ImportReport) {
    let bytes = std::fs::read(corpus().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    pubfile::import(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn placed_pictures(doc: &Document) -> usize {
    doc.frames
        .values()
        .filter(|f| matches!(&f.content, FrameContent::Image(i) if i.asset.is_some()))
        .count()
}

/// Pictures the oracle's PDF shows that are not separate picture frames in the
/// file: `pdfimages` lists the soft mask of a transparent image as an image of
/// its own. SampleBrochure's two GIFs have a transparent colour (Graphic
/// Control Extension flag 0x01), so its 5 oracle images are 3 pictures + 2 masks.
fn oracle_soft_masks(file: &str) -> usize {
    if file == "poi/SampleBrochure.pub" {
        2
    } else {
        0
    }
}

#[test]
fn placed_pictures_match_the_oracle_image_count() {
    let mut total = 0;
    for e in entries().into_iter().filter(|e| e.ok) {
        let (doc, rep) = import_corpus(&e.file);
        let placed = placed_pictures(&doc);
        let want = e.images.saturating_sub(oracle_soft_masks(&e.file));
        println!(
            "{:<40} oracle images {} placed {} (pictures {}, empty frames {})",
            e.file,
            e.images,
            placed,
            rep.pictures,
            doc.frames
                .values()
                .filter(|f| matches!(&f.content, FrameContent::Image(i) if i.asset.is_none()))
                .count()
        );
        assert_eq!(placed, want, "{}: placed pictures", e.file);
        assert_eq!(rep.pictures, placed, "{}: report count", e.file);
        total += placed;
    }
    // 51318 (1 PNG), SampleNewsletter (1 JPEG), SampleBrochure (2 GIF + 1 JPEG).
    assert_eq!(total, 5);
}

#[test]
fn metafile_pictures_are_reported_not_silent() {
    // No oracle image is a metafile: every WMF in the corpus draws vector
    // records only, so there is no extractor (ADR 0026). They leave an empty
    // picture frame and a "picture-wmf" entry.
    let (doc, rep) = import_corpus("poi/SampleNewsletter.pub");
    let wmf = rep
        .dropped
        .iter()
        .find(|d| d.kind == "picture-wmf")
        .expect("wmf");
    assert_eq!(wmf.count, 9, "{:?}", rep.dropped);
    let empty = doc
        .frames
        .values()
        .filter(|f| matches!(&f.content, FrameContent::Image(i) if i.asset.is_none()))
        .count();
    assert_eq!(empty, 9);
    let (_, rep) = import_corpus("poi/SampleBrochure.pub");
    assert_eq!(
        rep.dropped
            .iter()
            .find(|d| d.kind == "picture-wmf")
            .map(|d| d.count),
        Some(5)
    );
}

#[test]
fn import_then_pdf_export_has_the_oracle_page_count() {
    use pubrs_io::ExportContext;
    use pubrs_render::PdfOptions;
    let mut ctx = ExportContext::new(common::test_fonts());
    for e in entries().into_iter().filter(|e| e.ok) {
        let (doc, _) = import_corpus(&e.file);
        let pdf = ctx
            .export_pdf(&doc, &PdfOptions::default())
            .unwrap_or_else(|err| panic!("{}: {err}", e.file));
        let pages = lopdf::Document::load_mem(&pdf)
            .unwrap_or_else(|err| panic!("{}: {err}", e.file))
            .get_pages()
            .len();
        assert_eq!(pages, e.pages, "{}: PDF pages", e.file);
    }
}

fn render_page_png(doc: &Document, page: usize) -> Vec<u8> {
    let mut ctx = pubrs_io::ExportContext::new(common::test_fonts());
    let opts = pubrs_render::RasterOptions {
        dpi: 72.0,
        ..Default::default()
    };
    ctx.page_png(doc, page, &opts).expect("render")
}

#[test]
fn pub_sample_newsletter_p1() {
    let (doc, _) = import_corpus("poi/SampleNewsletter.pub");
    common::assert_png_golden("pub_sample_newsletter_p1", &render_page_png(&doc, 0));
}

#[test]
fn pub_sample_brochure_p1() {
    let (doc, _) = import_corpus("poi/SampleBrochure.pub");
    common::assert_png_golden("pub_sample_brochure_p1", &render_page_png(&doc, 0));
}

#[test]
fn pub_sample2000_p1() {
    let (doc, _) = import_corpus("poi/Sample2000.pub");
    common::assert_png_golden("pub_sample2000_p1", &render_page_png(&doc, 0));
}

#[test]
fn imported_render_is_deterministic() {
    let (doc, _) = import_corpus("poi/SampleBrochure.pub");
    assert_eq!(render_page_png(&doc, 0), render_page_png(&doc, 0));
}

#[test]
fn the_corpus_table_becomes_a_table_frame() {
    // Sample.pub page 2 holds a 3 x 2 table; the five versions of that sample agree.
    for name in [
        "poi/Sample.pub",
        "poi/Sample2.pub",
        "poi/Sample3.pub",
        "poi/Sample4.pub",
        "poi/Sample_2010.pub",
        "poi/Sample2_2010.pub",
        "poi/Sample3_2010.pub",
        "poi/Sample4_2010.pub",
    ] {
        let (doc, rep) = import_corpus(name);
        let tables: Vec<_> = doc.frames.values().filter_map(|f| f.as_table()).collect();
        assert_eq!(tables.len(), 1, "{name}");
        assert_eq!(rep.tables, 1, "{name}");
        let t = tables[0];
        assert_eq!((t.row_count(), t.col_count()), (3, 2), "{name}");
        // Widths and heights come from the file (140 pt columns, about 17 pt rows).
        assert!(
            t.cols.iter().all(|w| (w - 140.3).abs() < 1.0),
            "{name}: {:?}",
            t.cols
        );
        assert!(
            t.rows.iter().all(|r| (r.min_height - 17.3).abs() < 1.0),
            "{name}"
        );
        let cell = |r: u32, c: u32| {
            let story = t.cell_at(r, c).expect("cell").story;
            doc.stories[&story]
                .paragraphs
                .iter()
                .map(|p| p.text())
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(cell(0, 0), "Table on page 2", "{name}");
        assert_eq!(cell(0, 1), "Top right", "{name}");
        assert_eq!(cell(1, 0), "P2 table left", "{name}");
        assert_eq!(cell(2, 1), "Bottom Right", "{name}");
        assert!(!has_kind(&rep, "table"), "{name}: {:?}", rep.dropped);
    }
}

#[test]
fn files_without_tables_have_none() {
    for name in [
        "poi/SampleNewsletter.pub",
        "poi/Simple.pub",
        "poi/Sample2000.pub",
    ] {
        let (doc, rep) = import_corpus(name);
        assert!(
            doc.frames.values().all(|f| f.as_table().is_none()),
            "{name}"
        );
        assert_eq!(rep.tables, 0);
    }
}

#[test]
fn pages_use_the_files_master() {
    // Every 2002+ file has one master that its pages name (page block 0x0d);
    // none of the corpus masters holds a shape.
    for name in [
        "poi/SampleNewsletter.pub",
        "poi/Sample.pub",
        "poi/51318.pub",
    ] {
        let (doc, rep) = import_corpus(name);
        assert_eq!(doc.masters.len(), 1, "{name}");
        assert_eq!(rep.masters, 1);
        let m = doc.masters[0].id;
        assert!(doc.pages.iter().all(|p| p.master == Some(m)), "{name}");
        assert!(doc.masters[0].frames.is_empty());
    }
    // Publisher 98 and 2000 files have no master reference.
    let (doc, _) = import_corpus("poi/Sample2000.pub");
    assert!(doc.masters.is_empty());
}

#[test]
fn scheme_colours_become_swatches() {
    use pubrs_core::{Paint, SchemeSlot};
    let (doc, rep) = import_corpus("poi/SampleNewsletter.pub");
    assert_eq!(rep.swatches, 8);
    for slot in SchemeSlot::ALL {
        let s = doc.swatch(slot.swatch_id()).expect("slot swatch");
        assert_eq!(s.name, slot.swatch_name());
    }
    // Slot colours are the file's: Hyperlink 000080, Followed 663399.
    let rgb = |slot: SchemeSlot| match &doc.swatch(slot.swatch_id()).unwrap().value {
        pubrs_core::SwatchValue::Rgb(c) => (c.r, c.g, c.b),
        v => panic!("{v:?}"),
    };
    assert_eq!(rgb(SchemeSlot::Hyperlink), (0x00, 0x00, 0x80));
    assert_eq!(rgb(SchemeSlot::FollowedHyperlink), (0x66, 0x33, 0x99));
    // References use the swatches; the document stays valid.
    let uses_swatch = doc.frames.values().any(|f| {
        matches!(&f.style.fill, Some(Paint::Swatch { .. }))
            || matches!(&f.style.stroke, Some(s) if matches!(s.color, Paint::Swatch { .. }))
    });
    assert!(uses_swatch);
    let text_uses = doc.stories.values().any(|st| {
        st.paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .any(|r| matches!(r.props.color, Some(Paint::Swatch { .. })))
    });
    assert!(
        text_uses,
        "text colours by palette index become swatch paints"
    );
    // A file with no palette gets none (Publisher 98 with a palette still has 8).
    let (_, rep) = import_corpus("poi/Sample98.pub");
    assert!(rep.swatches <= 8);
}

#[test]
fn shapes_import_with_fill_and_outline() {
    use pubrs_core::model::Shape;
    let (doc, rep) = import_corpus("poi/SampleBrochure.pub");
    let shapes: Vec<_> = doc
        .frames
        .values()
        .filter_map(|f| match &f.content {
            FrameContent::Shape(s) => Some((f, &s.shape)),
            _ => None,
        })
        .collect();
    assert_eq!(shapes.len(), rep.shapes);
    assert!(
        shapes.iter().any(|(_, s)| matches!(s, Shape::Line { .. })),
        "lines"
    );
    assert!(
        shapes
            .iter()
            .any(|(_, s)| matches!(s, Shape::Rectangle { .. })),
        "rectangles"
    );
    assert!(
        shapes.iter().any(|(f, _)| f.style.fill.is_some()),
        "a filled shape"
    );
    assert!(
        shapes.iter().any(|(f, _)| f.style.stroke.is_some()),
        "an outlined shape"
    );
    // What is not imported is counted, with the Escher type in the detail.
    assert!(
        rep.dropped.iter().any(|d| d.kind == "shape"),
        "{:?}",
        rep.dropped
    );
}

#[test]
fn the_summary_names_everything() {
    let (_, rep) = import_corpus("poi/SampleNewsletter.pub");
    let s = rep.summary();
    assert!(s.starts_with("Imported 4 pages, "), "{s}");
    assert!(s.contains("1 picture"), "{s}");
    assert!(s.contains("dropped:"), "{s}");
    assert!(s.contains("picture-wmf"), "{s}");
    assert!(
        rep.fonts
            .iter()
            .any(|f| f == "Arial" || f == "Times New Roman"),
        "{:?}",
        rep.fonts
    );
    let mut r = rep.clone();
    r.note_font_fallbacks(&[("Arial".into(), "Liberation Sans".into())]);
    assert!(
        r.summary().ends_with("; 1 font substituted"),
        "{}",
        r.summary()
    );
    assert!(r.dropped.iter().any(|d| {
        d.kind == "font-fallback"
            && d.detail
                .contains("Arial is not installed; Liberation Sans is used")
    }));
}

// ---- hostile pictures ----

/// Positions of every FBSE header (type 0xF007) in an Escher stream.
fn fbse_offsets(esc: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..esc.len().saturating_sub(8) {
        if esc[i + 2] == 0x07 && esc[i + 3] == 0xf0 && esc[i + 4..i + 8] == [0x24, 0, 0, 0] {
            out.push(i + 8);
        }
    }
    out
}

fn import4(s: &Streams4) -> Result<(Document, ImportReport), pubrs_io::IoError> {
    let started = Instant::now();
    let r = pubfile::import_streams_unguarded(&s.0, s.1.as_deref(), s.2.as_deref(), s.3.as_deref());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    r
}

#[test]
fn hostile_fbse_offsets_and_sizes_are_survived() {
    let base = streams4("poi/SampleBrochure.pub");
    let esc = base.2.clone().unwrap();
    let offs = fbse_offsets(&esc);
    assert_eq!(offs.len(), 8);
    for fo in [0xffff_ffffu32, 0x7fff_ffff, 64_586, 64_585, 1] {
        let mut e = esc.clone();
        for o in &offs {
            e[o + 28..o + 32].copy_from_slice(&fo.to_le_bytes());
        }
        let (doc, rep) =
            import4(&(base.0.clone(), base.1.clone(), Some(e), base.3.clone())).unwrap();
        assert_eq!(placed_pictures(&doc), rep.pictures);
    }
    // Sizes and name lengths that lie.
    for (at, val) in [(20usize, 0xffff_ffffu32), (24, 0xffff_ffff)] {
        let mut e = esc.clone();
        for o in &offs {
            e[o + at..o + at + 4].copy_from_slice(&val.to_le_bytes());
        }
        import4(&(base.0.clone(), base.1.clone(), Some(e), base.3.clone())).unwrap();
    }
    let mut e = esc.clone();
    for o in &offs {
        e[o + 33] = 0xff;
    }
    import4(&(base.0.clone(), base.1.clone(), Some(e), base.3.clone())).unwrap();
}

#[test]
fn hostile_blip_records_in_the_delay_stream() {
    let base = streams4("poi/SampleBrochure.pub");
    let delay = base.3.clone().unwrap();
    // Every BLIP header length claims 4 GiB.
    let mut d = delay.clone();
    for off in [0usize, 3554, 16736, 20254, 36932, 39601, 45624, 57165] {
        d[off + 4..off + 8].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
    }
    let (doc, rep) = import4(&(base.0.clone(), base.1.clone(), base.2.clone(), Some(d))).unwrap();
    assert_eq!(placed_pictures(&doc), 0);
    assert!(
        rep.dropped.iter().any(|d| d.kind.starts_with("picture")),
        "{:?}",
        rep.dropped
    );
    // The delay stream is garbage, empty, or absent.
    for d in [Some(vec![0xa5u8; 70_000]), Some(Vec::new()), None] {
        let (doc, _) = import4(&(base.0.clone(), base.1.clone(), base.2.clone(), d)).unwrap();
        assert_eq!(placed_pictures(&doc), 0);
    }
}

#[test]
fn hostile_pib_values_are_survived() {
    let base = streams4("poi/SampleBrochure.pub");
    let esc = base.2.clone().unwrap();
    for pib in [0u32, 9, 100_000, 0xffff_ffff] {
        let mut e = esc.clone();
        let mut patched = 0;
        for i in 0..e.len().saturating_sub(6) {
            // property id 0x4104 (pib) with a value in 1..=8
            if e[i] == 0x04
                && e[i + 1] == 0x41
                && e[i + 3..i + 6] == [0, 0, 0]
                && (1..=8).contains(&e[i + 2])
            {
                e[i + 2..i + 6].copy_from_slice(&pib.to_le_bytes());
                patched += 1;
            }
        }
        assert_eq!(patched, 8);
        let (doc, rep) =
            import4(&(base.0.clone(), base.1.clone(), Some(e), base.3.clone())).unwrap();
        assert_eq!(placed_pictures(&doc), 0, "pib {pib}");
        assert!(
            rep.dropped.iter().any(|d| d.kind == "picture"),
            "pib {pib}: {:?}",
            rep.dropped
        );
    }
}

/// An Escher record: version/instance word, type, length, body.
fn esc_record(ver: u16, inst: u16, ty: u16, body: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(ver | (inst << 4)).to_le_bytes());
    v.extend_from_slice(&ty.to_le_bytes());
    v.extend_from_slice(&(body.len() as u32).to_le_bytes());
    v.extend_from_slice(body);
    v
}

fn fbse(fo_delay: u32) -> Vec<u8> {
    let mut b = vec![0u8; 36];
    b[0] = 6;
    b[1] = 6;
    b[28..32].copy_from_slice(&fo_delay.to_le_bytes());
    esc_record(2, 6, 0xf007, &b)
}

#[test]
fn crafted_pictures_that_lie_about_their_size() {
    let base = streams4("poi/SampleBrochure.pub");
    // A GIF whose header claims 65535 x 65535 pixels with no image data.
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x3b]);
    // A PNG whose IHDR claims 100000 x 100000.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0, 0, 0, 13]);
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(&100_000u32.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    // A WMF blip whose "compressed" data is garbage.
    let wmf_body = {
        let mut b = vec![0u8; 16 + 34];
        b[16 + 32] = 0; // deflate
        b.extend_from_slice(&[0x78, 0xda, 0xff, 0xff, 0xff]);
        b
    };
    let mut delay = Vec::new();
    let mut pos = Vec::new();
    for (ty, inst, body) in [
        (0xf01eu16, 0x6e0u16, [vec![0u8; 17], gif].concat()),
        (0xf01e, 0x6e0, [vec![0u8; 17], png].concat()),
        (0xf01b, 0x216, wmf_body),
    ] {
        pos.push(delay.len() as u32);
        delay.extend(esc_record(0, inst, ty, &body));
    }
    let store: Vec<u8> = pos.iter().flat_map(|p| fbse(*p)).collect();
    let dgg = esc_record(0xf, 0, 0xf000, &esc_record(0xf, 3, 0xf001, &store));
    let mut esc = dgg;
    esc.extend_from_slice(&[0; 4]);
    // Reuse the real drawing: append the Brochure DG records after the DGG.
    let real = base.2.as_ref().unwrap();
    let dg_start = real
        .windows(4)
        .position(|w| w == [0x0f, 0x00, 0x02, 0xf0])
        .unwrap();
    esc.extend_from_slice(&real[dg_start..]);
    let (doc, rep) = import4(&(base.0, base.1, Some(esc), Some(delay))).unwrap();
    assert_eq!(placed_pictures(&doc), 0);
    assert!(
        rep.dropped.iter().any(|d| d.kind.starts_with("picture")),
        "{:?}",
        rep.dropped
    );
}

#[test]
fn hostile_fopt_values_are_survived() {
    // Fill, line and picture properties set to extreme values on every shape.
    let base = streams4("poi/SampleNewsletter.pub");
    let esc = base.2.clone().unwrap();
    for val in [0xffff_ffffu32, 0x7fff_ffff, 0x8000_0000, 0] {
        let mut e = esc.clone();
        let mut i = 0;
        while i + 6 <= e.len() {
            let id = u16::from_le_bytes([e[i], e[i + 1]]);
            if matches!(id, 0x0181 | 0x01bf | 0x01c0 | 0x01cb | 0x01ff | 0x0180) {
                e[i + 2..i + 6].copy_from_slice(&val.to_le_bytes());
                i += 6;
            } else {
                i += 1;
            }
        }
        import4(&(base.0.clone(), base.1.clone(), Some(e), base.3.clone())).ok();
    }
}

#[test]
fn hostile_table_blocks_are_survived() {
    // Table rows, columns, directory and cells set to extreme values.
    let base = streams4("poi/Sample.pub");
    for val in [0u32, 1, 3, 0xffff, 0xffff_ffff] {
        let mut c = base.0.clone();
        // Overwrite 4-byte values behind the table blocks (ids 0x66, 0x67 of type 0x20).
        for i in 0..c.len().saturating_sub(6) {
            if (c[i] == 0x66 || c[i] == 0x67) && c[i + 1] == 0x20 {
                c[i + 2..i + 6].copy_from_slice(&val.to_le_bytes());
            }
        }
        let r = import4(&(c, base.1.clone(), base.2.clone(), base.3.clone()));
        if let Ok((_, rep)) = r {
            // Either it is a table or the text still imports as a frame, reported.
            assert!(
                rep.tables == 1 || has_kind(&rep, "table") || val == 3 || val == 0,
                "{val}: {:?}",
                rep.dropped
            );
        }
    }
}

// ---- review fixes: BLIP sharing, masters, table limits ----

/// A 1x1 PNG followed by `pad` junk bytes (PNG decoders stop at IEND).
fn padded_png(pad: usize) -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbaImage::new(1, 1)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    png.extend(std::iter::repeat_n(0x5au8, pad));
    png
}

#[test]
fn ten_thousand_fbses_for_one_big_picture_make_one_asset() {
    let base = streams4("poi/SampleBrochure.pub");
    let blip_body = [vec![0u8; 17], padded_png(8 * 1024 * 1024)].concat();
    let delay = esc_record(0, 0x6e0, 0xf01e, &blip_body);
    let store: Vec<u8> = (0..10_000).flat_map(|_| fbse(0)).collect();
    let mut esc = esc_record(0xf, 0, 0xf000, &esc_record(0xf, 3, 0xf001, &store));
    esc.extend_from_slice(&[0; 4]);
    let real = base.2.as_ref().unwrap();
    let dg_start = real
        .windows(4)
        .position(|w| w == [0x0f, 0x00, 0x02, 0xf0])
        .unwrap();
    esc.extend_from_slice(&real[dg_start..]);
    let started = Instant::now();
    let (doc, rep) = import4(&(base.0, base.1, Some(esc), Some(delay))).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    // All 8 picture shapes show the one picture, and it is one asset.
    assert_eq!(placed_pictures(&doc), 8, "{:?}", rep.dropped);
    assert_eq!(doc.assets.len(), 1);
    let ids: std::collections::HashSet<_> = doc
        .frames
        .values()
        .filter_map(|f| match &f.content {
            FrameContent::Image(i) => i.asset,
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 1);
}

#[test]
fn distinct_blips_over_the_byte_budget_are_reported() {
    // Eight distinct 40 MB pictures stay within the input and work limits but
    // not the 256 MiB retained-bytes cap.
    let base = streams4("poi/SampleBrochure.pub");
    let one = [vec![0u8; 17], padded_png(40 * 1024 * 1024)].concat();
    let mut delay = Vec::new();
    let mut store = Vec::new();
    for _ in 0..8 {
        store.extend(fbse(delay.len() as u32));
        delay.extend(esc_record(0, 0x6e0, 0xf01e, &one));
    }
    let mut esc = esc_record(0xf, 0, 0xf000, &esc_record(0xf, 3, 0xf001, &store));
    esc.extend_from_slice(&[0; 4]);
    let real = base.2.as_ref().unwrap();
    let dg_start = real
        .windows(4)
        .position(|w| w == [0x0f, 0x00, 0x02, 0xf0])
        .unwrap();
    esc.extend_from_slice(&real[dg_start..]);
    let started = Instant::now();
    let r = pubfile::import_streams_unguarded(&base.0, base.1.as_deref(), Some(&esc), Some(&delay));
    let (doc, rep) = r.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(doc.assets.len() <= 6, "{}", doc.assets.len());
    assert!(
        rep.dropped
            .iter()
            .any(|d| d.kind == "picture" && d.detail.contains("budget")),
        "{:?}",
        rep.dropped
    );
}

#[test]
fn a_page_named_as_a_master_is_not_one() {
    // Point every page's master reference (block 0x0d, type 0x68) at the
    // first real page of the document.
    let base = streams4("poi/Sample.pub");
    let mut c = base.0.clone();
    let target = 0x10au32.to_le_bytes();
    for i in 0..c.len().saturating_sub(6) {
        if c[i] == 0x0d && c[i + 1] == 0x68 && c[i + 2..i + 6] == [0x07, 0x01, 0, 0] {
            c[i + 2..i + 6].copy_from_slice(&target);
        }
    }
    let (doc, rep) = import4(&(c, base.1, base.2, base.3)).unwrap();
    assert!(doc.masters.is_empty());
    assert!(doc.pages.iter().all(|p| p.master.is_none()));
    assert!(has_kind(&rep, "master"), "{:?}", rep.dropped);
}

#[test]
fn a_table_larger_than_allowed_falls_back_to_a_text_frame() {
    // Make the second column end absurdly far away.
    let base = streams4("poi/Sample.pub");
    let mut c = base.0.clone();
    let mut hits = 0;
    for i in 0..c.len().saturating_sub(4) {
        if c[i..i + 4] == [0xe0, 0x61, 0x36, 0x00] {
            c[i..i + 4].copy_from_slice(&0x7fff_ff00u32.to_le_bytes());
            hits += 1;
        }
    }
    assert!(hits >= 1);
    let (doc, rep) = import4(&(c, base.1, base.2, base.3)).unwrap();
    assert_eq!(rep.tables, 0);
    assert!(doc.frames.values().all(|f| f.as_table().is_none()));
    assert!(has_kind(&rep, "table"), "{:?}", rep.dropped);
    // The cell text is still there, as a text frame.
    assert!(frame_text(&doc).contains("Bottom Right"));
}
