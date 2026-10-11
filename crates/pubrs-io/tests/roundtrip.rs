//! Save → load → equal, plus format edge cases.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::path::PathBuf;

use pubrs_core::*;
use pubrs_io::{IoError, format, image_info, import_image, read_bytes, write_bytes};

mod common;

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 128, 255]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([x as u8, y as u8, 64]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

/// A document exercising every part of the model.
fn rich_document() -> Document {
    let mut ids = SeqIds::new(2026);
    let mut ed = Editor::new(Document::new(&mut ids, PageSetup::default()));
    let page1 = ed.doc().pages[0].id;
    let page2 = PageId::new(&mut ids);
    let master = MasterId::new(&mut ids);
    fn run(ed: &mut Editor, c: Command) {
        ed.execute(c).unwrap();
    }
    run(
        &mut ed,
        Command::SetMeta {
            title: Some("Bakery flyer — ünïcode ✓".into()),
            author: Some("pubrs".into()),
        },
    );
    run(&mut ed, Command::SetBleed { bleed: 9.0 });
    run(
        &mut ed,
        Command::AddMaster {
            master: MasterPage {
                id: master,
                name: "Main".into(),
                frames: vec![],
                guides: vec![],
            },
        },
    );
    run(
        &mut ed,
        Command::AddPage {
            page: Page {
                id: page2,
                master: Some(master),
                frames: vec![],
                guides: vec![],
            },
            index: None,
        },
    );
    run(
        &mut ed,
        Command::AddGuide {
            owner: Owner::Page(page1),
            guide: Guide {
                id: GuideId::new(&mut ids),
                axis: Axis::Vertical,
                position: 306.0,
            },
        },
    );
    let (png_asset, png_data) = import_image(&mut ids, "photo.png", png_bytes(64, 48)).unwrap();
    let (jpg_asset, jpg_data) = import_image(&mut ids, "photo.jpg", jpeg_bytes(32, 32)).unwrap();
    let png_id = png_asset.id;
    run(
        &mut ed,
        Command::AddAsset {
            asset: png_asset,
            data: png_data,
        },
    );
    run(
        &mut ed,
        Command::AddAsset {
            asset: jpg_asset,
            data: jpg_data,
        },
    );
    let t1 = FrameId::new(&mut ids);
    let s1 = StoryId::new(&mut ids);
    let mut story = Story::new(s1, t1);
    story.insert_text(0, "Fresh bread\nEvery morning from 7am", None);
    let mut frame = Frame::new(
        t1,
        Rect::new(36.0, 36.0, 300.0, 120.0),
        FrameContent::Text(TextFrame::new(s1)),
    );
    frame.placeholder = Some(Placeholder {
        name: "headline".into(),
        kind: PlaceholderKind::Text,
        hint: "Headline".into(),
    });
    frame.style.fill = Some(Color::rgba(255, 250, 240, 200));
    run(
        &mut ed,
        Command::AddFrame {
            parent: Parent::Page(page1),
            index: None,
            frame,
            story: Some(story),
        },
    );
    let t2 = FrameId::new(&mut ids);
    let s2 = StoryId::new(&mut ids);
    run(
        &mut ed,
        Command::AddFrame {
            parent: Parent::Page(page2),
            index: None,
            frame: Frame::new(
                t2,
                Rect::new(36.0, 36.0, 300.0, 120.0),
                FrameContent::Text(TextFrame::new(s2)),
            ),
            story: Some(Story::new(s2, t2)),
        },
    );
    run(&mut ed, Command::LinkFrames { from: t1, to: t2 });
    let h1 = ed.doc().styles.by_name("Heading 1").unwrap().id;
    run(
        &mut ed,
        Command::SetParaStyle {
            story: s1,
            start: 0,
            end: 0,
            style: Some(h1),
        },
    );
    run(
        &mut ed,
        Command::ApplyCharProps {
            story: s1,
            start: 0,
            end: 5,
            props: CharProps {
                color: Some(Color::rgb(180, 60, 20)),
                italic: Some(true),
                ..Default::default()
            },
        },
    );
    let img = FrameId::new(&mut ids);
    run(
        &mut ed,
        Command::AddFrame {
            parent: Parent::Page(page1),
            index: None,
            frame: Frame::new(
                img,
                Rect::new(36.0, 200.0, 200.0, 150.0),
                FrameContent::Image(ImageFrame::new(Some(png_id))),
            ),
            story: None,
        },
    );
    run(
        &mut ed,
        Command::SetImageCrop {
            frame: img,
            crop: Crop {
                left: 0.1,
                top: 0.0,
                right: 0.05,
                bottom: 0.2,
            },
        },
    );
    let a = FrameId::new(&mut ids);
    let b = FrameId::new(&mut ids);
    for (id, shape) in [
        (a, Shape::Rectangle { corner_radius: 6.0 }),
        (
            b,
            Shape::Line {
                direction: LineDirection::UpRight,
            },
        ),
    ] {
        let mut f = Frame::new(
            id,
            Rect::new(400.0, 400.0, 80.0, 40.0),
            FrameContent::Shape(ShapeFrame { shape }),
        );
        f.style.stroke = Some(Stroke {
            color: Color::rgb(0, 0, 0),
            width: 2.0,
            dash: Dash::Dashed,
        });
        run(
            &mut ed,
            Command::AddFrame {
                parent: Parent::Page(page1),
                index: None,
                frame: f,
                story: None,
            },
        );
    }
    let g = FrameId::new(&mut ids);
    run(
        &mut ed,
        Command::Group {
            group: g,
            frames: vec![a, b],
        },
    );
    run(
        &mut ed,
        Command::SetRotation {
            frame: g,
            degrees: 15.0,
        },
    );
    let m_frame = FrameId::new(&mut ids);
    run(
        &mut ed,
        Command::AddFrame {
            parent: Parent::Master(master),
            index: None,
            frame: Frame::new(
                m_frame,
                Rect::new(0.0, 700.0, 612.0, 92.0),
                FrameContent::Shape(ShapeFrame {
                    shape: Shape::Rectangle { corner_radius: 0.0 },
                }),
            ),
            story: None,
        },
    );
    ed.doc().clone()
}

#[test]
fn save_load_equal() {
    let doc = rich_document();
    assert!(validate::check_integrity(&doc).is_empty());
    let bytes = write_bytes(&doc, None).unwrap();
    let back = read_bytes(&bytes).unwrap();
    assert_eq!(back, doc);
    assert_eq!(
        format::document_json(&back).unwrap(),
        format::document_json(&doc).unwrap()
    );
}

#[test]
fn output_is_deterministic() {
    let doc = rich_document();
    assert_eq!(
        write_bytes(&doc, None).unwrap(),
        write_bytes(&doc, None).unwrap()
    );
}

#[test]
fn archive_layout_uses_forward_slashes() {
    let doc = rich_document();
    let bytes = write_bytes(&doc, Some(&png_bytes(4, 4))).unwrap();
    let zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(names[0], "mimetype");
    assert!(names.contains(&"document.json".to_string()));
    assert!(names.contains(&"thumbnail.png".to_string()));
    let assets: Vec<&String> = names.iter().filter(|n| n.starts_with("assets/")).collect();
    assert_eq!(assets.len(), 2);
    assert!(names.iter().all(|n| !n.contains('\\')));
    assert!(format::read_thumbnail(&bytes).is_some());
}

#[test]
fn save_and_load_from_disk() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("io-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Flyer With Spaces.pubrs");
    let doc = rich_document();
    pubrs_io::save(&doc, &path, None).unwrap();
    // Saving again over an existing file works (atomic replace).
    pubrs_io::save(&doc, &path, None).unwrap();
    assert_eq!(pubrs_io::load(&path).unwrap(), doc);
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
    let missing = pubrs_io::load(&dir.join("nope.pubrs")).unwrap_err();
    assert_eq!(missing.code(), "file_not_found");
}

#[test]
fn rejects_corrupt_and_inconsistent_files() {
    assert!(matches!(read_bytes(b"not a zip"), Err(IoError::Zip(_))));
    let doc = rich_document();
    // Break integrity: reference a missing frame.
    let mut broken = doc.clone();
    broken.pages_mut()[0]
        .frames
        .push(FrameId::new(&mut SeqIds::new(99)));
    let bytes = write_bytes(&broken, None).unwrap();
    assert!(matches!(read_bytes(&bytes), Err(IoError::Integrity(_))));
}

#[test]
fn migrates_unversioned_json() {
    let mut ids = SeqIds::new(5);
    let doc = Document::new(&mut ids, PageSetup::default());
    let mut v = serde_json::to_value(&doc).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("schema_version");
    o.remove("grid");
    o.remove("meta");
    let migrated = format::parse_document_json(&v.to_string()).unwrap();
    assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    assert_eq!(migrated.grid, GridSettings::default());
    assert_eq!(migrated.pages, doc.pages);
}

#[test]
fn image_info_reads_png_and_jpeg() {
    let info = image_info(&png_bytes(64, 48)).unwrap();
    assert_eq!(
        (info.format, info.width, info.height),
        (ImageFormat::Png, 64, 48)
    );
    let info = image_info(&jpeg_bytes(30, 20)).unwrap();
    assert_eq!(
        (info.format, info.width, info.height),
        (ImageFormat::Jpeg, 30, 20)
    );
}

// ---- Locked Decision 2: older files keep opening ----

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// The document `tests/fixtures/schema_v2.pubrs` is written from (bless with
/// `PUBRS_BLESS=1`): a label page set up for Avery 5160, one record per label.
fn sheet_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(2302), PageSetup::default());
    let preset = pubrs_io::sheet_preset("avery-5160").unwrap();
    b.exec(Command::SetPageSize { size: preset.label }).unwrap();
    b.exec(Command::SetSheet {
        sheet: Some(preset.layout(SheetFill::Sequence)),
    })
    .unwrap();
    let p = b.page(0);
    b.text(
        p,
        Rect::new(8.0, 8.0, 170.0, 40.0),
        "Golden Crust\n12 Mill Lane",
    )
    .unwrap();
    b.add_page().unwrap();
    b.finish()
}

#[test]
fn schema_1_bakery_sample_opens_renders_the_same_golden_and_round_trips() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/samples/bakery-trifold.pubrs");
    let doc = pubrs_io::load(&path).unwrap();
    assert_eq!(doc.schema_version, SCHEMA_VERSION, "migrated on load");
    assert!(doc.setup.sheet.is_none() && !doc.setup.booklet);
    // The sample is also kept as the schema-1 fixture.
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(fixtures_dir().join("schema_v1.pubrs")).unwrap()
    );
    let mut ctx = pubrs_io::ExportContext::new(common::test_fonts());
    let png = ctx
        .page_png(
            &doc,
            0,
            &pubrs_render::RasterOptions {
                dpi: 72.0,
                ..Default::default()
            },
        )
        .unwrap();
    common::assert_png_golden("bakery_trifold_schema1", &png);
    let again = read_bytes(&write_bytes(&doc, None).unwrap()).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn every_schema_fixture_opens_and_round_trips() {
    let bless = std::env::var_os("PUBRS_BLESS").is_some();
    let v = SCHEMA_VERSION;
    let current = fixtures_dir().join(format!("schema_v{v}.pubrs"));
    if bless || !current.exists() {
        pubrs_io::save(&sheet_document(), &current, None).unwrap();
    }
    let mut n = 0;
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !(name.starts_with("schema_v") && name.ends_with(".pubrs")) {
            continue;
        }
        let doc = pubrs_io::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.schema_version, SCHEMA_VERSION, "{name}");
        let again = read_bytes(&write_bytes(&doc, None).unwrap()).unwrap();
        assert_eq!(again, doc, "{name} round trip");
        n += 1;
    }
    assert!(n >= 2, "expected schema_v1 and schema_v{v} fixtures");
    let doc = pubrs_io::load(&current).unwrap();
    let sheet = doc
        .setup
        .sheet
        .as_ref()
        .expect("the current fixture uses a sheet");
    assert_eq!(
        (sheet.preset.as_str(), sheet.fill),
        ("avery-5160", SheetFill::Sequence)
    );
}
