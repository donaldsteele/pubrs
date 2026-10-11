//! Map parsed Publisher data onto a pubrs [`Document`], through `DocBuilder`
//! commands: pages, text frames with their geometry, linked chains, story text
//! with character and paragraph formatting.

use std::collections::HashMap;

use pubrs_core::Color;
use pubrs_core::builder::DocBuilder;
use pubrs_core::geom::{Margins, Rect, Size};
use pubrs_core::id::{FrameId, PageId, SeqIds, StoryId};
use pubrs_core::model::{
    Document, Frame, FrameContent, PageSetup, Paragraph, Parent, Run, Story, TextFrame,
    VerticalAlign,
};
use pubrs_core::style::{Align, CharProps, ParaProps};

use crate::IoError;

use super::records::{CharStyle, EmuRect, ParaStyle, PubData, RawPara, RawStory};
use super::report::ImportReport;

/// EMU per point.
const EMU_PT: f64 = 12_700.0;
/// Pages and frames beyond these limits are dropped and reported, so a hostile
/// file cannot make the import build an enormous document.
pub const MAX_PAGES: usize = super::records::MAX_PAGES_PARSED;
pub const MAX_FRAMES: usize = 20_000;
/// Smallest and largest page dimension accepted, in points.
const MIN_PAGE: f64 = 4.0;
const MAX_PAGE: f64 = 14_400.0;
/// Default margin guides (Publisher's layout guides are not read yet).
const DEFAULT_MARGIN: f64 = 36.0;

fn pt(emu: i64) -> f64 {
    emu as f64 / EMU_PT
}

/// Colour reference to RGB: type 0x08 indexes the palette, else `0x00BBGGRR`.
fn resolve_color(r: u32, palette: &[[u8; 3]]) -> Option<Color> {
    if (r >> 24) as u8 == 0x08 {
        let c = palette.get((r & 0xff_ffff) as usize)?;
        Some(Color::rgb(c[0], c[1], c[2]))
    } else {
        Some(Color::rgb(r as u8, (r >> 8) as u8, (r >> 16) as u8))
    }
}

fn char_props(span: &CharStyle, dflt: &CharStyle, data: &PubData) -> CharProps {
    let font = span
        .font
        .or(dflt.font)
        .and_then(|i| data.fonts.get(i as usize))
        .filter(|n| !n.is_empty())
        .or_else(|| data.fonts.first().filter(|n| !n.is_empty()));
    let mut p = CharProps {
        font_family: font.cloned(),
        font_size: span
            .size_pt
            .or(dflt.size_pt)
            .filter(|s| *s > 0.0 && *s < 2000.0),
        ..Default::default()
    };
    // Publisher stores bold and italic as a toggle against the style default.
    if span.bold ^ dflt.bold {
        p.bold = Some(true);
    }
    if span.italic ^ dflt.italic {
        p.italic = Some(true);
    }
    if span.underline != 0 || dflt.underline != 0 {
        p.underline = Some(true);
    }
    p.color = span
        .color
        .or(dflt.color)
        .and_then(|c| resolve_color(c, &data.palette));
    p
}

fn para_props(st: &ParaStyle, dflt: &ParaStyle, size_hint: f64) -> ParaProps {
    let pick = |a: Option<u32>, b: Option<u32>| a.or(b);
    let mut p = ParaProps {
        align: pick(st.align, dflt.align).map(|a| match a {
            2 => Align::Center,
            1 => Align::Right,
            6 => Align::Justify,
            _ => Align::Left,
        }),
        ..Default::default()
    };
    match st.line_spacing.or(dflt.line_spacing) {
        // "sp" spacing: 1 sp is single spacing, about 1.2 times the font size.
        Some((false, x)) if x > 0.0 && x < 20.0 => p.line_spacing = Some(x * 1.2),
        Some((true, points)) if points > 0.0 => {
            p.line_spacing = Some((points / size_hint.max(1.0)).clamp(0.5, 10.0));
        }
        _ => {}
    }
    let emu = |v: Option<u32>| v.map(|e| f64::from(e) / EMU_PT).filter(|x| *x < 2000.0);
    p.space_before = emu(pick(st.space_before, dflt.space_before)).filter(|x| *x > 0.0);
    p.space_after = emu(pick(st.space_after, dflt.space_after)).filter(|x| *x > 0.0);
    p.left_indent = emu(pick(st.left_indent, dflt.left_indent)).filter(|x| *x > 0.0);
    p.right_indent = emu(pick(st.right_indent, dflt.right_indent)).filter(|x| *x > 0.0);
    p.first_line_indent = st
        .first_indent
        .or(dflt.first_indent)
        .map(|e| f64::from(e) / EMU_PT)
        .filter(|x| x.abs() < 2000.0 && *x != 0.0);
    p
}

fn convert_para(raw: &RawPara, data: &PubData) -> Paragraph {
    let dflt_para = data.default_para.first().cloned().unwrap_or_default();
    let dchar_idx = raw
        .style
        .default_char
        .or(dflt_para.default_char)
        .unwrap_or(0);
    let dchar = data
        .default_char
        .get(dchar_idx as usize)
        .or_else(|| data.default_char.first())
        .cloned()
        .unwrap_or_default();
    let mut runs: Vec<Run> = Vec::new();
    let mut size = dchar.size_pt.unwrap_or(10.0);
    for (i, (text, st)) in raw.spans.iter().enumerate() {
        let props = char_props(st, &dchar, data);
        if i == 0 {
            size = props.font_size.unwrap_or(size);
        }
        match runs.last_mut() {
            Some(last) if last.props == props => last.text.push_str(text),
            _ => runs.push(Run {
                text: text.clone(),
                style: None,
                props,
            }),
        }
    }
    if runs.is_empty() {
        runs.push(Run {
            text: String::new(),
            style: None,
            props: char_props(&CharStyle::default(), &dchar, data),
        });
    }
    Paragraph {
        style: None,
        props: para_props(&raw.style, &dflt_para, size),
        runs,
    }
}

fn convert_story(raw: &RawStory, data: &PubData) -> Vec<Paragraph> {
    let mut paras: Vec<Paragraph> = raw.paras.iter().map(|p| convert_para(p, data)).collect();
    if paras.is_empty() {
        paras.push(Paragraph::default());
    }
    paras
}

fn frame_rect(s: &super::records::ShapeRec, size: Size) -> Rect {
    let clamp = |v: f64| v.clamp(-MAX_PAGE, MAX_PAGE);
    match s.rect {
        Some(EmuRect { xs, ys, xe, ye }) => {
            let x = size.width / 2.0 + pt(xs);
            let y = size.height / 2.0 + pt(ys);
            Rect::new(
                clamp(x),
                clamp(y),
                (pt(xe) - pt(xs)).clamp(1.0, MAX_PAGE),
                (pt(ye) - pt(ys)).clamp(1.0, MAX_PAGE),
            )
        }
        None => Rect::new(
            DEFAULT_MARGIN,
            DEFAULT_MARGIN,
            (size.width - 2.0 * DEFAULT_MARGIN).max(1.0),
            (size.height - 2.0 * DEFAULT_MARGIN).max(1.0),
        ),
    }
}

/// Build the document.
pub fn build(data: &PubData, rep: &mut ImportReport) -> Result<Document, IoError> {
    let (w, h) = (pt(data.width_emu), pt(data.height_emu));
    if !(MIN_PAGE..=MAX_PAGE).contains(&w) || !(MIN_PAGE..=MAX_PAGE).contains(&h) {
        return Err(IoError::Format(format!(
            "pub: page size {w:.1} x {h:.1} pt is missing or out of range"
        )));
    }
    if data.pages.is_empty() {
        return Err(IoError::Format("pub: the file has no pages".into()));
    }
    let size = Size {
        width: w,
        height: h,
    };
    let setup = PageSetup {
        size,
        margins: Margins::uniform(DEFAULT_MARGIN.min(w / 4.0).min(h / 4.0)),
        bleed: 0.0,
        ..PageSetup::default()
    };
    let mut b = DocBuilder::new(SeqIds::new(0x5055_4252), setup);
    let corrupt = |e: pubrs_core::command::CommandError| {
        IoError::Format(format!("pub: could not build the document: {e}"))
    };

    // Pages.
    let mut page_ids: Vec<(u32, PageId)> = Vec::new();
    for (i, p) in data.pages.iter().enumerate() {
        if i >= MAX_PAGES {
            rep.drop_many("page", data.pages.len() - MAX_PAGES, "page limit reached");
            break;
        }
        let id = if i == 0 {
            b.page(0)
        } else {
            b.add_page().map_err(corrupt)?
        };
        page_ids.push((p.seq, id));
    }
    rep.pages = page_ids.len();

    // Stories by text id.
    let stories: HashMap<u32, &RawStory> = data.stories.iter().map(|s| (s.id, s)).collect();
    let page_of: HashMap<u32, PageId> = page_ids.iter().copied().collect();
    let shape_by_seq: HashMap<u32, &super::records::ShapeRec> =
        data.shapes.iter().map(|s| (s.seq, s)).collect();

    // Text shapes in page order, back to front.
    let mut ordered: Vec<&super::records::ShapeRec> = Vec::new();
    // Shapes bucketed by page once, so this stays linear in pages + shapes.
    let mut shapes_on: HashMap<u32, Vec<&super::records::ShapeRec>> = HashMap::new();
    for s in &data.shapes {
        if let Some(p) = s.page {
            shapes_on.entry(p).or_default().push(s);
        }
    }
    for p in &data.pages {
        let mut seen = std::collections::HashSet::new();
        for seq in &p.shapes {
            if let Some(s) = shape_by_seq.get(seq)
                && s.page == Some(p.seq)
                && seen.insert(*seq)
            {
                ordered.push(s);
            }
        }
        for s in shapes_on.get(&p.seq).into_iter().flatten() {
            if seen.insert(s.seq) {
                ordered.push(s);
            }
        }
    }

    // Chains: shapes sharing a text id are one story. Flow order follows the
    // prev/next links when the file has them, else page order.
    let mut by_text: HashMap<u32, Vec<&super::records::ShapeRec>> = HashMap::new();
    for s in &ordered {
        if let (Some(t), Some(_)) = (s.text_id, s.page)
            && stories.contains_key(&t)
        {
            by_text.entry(t).or_default().push(s);
        }
    }
    let mut chains: HashMap<u32, Vec<u32>> = HashMap::new();
    for (t, list) in &by_text {
        chains.insert(*t, chain_order(list));
    }
    let mut frame_of: HashMap<u32, FrameId> = HashMap::new();
    let mut used_stories = std::collections::HashSet::new();
    for s in ordered {
        let Some(text_id) = s.text_id else { continue };
        let Some(page) = s.page.and_then(|p| page_of.get(&p)).copied() else {
            continue;
        };
        let Some(raw) = stories.get(&text_id) else {
            rep.drop_item(
                "text-frame",
                format!("no story {text_id} for shape {}", s.seq),
            );
            continue;
        };
        if rep.frames >= MAX_FRAMES {
            rep.drop_item("frame", "frame limit reached");
            continue;
        }
        let fid = FrameId::new(&mut b.ids);
        let sid = StoryId::new(&mut b.ids);
        let is_head = chains.get(&text_id).and_then(|c| c.first()) == Some(&s.seq);
        let mut tf = TextFrame::new(sid);
        if let Some(i) = s.insets {
            tf.insets = Margins {
                left: pt(i[0]).clamp(0.0, 200.0),
                top: pt(i[1]).clamp(0.0, 200.0),
                right: pt(i[2]).clamp(0.0, 200.0),
                bottom: pt(i[3]).clamp(0.0, 200.0),
            };
        } else {
            tf.insets = Margins::uniform(2.88);
        }
        if let Some(c) = s.columns.filter(|c| (1..=64).contains(c)) {
            tf.columns = c;
        }
        if let Some(g) = s.column_gap {
            tf.column_gap = pt(g).clamp(0.0, 200.0);
        }
        tf.vertical_align = match s.valign {
            1 => VerticalAlign::Middle,
            2 => VerticalAlign::Bottom,
            _ => VerticalAlign::Top,
        };
        let mut story = Story::new(sid, fid);
        if is_head {
            story.paragraphs = convert_story(raw, data);
            used_stories.insert(text_id);
        }
        let mut frame = Frame::new(fid, frame_rect(s, size), FrameContent::Text(tf));
        frame.rotation = if s.rotation.is_finite() {
            s.rotation.rem_euclid(360.0)
        } else {
            0.0
        };
        b.frame(Parent::Page(page), frame, Some(story))
            .map_err(corrupt)?;
        rep.frames += 1;
        frame_of.insert(s.seq, fid);
    }
    // Link each chain in flow order.
    let mut text_ids: Vec<u32> = chains.keys().copied().collect();
    text_ids.sort_unstable();
    for t in text_ids {
        let frames: Vec<FrameId> = chains[&t]
            .iter()
            .filter_map(|seq| frame_of.get(seq).copied())
            .collect();
        for pair in frames.windows(2) {
            b.link(pair[0], pair[1]).map_err(corrupt)?;
        }
    }
    let unused = data
        .stories
        .iter()
        .filter(|s| !used_stories.contains(&s.id) && s.paras.iter().any(|p| !p.spans.is_empty()))
        .count();
    rep.drop_many("story", unused, "text with no frame on a page");
    Ok(b.finish())
}

/// The shapes of one story in flow order: start at a shape with no (listed)
/// predecessor and follow `next`; anything left over keeps page order.
fn chain_order(list: &[&super::records::ShapeRec]) -> Vec<u32> {
    let seqs: std::collections::HashSet<u32> = list.iter().map(|s| s.seq).collect();
    let next: HashMap<u32, u32> = list
        .iter()
        .filter_map(|s| s.next.filter(|n| seqs.contains(n)).map(|n| (s.seq, n)))
        .collect();
    let has_prev: std::collections::HashSet<u32> = next.values().copied().collect();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in list {
        if has_prev.contains(&s.seq) {
            continue;
        }
        let mut cur = Some(s.seq);
        while let Some(c) = cur {
            if !seen.insert(c) {
                break;
            }
            out.push(c);
            cur = next.get(&c).copied();
        }
    }
    for s in list {
        if seen.insert(s.seq) {
            out.push(s.seq);
        }
    }
    out
}
