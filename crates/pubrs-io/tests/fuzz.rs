//! Property tests: save/load round-trips, and the loaders never panic on
//! hostile input (random bytes, truncation, mutated zip entries).

use std::io::{Cursor, Read, Write};

use proptest::prelude::*;
use pubrs_core::*;
use pubrs_io::format::{read_thumbnail, write_bytes};
use pubrs_io::migrate::migrate;
use pubrs_io::read_bytes;
use zip::write::SimpleFileOptions;

fn sample_doc(texts: &[String], rects: &[[f64; 4]], title: &str) -> Document {
    let mut ids = SeqIds::new(11);
    let mut ed = Editor::new(Document::new(&mut ids, PageSetup::default()));
    let page = ed.doc().pages[0].id;
    let _ = ed.execute(Command::SetMeta {
        title: Some(title.into()),
        author: None,
    });
    for (i, r) in rects.iter().enumerate() {
        let fid = FrameId::new(&mut ids);
        let (content, story) = if let Some(t) = texts.get(i) {
            let sid = StoryId::new(&mut ids);
            let mut s = Story::new(sid, fid);
            s.insert_text(0, t, None);
            (FrameContent::Text(TextFrame::new(sid)), Some(s))
        } else {
            (
                FrameContent::Shape(ShapeFrame {
                    shape: Shape::Ellipse,
                }),
                None,
            )
        };
        let _ = ed.execute(Command::AddFrame {
            parent: Parent::Page(page),
            index: None,
            frame: Frame::new(fid, Rect::new(r[0], r[1], r[2], r[3]), content),
            story,
        });
        let _ = ed.execute(Command::SetRotation {
            frame: fid,
            degrees: r[0] / 7.0,
        });
    }
    ed.doc().clone()
}

fn valid_file() -> Vec<u8> {
    let doc = sample_doc(
        &["Hello\nworld".into(), "Second ünïcode ✓".into()],
        &[
            [10.0, 10.0, 200.0, 100.0],
            [50.0, 60.0, 80.0, 90.0],
            [5.0, 5.0, 30.0, 30.0],
        ],
        "Fuzz",
    );
    write_bytes(&doc, Some(b"\x89PNG not really")).expect("write")
}

/// Rebuild the zip with `document.json` replaced by `edit(original)`.
fn with_mutated_entry(file: &[u8], edit: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(Cursor::new(file)).expect("valid");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut edit = Some(edit);
    for i in 0..zin.len() {
        let mut e = zin.by_index(i).expect("entry");
        let name = e.name().expect("name").to_string();
        let mut data = Vec::new();
        e.read_to_end(&mut data).expect("read");
        if name == "document.json" {
            data = (edit.take().expect("once"))(data);
        }
        out.start_file(name, SimpleFileOptions::default())
            .expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn json_value() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::from),
        any::<i64>().prop_map(serde_json::Value::from),
        "[a-z_]{0,8}".prop_map(serde_json::Value::from),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(serde_json::Value::Array),
            prop::collection::btree_map(
                prop_oneof![
                    Just("schema_version".to_string()),
                    Just("pages".to_string()),
                    Just("frames".to_string()),
                    Just("stories".to_string()),
                    "[a-z_]{0,6}",
                ],
                inner,
                0..5
            )
            .prop_map(|m| serde_json::Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn save_load_round_trips(
        texts in prop::collection::vec("[a-zA-Z ü✓\n]{0,20}", 0..4),
        rects in prop::collection::vec(prop::array::uniform4(0.0f64..500.0), 0..6),
        title in ".{0,12}",
    ) {
        let doc = sample_doc(&texts, &rects, &title);
        let bytes = write_bytes(&doc, None).unwrap();
        let back = read_bytes(&bytes).unwrap();
        prop_assert_eq!(&back, &doc);
        // Deterministic output.
        prop_assert_eq!(write_bytes(&back, None).unwrap(), bytes);
    }

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        let _ = read_bytes(&bytes);
        let _ = read_thumbnail(&bytes);
    }

    #[test]
    fn truncated_file_never_panics(cut in 0.0f64..1.0) {
        let file = valid_file();
        let n = (file.len() as f64 * cut) as usize;
        let _ = read_bytes(&file[..n]);
        let _ = read_thumbnail(&file[..n]);
    }

    #[test]
    fn bit_flipped_file_never_panics(flips in prop::collection::vec((0.0f64..1.0, any::<u8>()), 1..8)) {
        let mut file = valid_file();
        for (pos, xor) in flips {
            let i = ((file.len() - 1) as f64 * pos) as usize;
            file[i] ^= xor;
        }
        let _ = read_bytes(&file);
        let _ = read_thumbnail(&file);
    }

    #[test]
    fn mutated_document_json_never_panics(
        edits in prop::collection::vec((0.0f64..1.0, any::<u8>()), 0..8),
        cut in prop::option::of(0.0f64..1.0),
    ) {
        let file = valid_file();
        let mutated = with_mutated_entry(&file, |mut data| {
            for (pos, b) in edits {
                if !data.is_empty() {
                    let i = ((data.len() - 1) as f64 * pos) as usize;
                    data[i] = b;
                }
            }
            if let Some(c) = cut {
                data.truncate((data.len() as f64 * c) as usize);
            }
            data
        });
        let _ = read_bytes(&mutated);
        let _ = read_thumbnail(&mutated);
    }

    #[test]
    fn migrate_never_panics(v in json_value()) {
        let _ = migrate(v);
    }

    #[test]
    fn migrate_of_object_with_version_never_panics(
        version in prop_oneof![Just(0u64), Just(1), Just(2), any::<u64>()],
    ) {
        let v = serde_json::json!({"schema_version": version});
        let _ = migrate(v);
    }
}
