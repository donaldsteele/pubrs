//! Character attributes: super/subscript, baseline shift, caps, strikethrough,
//! ligature and kerning switches, and missing-glyph names (ADR 0021).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_core::*;
use pubrs_core::{Baseline, Caps};
use pubrs_layout::*;

fn layouter() -> Layouter {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts");
    Layouter::new(FontLibrary::from_dir(&dir).expect("test fonts"))
}

fn one_frame(text: &str, size: f64) -> (DocBuilder<SeqIds>, FrameId, StoryId) {
    let mut b = DocBuilder::new(SeqIds::new(7), PageSetup::default());
    let p = b.page(0);
    let f = b
        .text(p, Rect::new(36.0, 36.0, 400.0, 200.0), text)
        .unwrap();
    let s = b.story_of(f);
    fmt(
        &mut b,
        s,
        0,
        text.chars().count(),
        CharProps {
            font_size: Some(size),
            ..Default::default()
        },
    );
    (b, f, s)
}

fn fmt(b: &mut DocBuilder<SeqIds>, story: StoryId, start: usize, end: usize, props: CharProps) {
    b.exec(Command::ApplyCharProps {
        story,
        start,
        end,
        props,
    })
    .unwrap();
}

fn lines(doc: &Document, f: FrameId) -> Vec<LineBox> {
    layouter()
        .layout_document(doc)
        .frame(f)
        .unwrap()
        .lines
        .clone()
}

const SANS: &str = "../../tests/fonts/LiberationSans-Regular.ttf";

fn sans_bytes() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SANS)).unwrap()
}

/// Liberation Sans Regular renamed to `family` (15 characters, like the
/// original), with `patch` applied to the bytes: a font with known quirks
/// that cannot clash with the real one.
fn variant(family: &str, patch: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    assert_eq!(family.len(), "Liberation Sans".len());
    let mut data = sans_bytes();
    let from = "Liberation Sans";
    let utf16 = |s: &str| {
        s.encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<u8>>()
    };
    for (a, b) in [
        (from.as_bytes().to_vec(), family.as_bytes().to_vec()),
        (utf16(from), utf16(family)),
    ] {
        let mut i = 0;
        while i + a.len() <= data.len() {
            if data[i..i + a.len()] == a[..] {
                data[i..i + a.len()].copy_from_slice(&b);
                i += a.len();
            } else {
                i += 1;
            }
        }
    }
    patch(&mut data);
    data
}

/// Byte range of the font table `tag`.
fn table(data: &[u8], tag: &[u8; 4]) -> std::ops::Range<usize> {
    use skrifa::raw::FontRef;
    let f = FontRef::new(data).unwrap();
    let rec = f
        .table_directory
        .table_records()
        .iter()
        .find(|r| &r.tag().to_be_bytes() == tag)
        .unwrap();
    let off = rec.offset() as usize;
    off..off + rec.length() as usize
}

fn layouter_with(extra: Vec<u8>) -> Layouter {
    let mut l = layouter();
    l.fonts.add_font(extra);
    l
}

fn os2_of(data: &[u8]) -> (f64, [f64; 4]) {
    use skrifa::raw::{FontRef, TableProvider};
    let f = FontRef::new(data).unwrap();
    let o = f.os2().unwrap();
    (
        f64::from(f.head().unwrap().units_per_em()),
        [
            f64::from(o.y_superscript_y_size()),
            f64::from(o.y_superscript_y_offset()),
            f64::from(o.y_subscript_y_size()),
            f64::from(o.y_subscript_y_offset()),
        ],
    )
}

fn glyphs(l: &LineBox) -> Vec<&PositionedGlyph> {
    l.runs.iter().flat_map(|r| r.glyphs.iter()).collect()
}

fn run_of(l: &LineBox, ch: char) -> &GlyphRun {
    l.runs
        .iter()
        .find(|r| {
            r.glyphs
                .iter()
                .any(|g| r.text[g.text.clone()].starts_with(ch))
        })
        .unwrap()
}

fn y_of(l: &LineBox, ch: char) -> f64 {
    let r = run_of(l, ch);
    r.glyphs
        .iter()
        .find(|g| r.text[g.text.clone()].starts_with(ch))
        .unwrap()
        .y
}

fn props(f: impl FnOnce(&mut CharProps)) -> CharProps {
    let mut p = CharProps::default();
    f(&mut p);
    p
}

#[test]
fn superscript_and_subscript_use_os2_metrics() {
    let (mut b, f, s) = one_frame("Hx2y3z", 20.0);
    let plain = lines(b.doc(), f);
    fmt(
        &mut b,
        s,
        2,
        3,
        props(|p| p.baseline = Some(Baseline::Superscript)),
    );
    fmt(
        &mut b,
        s,
        4,
        5,
        props(|p| p.baseline = Some(Baseline::Subscript)),
    );
    let l = &lines(b.doc(), f)[0];
    let (upem, [sup_size, sup_off, sub_size, sub_off]) = os2_of(&sans_bytes());
    assert!(
        sup_size > 0.0 && sup_off != 0.0,
        "the font has OS/2 records"
    );
    let sup = run_of(l, '2');
    assert!(
        (sup.font_size - 20.0 * sup_size / upem).abs() < 0.02,
        "{}",
        sup.font_size
    );
    let sub = run_of(l, '3');
    assert!((sub.font_size - 20.0 * sub_size / upem).abs() < 0.02);
    // Positions are relative to the line's own baseline: raised is above it.
    let up = l.baseline - y_of(l, '2');
    assert!(
        (up - 20.0 * sup_off.abs() / upem).abs() < 0.05,
        "raised {up}"
    );
    let down = y_of(l, '3') - l.baseline;
    assert!(
        (down - 20.0 * sub_off.abs() / upem).abs() < 0.05,
        "lowered {down}"
    );
    // Neighbours stay on the baseline and the story text is untouched.
    assert!((y_of(l, 'x') - l.baseline).abs() < 1e-6);
    assert!(l.rect.height >= plain[0].rect.height);
    assert_eq!(b.doc().story(s).unwrap().plain_text(), "Hx2y3z");
}

#[test]
fn superscript_falls_back_without_os2_data() {
    let data = variant("Liberation Nopt", |d| {
        let t = table(d, b"OS/2");
        for off in [10usize, 12, 14, 16, 18, 20, 22, 24] {
            d[t.start + off..t.start + off + 2].fill(0);
        }
    });
    let mut lay = layouter_with(data);
    let (mut b, f, s) = one_frame("x2y3", 20.0);
    fmt(
        &mut b,
        s,
        0,
        4,
        props(|p| p.font_family = Some("Liberation Nopt".into())),
    );
    fmt(
        &mut b,
        s,
        1,
        2,
        props(|p| p.baseline = Some(Baseline::Superscript)),
    );
    fmt(
        &mut b,
        s,
        3,
        4,
        props(|p| p.baseline = Some(Baseline::Subscript)),
    );
    let l = lay.layout_document(b.doc()).frame(f).unwrap().lines[0].clone();
    let sup = run_of(&l, '2');
    assert!(
        (sup.font_size - 20.0 * 0.58).abs() < 0.02,
        "{}",
        sup.font_size
    );
    assert!((l.baseline - y_of(&l, '2') - 20.0 * 0.33).abs() < 0.05);
    assert!((y_of(&l, '3') - l.baseline - 20.0 * 0.33).abs() < 0.05);
}

#[test]
fn baseline_shift_moves_without_scaling_and_grows_the_line() {
    let (mut b, f, s) = one_frame("abc", 12.0);
    let plain = lines(b.doc(), f);
    fmt(
        &mut b,
        s,
        1,
        2,
        props(|p| p.baseline = Some(Baseline::Shift(30.0))),
    );
    let l = &lines(b.doc(), f)[0];
    let r = run_of(l, 'b');
    assert_eq!(r.font_size, 12.0);
    assert!((l.baseline - y_of(l, 'b') - 30.0).abs() < 1e-3);
    // The raised run is inside the line box, which grew upward.
    assert!(
        l.rect.height > plain[0].rect.height + 25.0,
        "{}",
        l.rect.height
    );
    assert!(l.baseline - 30.0 - 0.9 * 12.0 >= l.rect.y - 1e-6);
    // Negative shifts lower the run.
    fmt(
        &mut b,
        s,
        1,
        2,
        props(|p| p.baseline = Some(Baseline::Shift(-4.0))),
    );
    let l = &lines(b.doc(), f)[0];
    assert!((y_of(l, 'b') - l.baseline - 4.0).abs() < 1e-3);
}

#[test]
fn all_caps_maps_one_to_one_and_keeps_sharp_s() {
    let (mut b, f, s) = one_frame("straße", 16.0);
    let plain = lines(b.doc(), f);
    fmt(&mut b, s, 0, 6, props(|p| p.caps = Some(Caps::All)));
    let caps = &lines(b.doc(), f)[0];
    let (_, upper) = {
        let (mut b2, f2, _) = one_frame("STRAßE", 16.0);
        let l = lines(b2.doc(), f2);
        let _ = &mut b2;
        ((), l)
    };
    let ids = |l: &LineBox| glyphs(l).iter().map(|g| g.id).collect::<Vec<_>>();
    assert_eq!(
        ids(caps),
        ids(&upper[0]),
        "ß is kept, the rest is capitalised"
    );
    assert_ne!(ids(caps), ids(&plain[0]));
    // Offsets are the story's own, one per character.
    let offs: Vec<_> = glyphs(caps).iter().map(|g| g.text.clone()).collect();
    assert_eq!(offs.len(), 6);
    assert_eq!(caps.chars, plain[0].chars);
    assert_eq!(b.doc().story(s).unwrap().plain_text(), "straße");
    for (o, ch) in "straße"
        .char_indices()
        .map(|(i, _)| i)
        .zip("straße".chars())
    {
        let _ = (o, ch);
    }
}

#[test]
fn synthesized_small_caps_shrink_lowercase_only() {
    let (mut b, f, s) = one_frame("aBc", 20.0);
    fmt(&mut b, s, 0, 3, props(|p| p.caps = Some(Caps::Small)));
    let l = &lines(b.doc(), f)[0];
    // Liberation Sans has no smcp: lowercase letters are capitals at 70%.
    assert_eq!(run_of(l, 'a').font_size, 14.0);
    assert_eq!(run_of(l, 'B').font_size, 20.0);
    assert_eq!(run_of(l, 'c').font_size, 14.0);
    let caps = {
        let (b2, f2, _) = one_frame("ABC", 20.0);
        lines(b2.doc(), f2)
    };
    let ids = |l: &LineBox| glyphs(l).iter().map(|g| g.id).collect::<Vec<_>>();
    assert_eq!(ids(l), ids(&caps[0]));
    assert_eq!(b.doc().story(s).unwrap().plain_text(), "aBc");
}

/// Liberation Sans with a hand-made GSUB table whose `liga` and `smcp`
/// features both turn "fi" into the glyph of "x": a font that has real small
/// caps and a ligature, which no bundled font does.
fn gsub_font() -> Vec<u8> {
    variant("Liberation Smcp", |d| {
        let t = table(d, b"GSUB");
        let be = |v: &[u16]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
        let mut g = Vec::new();
        // Header: version 1.0, script list @10, feature list @32, lookup list @58.
        g.extend(be(&[1, 0, 10, 32, 58]));
        // Script list: DFLT, default language system with both features.
        g.extend(be(&[1]));
        g.extend(b"DFLT");
        g.extend(be(&[8, 4, 0, 0, 0xFFFF, 2, 0, 1]));
        // Feature list: liga and smcp, each one lookup 0.
        g.extend(be(&[2]));
        g.extend(b"liga");
        g.extend(be(&[14]));
        g.extend(b"smcp");
        g.extend(be(&[20]));
        g.extend(be(&[0, 1, 0, 0, 1, 0]));
        // Lookup list: one ligature lookup (type 4), f i -> x.
        let (f, i, x) = (GID_F, GID_I, GID_X);
        g.extend(be(&[1, 4, 4, 0, 1, 8]));
        g.extend(be(&[1, 8, 1, 14]));
        g.extend(be(&[1, 1, f]));
        g.extend(be(&[1, 4]));
        g.extend(be(&[x, 2, i]));
        assert_eq!(g.len(), 94);
        assert!(g.len() <= t.len());
        d[t.start..t.start + g.len()].copy_from_slice(&g);
        d[t.start + g.len()..t.end].fill(0);
    })
}

#[test]
fn strikethrough_draws_a_rule_through_the_text() {
    let (mut b, f, s) = one_frame("strike", 20.0);
    fmt(&mut b, s, 0, 6, props(|p| p.strikethrough = Some(true)));
    let l = &lines(b.doc(), f)[0];
    assert_eq!(l.decorations.len(), 1);
    let d = &l.decorations[0];
    let adv: f64 = glyphs(l).iter().map(|g| g.advance).sum();
    assert!((d.rect.width - adv).abs() < 0.01);
    // Above the baseline, in the x-height band.
    let up = l.baseline - d.rect.y;
    assert!(up > 0.2 * 20.0 && up < 0.5 * 20.0, "{up}");
    assert!(d.rect.height >= 0.5 && d.rect.height < 3.0);
    // Underline and strikethrough together are two rules.
    fmt(&mut b, s, 0, 6, props(|p| p.underline = Some(true)));
    assert_eq!(lines(b.doc(), f)[0].decorations.len(), 2);
}

#[test]
fn kerning_off_widens_pairs() {
    let (mut b, f, s) = one_frame("AVAV", 20.0);
    let width = |l: &LineBox| glyphs(l).iter().map(|g| g.advance).sum::<f64>();
    let on = width(&lines(b.doc(), f)[0]);
    fmt(&mut b, s, 0, 4, props(|p| p.kerning = Some(false)));
    let off = width(&lines(b.doc(), f)[0]);
    assert!(off > on + 1.0, "kerned {on}, unkerned {off}");
}

#[test]
fn missing_characters_are_named() {
    let (b, f, _) = one_frame("ok \u{2713} and \u{2713} \u{1F600}", 12.0);
    let lay = layouter().layout_document(b.doc());
    let sl = lay.stories.values().next().unwrap();
    assert!(sl.missing_glyphs >= 3, "{}", sl.missing_glyphs);
    let names: Vec<char> = sl.missing_chars.iter().copied().collect();
    assert_eq!(names, ['\u{2713}', '\u{1F600}']);
    let _ = f;
    let (b, _, _) = one_frame("all covered", 12.0);
    let lay = layouter().layout_document(b.doc());
    let sl = lay.stories.values().next().unwrap();
    assert!(sl.missing_chars.is_empty() && sl.missing_glyphs == 0);
}

const GID_F: u16 = 73;
const GID_I: u16 = 76;
const GID_X: u16 = 91;
const FAMILY_GSUB: &str = "Liberation Smcp";

/// Shaped glyph ids and the first run's size for "fi" in `family` with `p`.
fn fi(lay: &mut Layouter, family: &str, p: CharProps) -> (Vec<u32>, f64) {
    let (mut b, f, s) = one_frame("fi", 20.0);
    fmt(
        &mut b,
        s,
        0,
        2,
        CharProps {
            font_family: Some(family.into()),
            ..p
        },
    );
    let l = lay.layout_document(b.doc()).frame(f).unwrap().lines[0].clone();
    let ids = glyphs(&l)
        .iter()
        .filter(|g| !g.text.is_empty())
        .map(|g| g.id)
        .collect();
    (ids, l.runs[0].font_size)
}

#[test]
fn small_caps_use_the_smcp_feature_when_the_font_has_it() {
    let mut lay = layouter_with(gsub_font());
    // The feature substitutes: one glyph for "fi", at full size and with no
    // synthesis (ligatures are off, so only smcp can have done it).
    let small_only = props(|p| {
        p.caps = Some(Caps::Small);
        p.ligatures = Some(false);
    });
    assert_eq!(
        fi(&mut lay, FAMILY_GSUB, small_only),
        (vec![u32::from(GID_X)], 20.0)
    );
    // No small caps and no ligatures: two glyphs.
    let neither = props(|p| p.ligatures = Some(false));
    assert_eq!(fi(&mut lay, FAMILY_GSUB, neither).0.len(), 2);
    // A font without smcp: synthesized capitals at 70%.
    let (ids, size) = fi(
        &mut lay,
        "Liberation Sans",
        props(|p| p.caps = Some(Caps::Small)),
    );
    assert_eq!((ids.len(), size), (2, 14.0));
}

#[test]
fn ligatures_off_keeps_letters_apart() {
    let mut lay = layouter_with(gsub_font());
    let on = fi(&mut lay, FAMILY_GSUB, CharProps::default()).0;
    assert_eq!(on, [u32::from(GID_X)], "fi ligates by default");
    let off = fi(&mut lay, FAMILY_GSUB, props(|p| p.ligatures = Some(false))).0;
    assert_eq!(off, [u32::from(GID_F), u32::from(GID_I)]);
    // Kerning off does not touch ligatures.
    let k = fi(&mut lay, FAMILY_GSUB, props(|p| p.kerning = Some(false))).0;
    assert_eq!(k, on);
}

#[test]
fn unset_attributes_lay_out_exactly_as_before() {
    let mut lay = layouter();
    let (mut b, f, s) = one_frame("Plain text with fi and AV.", 14.0);
    let before = lay.layout_document(b.doc());
    // Explicit defaults change nothing visible either.
    fmt(
        &mut b,
        s,
        0,
        26,
        CharProps {
            baseline: Some(Baseline::Normal),
            caps: Some(Caps::None),
            strikethrough: Some(false),
            ligatures: Some(true),
            kerning: Some(true),
            ..Default::default()
        },
    );
    let after = lay.layout_document(b.doc());
    assert_eq!(after.frame(f), before.frame(f));
}
