//! Save → load → equal, plus format edge cases.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::path::PathBuf;

use pubrs_core::style::{List, ListFormat, ListKind, TabAlign, TabLeader, TabStop};
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
    frame.style.fill = Some(Color::rgba(255, 250, 240, 200).into());
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
                color: Some(Color::rgb(180, 60, 20).into()),
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
    run(
        &mut ed,
        Command::SetImageAlt {
            frame: img,
            alt_text: "A loaf of sourdough on a wooden board".into(),
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
            color: Color::rgb(0, 0, 0).into(),
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

/// The document `tests/fixtures/schema_v2.pubrs` was written from: a label page set up for Avery 5160, one record per label.
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

/// Create the missing fixture of the current schema version, once per test
/// process (tests run in parallel and both read it).
fn ensure_current_fixture() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let fixtures: [(u32, Document); 10] = [
            (2, sheet_document()),
            (3, hyphenation_document()),
            (4, common::colour_document()),
            (5, rich_document()),
            (6, paragraph_controls_document()),
            (7, tabs_lists_document()),
            (8, char_format_document()),
            (9, effects_document()),
            (10, wrap_document()),
            (11, table_document()),
        ];
        for (v, doc) in &fixtures {
            let path = fixtures_dir().join(format!("schema_v{v}.pubrs"));
            if !path.exists() && *v == SCHEMA_VERSION {
                pubrs_io::save(doc, &path, None).unwrap();
            }
        }
    });
}

#[test]
fn every_schema_fixture_opens_and_round_trips() {
    // A fixture is created only when it is missing and only for the current
    // schema version; an existing fixture is never rewritten, even when
    // blessing, so files written by earlier versions stay frozen.
    ensure_current_fixture();
    let mut n = 0;
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if !(name.starts_with("schema_v") && name.ends_with(".pubrs")) {
            continue;
        }
        // The version on disk is the one in the file name: no silent upgrade.
        let named: u64 = name["schema_v".len()..name.len() - ".pubrs".len()]
            .parse()
            .unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("document.json").unwrap(), &mut text)
            .unwrap();
        let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(pubrs_io::migrate::version_of(&raw), named, "{name} on disk");
        let doc = pubrs_io::load(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.schema_version, SCHEMA_VERSION, "{name}");
        let again = read_bytes(&write_bytes(&doc, None).unwrap()).unwrap();
        assert_eq!(again, doc, "{name} round trip");
        n += 1;
    }
    assert!(n >= 11, "expected the schema_v1 to v11 fixtures");
    let v5 = pubrs_io::load(&fixtures_dir().join("schema_v5.pubrs")).unwrap();
    let alt: Vec<_> = v5
        .frames
        .values()
        .filter_map(|f| f.as_image().map(|i| i.alt_text.clone()))
        .collect();
    assert_eq!(alt, ["A loaf of sourdough on a wooden board"]);
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v2.pubrs")).unwrap();
    let sheet = doc.setup.sheet.as_ref().expect("v2 fixture uses a sheet");
    assert_eq!(
        (sheet.preset.as_str(), sheet.fill),
        ("avery-5160", SheetFill::Sequence)
    );
    assert_eq!(doc.setup.sheet, sheet_document().setup.sheet);
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v3.pubrs")).unwrap();
    let story = doc.stories.values().next().unwrap();
    let a = doc.styles.resolve_para(&story.paragraphs[0]);
    assert!(a.hyphenate);
    assert_eq!(a.language, "de-DE");
    assert_eq!(a.justify_limits.unwrap().max_word_space, 1.2);
    // The schema 4 fixture uses swatches, a scheme, tints and gradients.
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v4.pubrs")).unwrap();
    assert_eq!(doc.scheme, Some(SchemeId("copper_dawn".into())));
    assert!(doc.swatches.len() >= 10, "scheme slots plus three swatches");
    let paints: Vec<&Paint> = doc
        .frames
        .values()
        .filter_map(|f| f.style.fill.as_ref())
        .collect();
    assert!(paints.iter().any(|p| matches!(
        p,
        Paint::Gradient {
            gradient: GradientKind::Linear,
            ..
        }
    )));
    assert!(paints.iter().any(|p| matches!(
        p,
        Paint::Gradient {
            gradient: GradientKind::Radial,
            ..
        }
    )));
    assert!(
        paints
            .iter()
            .any(|p| matches!(p, Paint::Swatch { tint, .. } if *tint == 25.0))
    );
    assert!(
        doc.swatches
            .iter()
            .any(|s| matches!(s.value, SwatchValue::Spot { .. }))
    );
    assert!(
        doc.swatches
            .iter()
            .any(|s| matches!(s.value, SwatchValue::Cmyk(_)))
    );
    // Keeps, widow and orphan control, a drop cap and one break character
    // of each kind.
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v6.pubrs")).unwrap();
    let story = doc.stories.values().next().unwrap();
    let heading = doc.styles.resolve_para(&story.paragraphs[0]);
    let body = doc.styles.resolve_para(&story.paragraphs[1]);
    assert!(heading.keep_with_next && !heading.keep_together);
    assert!(body.keep_together);
    assert_eq!(body.widow_orphan, 2);
    assert_eq!(
        body.drop_cap,
        Some(pubrs_core::style::DropCap { lines: 3, chars: 2 })
    );
    let text = story.plain_text();
    for k in pubrs_core::text::BreakKind::ALL {
        assert!(text.contains(k.char()), "{k:?} break survives the file");
    }
    // Tab stops with leaders, and a nested numbered list.
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v7.pubrs")).unwrap();
    let story = doc.stories.values().next().unwrap();
    let price = doc.styles.resolve_para(&story.paragraphs[0]);
    assert_eq!(
        price.tab_stops,
        [TabStop {
            pos: 200.0,
            align: TabAlign::Right,
            leader: TabLeader::Dot
        }]
    );
    let item = doc.styles.resolve_para(&story.paragraphs[2]).list.unwrap();
    assert_eq!((item.kind, item.level), (ListKind::Number, 1));
    assert_eq!(item.format, ListFormat::LowerAlpha);
    // The current fixture: super and subscript, baseline shift, caps,
    // strikethrough and the ligature and kerning switches.
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v8.pubrs")).unwrap();
    let story = doc.stories.values().next().unwrap();
    let runs = &story.paragraphs[0].runs;
    assert!(
        runs.iter()
            .any(|r| r.props.baseline == Some(Baseline::Subscript))
    );
    assert!(
        runs.iter()
            .any(|r| r.props.baseline == Some(Baseline::Shift(3.5)))
    );
    assert!(runs.iter().any(|r| r.props.caps == Some(Caps::Small)));
    assert!(runs.iter().any(|r| r.props.caps == Some(Caps::All)));
    assert!(runs.iter().any(|r| r.props.strikethrough == Some(true)));
    assert!(runs.iter().any(|r| r.props.ligatures == Some(false)));
    assert!(runs.iter().any(|r| r.props.kerning == Some(false)));
}

/// The schema 9 fixture: WordArt with a gradient, an outline and a shadow,
/// a swatch-coloured drop shadow, and BorderArt.
#[test]
fn schema_9_fixture_keeps_wordart_shadows_and_border_art() {
    ensure_current_fixture();
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v9.pubrs")).unwrap();
    let art: Vec<_> = doc.frames.values().filter_map(|f| f.as_wordart()).collect();
    assert_eq!(art.len(), 1);
    let w = art[0];
    assert_eq!((w.text.as_str(), w.warp), ("Grand Opening", Warp::ArchUp));
    assert!(w.fill.is_gradient());
    assert_eq!(w.outline.as_ref().map(|o| o.width), Some(1.5));
    assert_eq!(
        w.shadow.as_ref().map(|s| (s.offset_x, s.blur)),
        Some((3.0, 4.0))
    );
    let styled: Vec<_> = doc
        .frames
        .values()
        .filter(|f| f.style.shadow.is_some() || f.style.border_art.is_some())
        .collect();
    assert_eq!(styled.len(), 2);
    assert!(styled.iter().any(|f| matches!(
        f.style.shadow.as_ref().map(|s| &s.color),
        Some(Paint::Swatch { .. })
    )));
    assert!(styled.iter().any(|f| {
        f.style
            .border_art
            .as_ref()
            .is_some_and(|b| b.pattern == "scallops" && b.width == 14.0)
    }));
    // Older files never mention the new fields.
    let json = serde_json::to_string(&paragraph_controls_document()).unwrap();
    assert!(!json.contains("shadow") && !json.contains("border_art") && !json.contains("word_art"));
}

/// The document `tests/fixtures/schema_v8.pubrs` is written from.
fn effects_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(1702), PageSetup::default());
    let p = b.page(0);
    let ink = SwatchId::new(&mut b.ids);
    b.exec(Command::AddSwatch {
        swatch: Swatch {
            id: ink,
            name: "Shadow Brown".into(),
            value: SwatchValue::Cmyk(Cmyk::new(30.0, 60.0, 80.0, 40.0)),
        },
    })
    .unwrap();
    b.wordart(
        p,
        Rect::new(36.0, 36.0, 400.0, 140.0),
        WordArtFrame {
            text: "Grand Opening".into(),
            font: "Liberation Sans".into(),
            size: 72.0,
            warp: Warp::ArchUp,
            fill: Paint::Gradient {
                gradient: GradientKind::Linear,
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::rgb(250, 204, 21).into(),
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::rgb(220, 38, 38).into(),
                    },
                ],
                angle: 90.0,
            },
            outline: Some(Stroke {
                color: Color::rgb(30, 20, 60).into(),
                width: 1.5,
                dash: Dash::Solid,
            }),
            shadow: Some(Shadow {
                offset_x: 3.0,
                offset_y: 3.0,
                blur: 4.0,
                color: Color::BLACK.into(),
                opacity: 0.5,
            }),
        },
    )
    .unwrap();
    let r = b
        .shape(
            p,
            Rect::new(36.0, 220.0, 200.0, 100.0),
            Shape::Rectangle { corner_radius: 8.0 },
        )
        .unwrap();
    b.style(
        r,
        FrameStyle {
            fill: Some(Color::rgb(30, 120, 220).into()),
            shadow: Some(Shadow {
                offset_x: 5.0,
                offset_y: 6.0,
                blur: 6.0,
                color: Paint::swatch(ink),
                opacity: 0.6,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let t = b
        .text(p, Rect::new(260.0, 220.0, 220.0, 100.0), "Bordered text")
        .unwrap();
    b.style(
        t,
        FrameStyle {
            border_art: Some(BorderArt {
                pattern: "scallops".into(),
                width: 14.0,
            }),
            ..Default::default()
        },
    )
    .unwrap();
    b.finish()
}

/// The document `tests/fixtures/schema_v6.pubrs` is written from: a
/// heading kept with its paragraph, a kept-together paragraph with widow
/// and orphan control and a two-letter drop cap, and one break character
/// of each kind, across two linked frames on two pages.
fn paragraph_controls_document() -> Document {
    use pubrs_core::text::BreakKind;
    let mut b = DocBuilder::new(SeqIds::new(1502), PageSetup::default());
    let p = b.page(0);
    let text = format!(
        "Spring menu\nOur ovens open at six.{}New loaves daily.{}Pastries{}Order ahead.",
        BreakKind::Column.char(),
        BreakKind::Frame.char(),
        BreakKind::Page.char()
    );
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 200.0), &text)
        .unwrap();
    let p2 = b.add_page().unwrap();
    let g = b.text(p2, Rect::new(36.0, 36.0, 300.0, 200.0), "").unwrap();
    b.link(f, g).unwrap();
    let story = b.story_of(f);
    b.exec(Command::ApplyParaProps {
        story,
        start: 0,
        end: 0,
        props: ParaProps {
            keep_with_next: Some(true),
            keep_together: Some(false),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyParaProps {
        story,
        start: 12,
        end: 12,
        props: ParaProps {
            keep_together: Some(true),
            widow_orphan: Some(2),
            drop_cap: Some(pubrs_core::style::DropCap { lines: 3, chars: 2 }),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(2),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

/// The document `tests/fixtures/schema_v3.pubrs` is written from: a
/// justified German paragraph with hyphenation and justify limits.
fn hyphenation_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(1402), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(
            p,
            Rect::new(36.0, 36.0, 200.0, 120.0),
            "Donaudampfschifffahrtsgesellschaft und Kaffeetassenhersteller.",
        )
        .unwrap();
    let story = b.story_of(f);
    b.exec(Command::ApplyParaProps {
        story,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Justify),
            hyphenate: Some(true),
            language: Some("de-DE".into()),
            justify_limits: Some(JustifyLimits {
                max_word_space: 1.2,
                letter_spacing: 0.03,
                ..Default::default()
            }),
            ..Default::default()
        },
    })
    .unwrap();
    b.finish()
}

/// Old documents keep their hex colours byte for byte: a document with only
/// hex paints serializes without swatches or scheme, and its colours stay
/// plain strings.
#[test]
fn hex_colours_stay_plain_strings() {
    let mut b = DocBuilder::new(SeqIds::new(5), PageSetup::default());
    let p = b.page(0);
    let f = b
        .shape(p, Rect::new(0.0, 0.0, 10.0, 10.0), Shape::Ellipse)
        .unwrap();
    b.style(
        f,
        FrameStyle {
            fill: Some(Color::rgb(255, 0, 16).into()),
            ..Default::default()
        },
    )
    .unwrap();
    let json = serde_json::to_string(&b.finish()).unwrap();
    assert!(json.contains("\"fill\":\"#FF0010\""), "{json}");
    assert!(!json.contains("swatches") && !json.contains("scheme"));
}

/// The document `tests/fixtures/schema_v7.pubrs` is written from: a price
/// line with a right tab and dot leader, and a numbered list with a nested
/// lettered item.
fn tabs_lists_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(1602), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(
            p,
            Rect::new(36.0, 36.0, 300.0, 200.0),
            "Tomato soup\t4.50\nFirst course\nGarlic bread\nMain course",
        )
        .unwrap();
    let story = b.story_of(f);
    b.exec(Command::ApplyParaProps {
        story,
        start: 0,
        end: 0,
        props: ParaProps {
            tab_stops: Some(vec![TabStop {
                pos: 200.0,
                align: TabAlign::Right,
                leader: TabLeader::Dot,
            }]),
            ..Default::default()
        },
    })
    .unwrap();
    for (para, level, format) in [
        (1, 0, ListFormat::Decimal),
        (2, 1, ListFormat::LowerAlpha),
        (3, 0, ListFormat::Decimal),
    ] {
        let at = b.doc().story(story).unwrap().paragraph_start(para);
        b.exec(Command::ApplyParaProps {
            story,
            start: at,
            end: at,
            props: ParaProps {
                left_indent: Some(18.0 * (f64::from(level) + 1.0)),
                list: Some(List {
                    kind: ListKind::Number,
                    format,
                    level,
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
        .unwrap();
    }
    b.finish()
}

/// The document `tests/fixtures/schema_v8.pubrs` is written from: runs with
/// superscript, subscript, a custom baseline shift, small caps, all caps,
/// strikethrough, and ligatures and kerning switched off.
fn char_format_document() -> Document {
    use pubrs_core::{Baseline, Caps};
    let mut b = DocBuilder::new(SeqIds::new(1702), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(
            p,
            Rect::new(36.0, 36.0, 300.0, 200.0),
            "H2O and x2 raised small CAPS all caps struck office AV",
        )
        .unwrap();
    let story = b.story_of(f);
    let marks: [(usize, usize, CharProps); 7] = [
        (
            1,
            2,
            CharProps {
                baseline: Some(Baseline::Subscript),
                ..Default::default()
            },
        ),
        (
            11,
            12,
            CharProps {
                baseline: Some(Baseline::Superscript),
                ..Default::default()
            },
        ),
        (
            13,
            19,
            CharProps {
                baseline: Some(Baseline::Shift(3.5)),
                ..Default::default()
            },
        ),
        (
            20,
            31,
            CharProps {
                caps: Some(Caps::Small),
                ..Default::default()
            },
        ),
        (
            32,
            40,
            CharProps {
                caps: Some(Caps::All),
                ..Default::default()
            },
        ),
        (
            41,
            47,
            CharProps {
                strikethrough: Some(true),
                ligatures: Some(false),
                ..Default::default()
            },
        ),
        (
            48,
            54,
            CharProps {
                kerning: Some(false),
                ..Default::default()
            },
        ),
    ];
    for (start, end, props) in marks {
        b.exec(Command::ApplyCharProps {
            story,
            start,
            end,
            props,
        })
        .unwrap();
    }
    b.finish()
}

/// The document `tests/fixtures/schema_v10.pubrs` is written from: text
/// wrapping tight around an ellipse on both sides, through a line and
/// tight around a picture frame.
fn wrap_document() -> Document {
    let mut b = DocBuilder::new(SeqIds::new(1902), PageSetup::default());
    let p = b.page(0);
    b.text(
        p,
        Rect::new(36.0, 36.0, 300.0, 240.0),
        "Text runs around the outline of a shape and, when asked, on both sides of it.",
    )
    .unwrap();
    let e = b
        .shape(p, Rect::new(150.0, 60.0, 90.0, 90.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        e,
        TextWrap {
            mode: WrapMode::Tight,
            margin: 8.0,
            both_sides: true,
        },
    )
    .unwrap();
    let l = b
        .shape(
            p,
            Rect::new(60.0, 160.0, 60.0, 60.0),
            Shape::Line {
                direction: LineDirection::UpRight,
            },
        )
        .unwrap();
    b.wrap(
        l,
        TextWrap {
            mode: WrapMode::Through,
            margin: 4.0,
            both_sides: false,
        },
    )
    .unwrap();
    let i = b
        .image(p, Rect::new(220.0, 160.0, 80.0, 80.0), None)
        .unwrap();
    b.wrap(
        i,
        TextWrap {
            mode: WrapMode::Tight,
            margin: 6.0,
            both_sides: false,
        },
    )
    .unwrap();
    b.finish()
}

/// The schema 10 fixture: tight and through wrap and text on both sides.
#[test]
fn schema_10_fixture_keeps_wrap_modes() {
    ensure_current_fixture();
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v10.pubrs")).unwrap();
    let wraps: Vec<_> = doc
        .frames
        .values()
        .map(|f| f.wrap)
        .filter(|w| w.mode != WrapMode::None)
        .collect();
    assert!(
        wraps
            .iter()
            .any(|w| w.mode == WrapMode::Tight && w.both_sides)
    );
    assert!(wraps.iter().any(|w| w.mode == WrapMode::Through));
    assert!(
        wraps
            .iter()
            .any(|w| w.mode == WrapMode::Tight && !w.both_sides)
    );
}

/// The document `tests/fixtures/schema_v11.pubrs` is written from: a 3x3
/// table with a merged header, a fill, borders, insets and a vertically
/// centred cell.
fn table_document() -> Document {
    use pubrs_core::table::CellStyle;
    let mut b = DocBuilder::new(SeqIds::new(1903), PageSetup::default());
    let p = b.page(0);
    let hair = Stroke {
        color: Color::BLACK.into(),
        width: 0.75,
        dash: Dash::Solid,
    };
    let style = CellStyle {
        borders: std::array::from_fn(|_| Some(hair.clone())),
        ..CellStyle::default()
    };
    let t = b
        .table(
            Parent::Page(p),
            Rect::new(36.0, 36.0, 300.0, 120.0),
            3,
            3,
            &style,
        )
        .unwrap();
    b.exec(Command::TableMergeCells {
        frame: t,
        row: 0,
        col: 0,
        row_span: 1,
        col_span: 3,
    })
    .unwrap();
    b.exec(Command::TableSetCellStyle {
        frame: t,
        row: 0,
        col: 0,
        style: CellStyle {
            fill: Some(Color::rgb(0xDB, 0xE4, 0xF5).into()),
            valign: VerticalAlign::Middle,
            insets: Margins::uniform(6.0),
            ..style.clone()
        },
    })
    .unwrap();
    b.cell_text(t, 0, 0, "Prices").unwrap();
    b.cell_text(t, 1, 0, "Cakes").unwrap();
    b.cell_text(t, 1, 2, "2.50").unwrap();
    b.finish()
}

/// The schema 11 fixture: a table keeps its cells, spans and styles.
#[test]
fn schema_11_fixture_keeps_tables() {
    ensure_current_fixture();
    let doc = pubrs_io::load(&fixtures_dir().join("schema_v11.pubrs")).unwrap();
    let t = doc
        .frames
        .values()
        .find_map(|f| f.as_table())
        .expect("a table");
    assert_eq!((t.rows.len(), t.cols.len()), (3, 3));
    let head = t.cell_at(0, 2).unwrap();
    assert_eq!((head.row, head.col, head.col_span), (0, 0, 3));
    assert_eq!(head.valign, VerticalAlign::Middle);
    assert!(head.fill.is_some() && head.borders.iter().all(Option::is_some));
    assert!(pubrs_core::validate::check_integrity(&doc).is_empty());
    let text = |r, c| {
        doc.story(t.cell_at(r, c).unwrap().story)
            .unwrap()
            .plain_text()
    };
    assert_eq!(
        (text(0, 0).as_str(), text(1, 0).as_str()),
        ("Prices", "Cakes")
    );
}
