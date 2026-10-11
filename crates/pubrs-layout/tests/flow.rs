//! Text flow tests: single frames, linked chains, overflow, columns, wrap.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_layout::*;

const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in \
reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint \
occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

fn fonts() -> FontLibrary {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    FontLibrary::from_dir(&dir).expect("test fonts")
}

fn builder() -> DocBuilder<SeqIds> {
    DocBuilder::new(SeqIds::new(7), PageSetup::default())
}

fn layout(doc: &Document) -> DocumentLayout {
    Layouter::new(fonts()).layout_document(doc)
}

fn line_texts(doc: &Document, ft: &FrameText) -> Vec<String> {
    let story = doc
        .stories
        .values()
        .find(|s| {
            s.frames
                .iter()
                .any(|f| ft.lines.is_empty() || doc.frame(*f).is_some())
        })
        .unwrap();
    let text: Vec<char> = story.plain_text().chars().collect();
    ft.lines
        .iter()
        .map(|l| text[l.chars.clone()].iter().collect::<String>())
        .collect()
}

#[test]
fn short_text_fits_one_line() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 200.0, 100.0), "Hello world")
        .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let s = &lay.stories[&doc.story_of(f).unwrap()];
    assert!(s.overflow.is_none());
    let ft = &s.frames[&f];
    assert_eq!(ft.lines.len(), 1);
    assert_eq!(ft.chars, 0..11);
    assert_eq!(line_texts(&doc, ft), vec!["Hello world"]);
    let l = &ft.lines[0];
    assert_eq!(l.rect.x, 4.0, "left inset");
    assert!(l.baseline > 4.0 && l.baseline < 4.0 + l.rect.height);
    assert_eq!(l.carets.len(), 12);
    assert!(l.carets.windows(2).all(|w| w[0].1 <= w[1].1));
    assert_eq!(l.offset_at_x(1000.0), 11);
    assert_eq!(l.offset_at_x(0.0), 0);
    assert!(s.missing_fonts.is_empty());
    assert_eq!(s.missing_glyphs, 0);
}

#[test]
fn overflow_then_linking_flows_into_next_frame() {
    let mut b = builder();
    let p = b.page(0);
    let a = b
        .text(p, Rect::new(36.0, 36.0, 150.0, 40.0), LOREM)
        .unwrap();
    let doc = b.doc().clone();
    let story = doc.story_of(a).unwrap();
    let lay = layout(&doc);
    let o = lay.stories[&story].overflow.expect("overflows");
    let total = doc.story(story).unwrap().char_len();
    assert_eq!(o.offset + o.hidden_chars, total);
    assert_eq!(lay.stories[&story].frames[&a].chars.end, o.offset);

    let c = b.text(p, Rect::new(36.0, 100.0, 110.0, 600.0), "").unwrap();
    b.link(a, c).unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let s = &lay.stories[&story];
    assert!(s.overflow.is_none(), "{:?}", s.overflow);
    let fa = &s.frames[&a];
    let fc = &s.frames[&c];
    assert_eq!(fa.chars.end, o.offset, "first frame shows the same text");
    assert!(fc.chars.start >= fa.chars.end && fc.chars.start <= fa.chars.end + 1);
    assert_eq!(fc.chars.end, total);
    // The second frame is narrower: every line fits its width.
    for l in &fc.lines {
        assert!(l.rect.width <= 110.0 - 8.0 + 1e-6);
        let last = l.runs.iter().flat_map(|r| r.glyphs.iter()).last().unwrap();
        assert!(last.x <= 110.0, "glyph inside frame");
    }
    // Line y restarts at the top inset of the second frame.
    assert_eq!(fc.lines[0].rect.y, 4.0);
}

#[test]
fn columns_split_the_frame() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 300.0, 60.0), LOREM)
        .unwrap();
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            columns: Some(2),
            column_gap: Some(12.0),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let col_w = (300.0 - 8.0 - 12.0) / 2.0;
    let second_x = 4.0 + col_w + 12.0;
    assert!(ft.lines.iter().any(|l| (l.rect.x - 4.0).abs() < 1e-6));
    assert!(ft.lines.iter().any(|l| (l.rect.x - second_x).abs() < 1e-6));
    assert!(ft.lines.iter().all(|l| (l.rect.width - col_w).abs() < 1e-6));
}

#[test]
fn square_wrap_pushes_lines_aside() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 300.0), LOREM).unwrap();
    let img = b
        .shape(p, Rect::new(0.0, 0.0, 100.0, 60.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::Square,
            margin: 6.0,
        },
    )
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    for l in &ft.lines {
        if l.rect.y < 66.0 {
            assert!(
                l.rect.x >= 106.0 - 1e-6,
                "line beside obstacle at y={}",
                l.rect.y
            );
        }
    }
    assert!(ft.lines.iter().any(|l| l.rect.y >= 66.0 && l.rect.x == 4.0));
}

#[test]
fn top_bottom_wrap_skips_the_band() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 400.0), LOREM).unwrap();
    let img = b
        .shape(p, Rect::new(50.0, 40.0, 100.0, 50.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::TopBottom,
            margin: 0.0,
        },
    )
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    for l in &ft.lines {
        assert!(l.rect.bottom() <= 40.0 + 1e-6 || l.rect.y >= 90.0 - 1e-6);
    }
    assert!(ft.lines.iter().any(|l| l.rect.y >= 90.0));
}

#[test]
fn wrap_ignores_frames_below_in_z_order() {
    let mut b = builder();
    let p = b.page(0);
    let img = b
        .shape(p, Rect::new(0.0, 0.0, 100.0, 60.0), Shape::Ellipse)
        .unwrap();
    b.wrap(
        img,
        TextWrap {
            mode: WrapMode::Square,
            margin: 0.0,
        },
    )
    .unwrap();
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 300.0), LOREM).unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    assert_eq!(lay.frame(f).unwrap().lines[0].rect.x, 4.0);
}

#[test]
fn alignment_center_and_right() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Hi").unwrap();
    let s = b.story_of(f);
    let first_x = |doc: &Document| {
        let lay = layout(doc);
        lay.frame(f).unwrap().lines[0].runs[0].glyphs[0].x
    };
    let left = first_x(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Center),
            ..Default::default()
        },
    })
    .unwrap();
    let center = first_x(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            align: Some(Align::Right),
            ..Default::default()
        },
    })
    .unwrap();
    let right = first_x(b.doc());
    assert_eq!(left, 4.0);
    assert!(center > 130.0 && center < 150.0, "{center}");
    assert!(right > 270.0 && right < 296.0, "{right}");
}

#[test]
fn missing_font_falls_back_and_is_reported() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Fallback")
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 8,
        props: CharProps {
            font_family: Some("Nope Sans Pro".into()),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let sl = &lay.stories[&s];
    assert!(sl.missing_fonts.contains("Nope Sans Pro"));
    let glyphs: usize = sl.frames[&f].lines[0]
        .runs
        .iter()
        .map(|r| r.glyphs.len())
        .sum();
    assert_eq!(glyphs, 8, "fallback font draws every character");
    assert_eq!(sl.missing_glyphs, 0);
}

#[test]
fn bold_uses_bold_face_and_italic_serif_is_synthesised() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "Bold Serif")
        .unwrap();
    let s = b.story_of(f);
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 0,
        end: 4,
        props: CharProps {
            bold: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    b.exec(Command::ApplyCharProps {
        story: s,
        start: 5,
        end: 10,
        props: CharProps {
            font_family: Some("Liberation Serif".into()),
            italic: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let runs = &lay.frame(f).unwrap().lines[0].runs;
    assert!(runs.len() >= 2);
    assert!(!runs[0].synthetic_bold, "real bold face in tests/fonts");
    let serif = runs.last().unwrap();
    assert_eq!(
        serif.synthetic_skew,
        Some(14.0),
        "no serif italic face in tests/fonts"
    );
}

#[test]
fn vertical_align_bottom_and_paragraph_spacing() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 200.0, 200.0), "One\nTwo")
        .unwrap();
    let s = b.story_of(f);
    let ys = |doc: &Document| -> Vec<f64> {
        layout(doc)
            .frame(f)
            .unwrap()
            .lines
            .iter()
            .map(|l| l.rect.y)
            .collect()
    };
    let base = ys(b.doc());
    b.exec(Command::ApplyParaProps {
        story: s,
        start: 0,
        end: 0,
        props: ParaProps {
            space_after: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    let spaced = ys(b.doc());
    assert!((spaced[1] - base[1] - 10.0).abs() < 1e-6);
    b.exec(Command::SetTextFrame {
        frame: f,
        props: TextFrameProps {
            vertical_align: Some(VerticalAlign::Bottom),
            ..Default::default()
        },
    })
    .unwrap();
    let lay = layout(b.doc());
    let last = lay.frame(f).unwrap().lines.last().unwrap().rect;
    assert!((last.bottom() - 196.0).abs() < 1e-6, "{last:?}");
}

#[test]
fn empty_story_has_one_caret_line() {
    let mut b = builder();
    let p = b.page(0);
    let f = b.text(p, Rect::new(0.0, 0.0, 200.0, 100.0), "").unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert_eq!(ft.lines.len(), 1);
    assert_eq!(ft.lines[0].carets.len(), 1);
    assert_eq!(ft.lines[0].carets[0].0, 0);
}

#[test]
fn unicode_offsets_are_characters() {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(0.0, 0.0, 300.0, 100.0), "héllo wörld\nñ")
        .unwrap();
    let doc = b.finish();
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert_eq!(ft.lines[0].chars, 0..11);
    assert_eq!(ft.lines[1].chars, 12..13);
    assert_eq!(ft.lines[0].carets.len(), 12);
    // Glyph text ranges index into the paragraph text at char boundaries.
    for r in &ft.lines[0].runs {
        for g in &r.glyphs {
            assert!(r.text.is_char_boundary(g.text.start) && r.text.is_char_boundary(g.text.end));
        }
    }
}

#[test]
fn layout_is_deterministic_across_font_libraries() {
    let mut b = builder();
    let p = b.page(0);
    b.text(p, Rect::new(0.0, 0.0, 180.0, 500.0), LOREM).unwrap();
    let doc = b.finish();
    let fingerprint = |lay: &DocumentLayout| -> Vec<(u32, i64, i64)> {
        lay.stories
            .values()
            .flat_map(|s| s.frames.values())
            .flat_map(|f| f.lines.iter())
            .flat_map(|l| l.runs.iter())
            .flat_map(|r| r.glyphs.iter())
            .map(|g| (g.id, (g.x * 1000.0) as i64, (g.y * 1000.0) as i64))
            .collect()
    };
    let a = fingerprint(&layout(&doc));
    let b2 = fingerprint(&layout(&doc));
    assert!(!a.is_empty());
    assert_eq!(a, b2);
}

/// Text frame (100,100,300x100) rotated 90 degrees about (250,150) covers the
/// page region x 200..300, y 0..300; its local left end is at the page top.
fn rotated_wrap_doc(obstacle: Rect) -> (Document, FrameId) {
    let mut b = builder();
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(100.0, 100.0, 300.0, 100.0), LOREM)
        .unwrap();
    let ob = b.shape(p, obstacle, Shape::Ellipse).unwrap();
    b.wrap(
        ob,
        TextWrap {
            mode: WrapMode::Square,
            margin: 0.0,
        },
    )
    .unwrap();
    let mut doc = b.finish();
    doc.frame_mut(f).unwrap().rotation = 90.0;
    (doc, f)
}

#[test]
fn wrap_around_obstacle_follows_rotated_text_frame() {
    // Overlaps the top of the rotated frame on the page = its local left end
    // (local x 0..40), so lines must start at or beyond x = 40.
    let (doc, f) = rotated_wrap_doc(Rect::new(200.0, 0.0, 100.0, 40.0));
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    assert!(!ft.lines.is_empty());
    for l in &ft.lines {
        assert!(l.rect.x >= 40.0 - 1e-6, "line at x={} overlaps", l.rect.x);
    }
}

#[test]
fn wrap_does_not_cut_rotated_text_frame_where_it_does_not_overlap() {
    // Beside the rotated frame on the page. Its page-space box coincides with
    // the unrotated frame's rect, but it does not touch the rotated frame.
    let (doc, f) = rotated_wrap_doc(Rect::new(320.0, 100.0, 60.0, 60.0));
    let lay = layout(&doc);
    let ft = lay.frame(f).unwrap();
    let mut clear = doc.clone();
    for fr in clear.frames.values_mut() {
        std::sync::Arc::make_mut(fr).wrap.mode = WrapMode::None;
    }
    let base = layout(&clear);
    let base_ft = base.frame(f).unwrap();
    assert_eq!(ft.lines.len(), base_ft.lines.len());
    for (a, b) in ft.lines.iter().zip(&base_ft.lines) {
        assert_eq!(a.rect, b.rect);
    }
}
