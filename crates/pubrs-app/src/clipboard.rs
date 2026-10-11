//! Clipboard traffic: frames out and in (ADR 0025), pictures, and text
//! reduced from HTML to bold, italic, underline, size and colour.

use base64::Engine;
use pubrs_api::core::{CharProps, Color, DocumentId, FrameId, PageId, Paint, Parent};
use pubrs_api::tools::frames::FrameAddImage;
use pubrs_api::tools::frames::{ClipPayload, FrameAddText, PasteAt};

use crate::app::PubrsApp;
use crate::platform::ClipImage;
use crate::textedit::TextEdit;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// Largest HTML put on the OS clipboard. Bigger selections still paste
/// inside this process (the app keeps the payload itself).
pub const MAX_CLIP_HTML: usize = 48 * 1024 * 1024;

/// Largest base64 section of a pubrs payload read from the clipboard.
pub const MAX_CLIP_BASE64: usize = 96 * 1024 * 1024;
/// HTML without a pubrs payload over this size is pasted as plain text.
pub const MAX_REDUCE_HTML: usize = 4 * 1024 * 1024;
/// Most characters and runs kept from reduced HTML.
pub const MAX_REDUCED_CHARS: usize = 200_000;
pub const MAX_REDUCED_RUNS: usize = 10_000;
/// Deepest nesting of formatting tags followed.
const MAX_TAG_DEPTH: usize = 64;
/// Most characters of plain text pasted at once.
pub const MAX_PASTE_CHARS: usize = 1_000_000;

/// Frames copied in this process, and the text we put on the OS clipboard
/// with them, to tell whether another program has replaced it since.
#[derive(Debug, Clone)]
pub struct InProcessClip {
    pub payload: ClipPayload,
    pub marker: String,
    pub doc: DocumentId,
}

/// HTML for the OS clipboard: the payload in a comment, and a picture of
/// the selection for programs that paste images from HTML.
pub fn payload_to_html(payload: &ClipPayload, preview_png: Option<&[u8]>) -> Option<String> {
    let json = payload.to_json();
    if json.is_empty() {
        return None;
    }
    let mut html = String::from("<meta charset=\"utf-8\"><div data-pubrs=\"1\"><!--pubrs:");
    html.push_str(&B64.encode(json.as_bytes()));
    html.push_str("-->");
    if let Some(png) = preview_png {
        html.push_str("<img src=\"data:image/png;base64,");
        html.push_str(&B64.encode(png));
        html.push_str("\">");
    }
    html.push_str("</div>");
    (html.len() <= MAX_CLIP_HTML).then_some(html)
}

/// The payload inside HTML made by [`payload_to_html`].
pub fn payload_from_html(html: &str) -> Option<ClipPayload> {
    let start = html.find("<!--pubrs:")? + "<!--pubrs:".len();
    let end = html[start..].find("-->")? + start;
    if end - start > MAX_CLIP_BASE64 {
        return None;
    }
    let bytes = B64.decode(html[start..end].trim()).ok()?;
    ClipPayload::from_json(std::str::from_utf8(&bytes).ok()?)
}

// ------------------------------------------------------------------ HTML

/// A piece of text with the formatting HTML gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct RichRun {
    pub text: String,
    pub props: CharProps,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Fmt {
    bold: bool,
    italic: bool,
    underline: bool,
    size: Option<f64>,
    color: Option<Color>,
}

impl Fmt {
    fn props(&self) -> CharProps {
        CharProps {
            bold: self.bold.then_some(true),
            italic: self.italic.then_some(true),
            underline: self.underline.then_some(true),
            font_size: self.size,
            color: self.color.map(Paint::Color),
            ..Default::default()
        }
    }
}

fn parse_color(v: &str) -> Option<Color> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|x| x * 17);
        let b = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        if !h.is_ascii() {
            return None;
        }
        return match h.len() {
            3 => Some(Color::rgb(d(0)?, d(1)?, d(2)?)),
            6 => Some(Color::rgb(b(0)?, b(2)?, b(4)?)),
            _ => None,
        };
    }
    if let Some(inner) = v
        .strip_prefix("rgb(")
        .or_else(|| v.strip_prefix("rgba("))
        .and_then(|r| r.split(')').next())
    {
        let n: Vec<f64> = inner
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .filter_map(|s| s.trim_end_matches('%').parse().ok())
            .collect();
        if n.len() == 3 {
            let c = |x: f64| x.round().clamp(0.0, 255.0) as u8;
            return Some(Color::rgb(c(n[0]), c(n[1]), c(n[2])));
        }
        return None;
    }
    Some(match v.as_str() {
        "black" => Color::rgb(0, 0, 0),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "yellow" => Color::rgb(255, 255, 0),
        "orange" => Color::rgb(255, 165, 0),
        "purple" => Color::rgb(128, 0, 128),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        _ => return None,
    })
}

fn parse_size(v: &str) -> Option<f64> {
    let v = v.trim().to_ascii_lowercase();
    let (num, scale) = match v.strip_suffix("pt") {
        Some(n) => (n, 1.0),
        None => (v.strip_suffix("px")?, 0.75),
    };
    let pt = num.trim().parse::<f64>().ok()? * scale;
    (pt.is_finite() && (1.0..=400.0).contains(&pt)).then_some(pt)
}

/// Apply the declarations of a `style="..."` attribute.
fn apply_style(fmt: &mut Fmt, style: &str) {
    for decl in style.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_ascii_lowercase());
        match k.as_str() {
            "font-weight" => {
                fmt.bold = v == "bold" || v == "bolder" || v.parse::<u32>().is_ok_and(|w| w >= 600);
            }
            "font-style" => fmt.italic = v == "italic" || v == "oblique",
            "text-decoration" | "text-decoration-line" => {
                if v.contains("underline") {
                    fmt.underline = true;
                } else if v.contains("none") {
                    fmt.underline = false;
                }
            }
            "font-size" => {
                if let Some(s) = parse_size(&v) {
                    fmt.size = Some(s);
                }
            }
            "color" => {
                if let Some(c) = parse_color(&v) {
                    fmt.color = Some(c);
                }
            }
            _ => {}
        }
    }
}

/// The value of attribute `name` in a tag's attribute text.
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
        let rest = attrs[at + name.len()..].trim_start();
        if before_ok && let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start();
            let (q, body) = match r.chars().next() {
                Some(q @ ('"' | '\'')) => (q, &r[1..]),
                _ => return r.split_whitespace().next(),
            };
            return body.split(q).next();
        }
        from = at + name.len();
    }
    None
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // Only the next few bytes can hold a name: no scan to the end.
        let window = &rest.as_bytes()[..rest.len().min(12)];
        let Some(semi) = window.iter().position(|b| *b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

const BLOCKS: [&str; 17] = [
    "p",
    "div",
    "li",
    "tr",
    "ul",
    "ol",
    "table",
    "blockquote",
    "pre",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
];

/// HTML reduced to paragraphs of runs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reduced {
    pub paras: Vec<Vec<RichRun>>,
    /// The input was longer than the caps and the rest was dropped.
    pub truncated: bool,
}

/// Reduce HTML to paragraphs of runs. Tolerant: unknown tags vanish and
/// their text stays; `<script>`, `<style>` and `<head>` content is dropped.
/// Bounded: formatting follows at most [`MAX_TAG_DEPTH`] nested tags, and the
/// output stops at [`MAX_REDUCED_CHARS`] characters or [`MAX_REDUCED_RUNS`] runs.
pub fn html_to_paragraphs(html: &str) -> Reduced {
    struct Out {
        paras: Vec<Vec<RichRun>>,
        chars: usize,
        runs: usize,
        full: bool,
    }
    impl Out {
        fn push_text(&mut self, fmt: &Fmt, text: &str, pre: bool) {
            let decoded = decode_entities(text);
            let mut prev_space = self
                .paras
                .last()
                .and_then(|p| p.last())
                .is_none_or(|r| r.text.ends_with(' '));
            let mut piece = String::with_capacity(decoded.len().min(4096));
            for ch in decoded.chars() {
                if self.chars >= MAX_REDUCED_CHARS {
                    self.full = true;
                    break;
                }
                let ch = if pre {
                    if ch == '\r' {
                        continue;
                    }
                    ch
                } else if ch.is_whitespace() {
                    if prev_space {
                        continue;
                    }
                    prev_space = true;
                    ' '
                } else {
                    prev_space = false;
                    ch
                };
                piece.push(ch);
                self.chars += 1;
            }
            if piece.is_empty() {
                return;
            }
            let props = fmt.props();
            let Some(cur) = self.paras.last_mut() else {
                return;
            };
            match cur.last_mut() {
                Some(r) if r.props == props => r.text.push_str(&piece),
                _ => {
                    self.runs += 1;
                    cur.push(RichRun { text: piece, props });
                }
            }
            if self.runs >= MAX_REDUCED_RUNS {
                self.full = true;
            }
        }
        fn new_para(&mut self, force: bool) {
            if self.paras.last().is_some_and(|c| force || !c.is_empty()) {
                self.paras.push(Vec::new());
                self.chars += 1;
                if self.chars >= MAX_REDUCED_CHARS {
                    self.full = true;
                }
            }
        }
    }
    let mut out = Out {
        paras: vec![Vec::new()],
        chars: 0,
        runs: 0,
        full: false,
    };
    let mut stack: Vec<(String, Fmt)> = Vec::new();
    let mut pre = 0usize;
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() && !out.full {
        let tag_start = bytes[i] == b'<'
            && bytes
                .get(i + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'/' | b'!' | b'?'));
        if !tag_start {
            let first = html[i..].chars().next().map_or(1, char::len_utf8);
            let end = html[i + first..]
                .find('<')
                .map_or(html.len(), |j| i + first + j);
            let fmt = stack.last().map(|s| s.1.clone()).unwrap_or_default();
            out.push_text(&fmt, &html[i..end], pre > 0);
            i = end;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map_or(html.len(), |j| i + j + 3);
            continue;
        }
        let Some(close) = html[i..].find('>').map(|j| i + j) else {
            break;
        };
        let inner = &html[i + 1..close];
        let closing = inner.starts_with('/');
        let body = inner.trim_start_matches('/').trim_end_matches('/');
        let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attrs = &body[name_end..];
        i = close + 1;
        if name.is_empty() || name.starts_with('!') || name.starts_with('?') {
            continue;
        }
        if !closing && matches!(name.as_str(), "script" | "style" | "head" | "title") {
            let end_tag = format!("</{name}");
            i = lower[i..].find(&end_tag).map_or(html.len(), |j| i + j);
            continue;
        }
        if name == "br" {
            out.new_para(true);
            continue;
        }
        let block = BLOCKS.contains(&name.as_str());
        if closing {
            // The stack is at most MAX_TAG_DEPTH deep, so this is bounded.
            if let Some(at) = stack.iter().rposition(|s| s.0 == name) {
                stack.truncate(at);
            }
            if name == "pre" {
                pre = pre.saturating_sub(1);
            }
            if block {
                out.new_para(false);
            }
            continue;
        }
        if block {
            out.new_para(false);
        }
        if name == "pre" {
            pre += 1;
        }
        if stack.len() >= MAX_TAG_DEPTH {
            continue;
        }
        let mut fmt = stack.last().map(|s| s.1.clone()).unwrap_or_default();
        match name.as_str() {
            "b" | "strong" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => fmt.bold = true,
            "i" | "em" | "cite" => fmt.italic = true,
            "u" | "ins" => fmt.underline = true,
            _ => {}
        }
        if let Some(s) = attr(attrs, "style") {
            apply_style(&mut fmt, s);
        }
        if name == "font"
            && let Some(c) = attr(attrs, "color").and_then(parse_color)
        {
            fmt.color = Some(c);
        }
        stack.push((name, fmt));
    }
    let mut paras = out.paras;
    for p in &mut paras {
        if let Some(last) = p.last_mut() {
            let t = last.text.trim_end_matches(' ').len();
            last.text.truncate(t);
        }
        p.retain(|r| !r.text.is_empty());
    }
    while paras.len() > 1 && paras.last().is_some_and(Vec::is_empty) {
        paras.pop();
    }
    Reduced {
        paras,
        truncated: out.full,
    }
}

/// Whether HTML without a pubrs payload is small enough to reduce; bigger
/// pastes as plain text.
pub fn can_reduce(html: &str) -> bool {
    html.len() <= MAX_REDUCE_HTML
}

/// The paragraphs' text, one line each.
pub fn rich_text(paras: &[Vec<RichRun>]) -> String {
    paras
        .iter()
        .map(|p| p.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

// ------------------------------------------------------------------ app

/// What a paste found.
enum Found {
    Frames(Box<ClipPayload>, bool),
    Image(ClipImage),
    Text(String),
    Nothing,
}

impl PubrsApp {
    /// The frames on the clipboard, from this process or another program.
    fn clipboard_frames(&mut self, html: Option<&str>) -> Option<(ClipPayload, bool)> {
        if let Some(p) = html.and_then(payload_from_html) {
            let same = self.clip.as_ref().is_some_and(|c| {
                c.doc == self.doc_id().unwrap_or(c.doc) && c.payload.roots == p.roots
            });
            return Some((p, same));
        }
        let ours = self.clip.clone()?;
        let text = self.platform.clipboard.get_text();
        let image = self.platform.clipboard.get_image();
        let untouched =
            text.as_deref() == Some(ours.marker.as_str()) || (text.is_none() && image.is_none());
        let same = Some(ours.doc) == self.doc_id();
        untouched.then_some((ours.payload, same))
    }

    fn find_on_clipboard(&mut self, editing: bool, html: Option<&str>) -> Found {
        if let Some((p, same)) = self.clipboard_frames(html) {
            return Found::Frames(Box::new(p), same);
        }
        if !editing && let Some(img) = self.platform.clipboard.get_image() {
            return Found::Image(img);
        }
        match self.platform.clipboard.get_text() {
            Some(t) if !t.is_empty() => Found::Text(t),
            _ => Found::Nothing,
        }
    }

    /// Copy the selected frames: pubrs payload and a picture as HTML, the
    /// frames' text as the plain-text fallback, and the payload itself kept
    /// in the app.
    pub fn copy_frames(&mut self) {
        let sel = self.selection();
        let Some(doc_id) = self.doc_id() else { return };
        if sel.is_empty() {
            return;
        }
        let payload = match self.session.clip_copy(Some(doc_id), &sel) {
            Ok(p) => p,
            Err(e) => {
                self.show_error(&e);
                return;
            }
        };
        let preview = self.selection_preview(doc_id, &payload);
        let mut alt = payload.plain_text();
        if alt.trim().is_empty() {
            alt = "pubrs frames".into();
        }
        match payload_to_html(&payload, preview.as_deref()) {
            Some(html) => self.platform.clipboard.set_html(&html, &alt),
            None => self.platform.clipboard.set_text(&alt),
        }
        self.clip = Some(InProcessClip {
            payload,
            marker: alt,
            doc: doc_id,
        });
        self.notify("Copied");
    }

    /// A PNG of the copied frames (their page, cropped), where one is cheap.
    fn selection_preview(&mut self, doc_id: DocumentId, payload: &ClipPayload) -> Option<Vec<u8>> {
        let doc = self.doc()?;
        let first = payload.roots.first()?;
        let page = match doc.owner_of(*first)? {
            pubrs_api::core::Owner::Page(p) => doc.page_index(p)?,
            pubrs_api::core::Owner::Master(_) => return None,
        };
        let bounds = payload
            .frames
            .iter()
            .filter(|f| payload.roots.contains(&f.id))
            .map(|f| f.bounds())
            .reduce(|a, b| a.union(&b))?;
        let dpi = 96.0;
        let opts = pubrs_api::render::raster::RasterOptions {
            dpi,
            include_bleed: false,
            background: Some(Color::WHITE),
        };
        let (_, _, png) = self
            .session
            .page_png(Some(doc_id), page, &opts, false)
            .ok()?;
        let s = dpi / 72.0;
        pubrs_api::io::crop_png(
            &png,
            (bounds.x * s).max(0.0) as u32,
            (bounds.y * s).max(0.0) as u32,
            (bounds.width * s).ceil() as u32,
            (bounds.height * s).ceil() as u32,
        )
        .ok()
    }

    /// Paste when no text is being edited: frames, a picture, or text in a
    /// new text frame.
    pub fn paste_frames(&mut self) {
        let html = self.platform.clipboard.get_html();
        match self.find_on_clipboard(false, html.as_deref()) {
            Found::Frames(p, same) => self.paste_payload(&p, same),
            Found::Image(img) => self.paste_image(img),
            Found::Text(t) => self.paste_text_frame(&t),
            Found::Nothing => self.notify("Nothing to paste"),
        }
    }

    /// Paste while editing text. `event_text` is what the windowing layer
    /// read from the clipboard, used if the platform clipboard has nothing.
    pub fn paste_into_text(&mut self, e: &TextEdit, event_text: Option<&str>) {
        let html = self.platform.clipboard.get_html();
        match self.find_on_clipboard(true, html.as_deref()) {
            // Inside text, copied frames paste as their text (the frames
            // paste only when no text is being edited).
            Found::Frames(p, _) => {
                let text = self.capped(p.plain_text());
                if text.is_empty() {
                    self.notify("The copied frames have no text to paste");
                } else {
                    crate::canvas::edit_insert(self, e, &text);
                }
                return;
            }
            Found::Image(_) | Found::Text(_) | Found::Nothing => {}
        }
        if let Some(h) = html.as_deref() {
            if !can_reduce(h) {
                self.notify(
                    "The copied page is too big to keep its formatting; pasted as plain text",
                );
            } else {
                let r = html_to_paragraphs(h);
                if !rich_text(&r.paras).trim().is_empty() {
                    self.insert_rich(e, &r.paras);
                    if r.truncated {
                        self.notify(format!(
                            "Pasted the first {MAX_REDUCED_CHARS} characters; the rest was left out"
                        ));
                    }
                    return;
                }
            }
        }
        let text = self
            .platform
            .clipboard
            .get_text()
            .filter(|t| !t.is_empty())
            .or_else(|| event_text.map(str::to_string));
        match text {
            Some(t) => {
                let t = self.capped(t);
                crate::canvas::edit_insert(self, e, &t)
            }
            None => self.notify("Nothing to paste"),
        }
    }

    /// `text` cut to [`MAX_PASTE_CHARS`], with a status note if it was.
    fn capped(&mut self, text: String) -> String {
        match text.char_indices().nth(MAX_PASTE_CHARS) {
            Some((at, _)) => {
                self.notify(format!(
                    "Pasted the first {MAX_PASTE_CHARS} characters; the rest was left out"
                ));
                text[..at].to_string()
            }
            None => text,
        }
    }

    /// Insert paragraphs of runs at the selection as one undo step.
    pub fn insert_rich(&mut self, e: &TextEdit, paras: &[Vec<RichRun>]) {
        use pubrs_api::core::Command;
        let text = rich_text(paras);
        let n = text.chars().count();
        let (s, t) = e.range();
        let mut commands = vec![if s != t {
            Command::ReplaceText {
                story: e.story,
                start: s,
                end: t,
                text,
            }
        } else {
            Command::InsertText {
                story: e.story,
                at: s,
                text,
                format: None,
            }
        }];
        let mut at = s;
        for (k, p) in paras.iter().enumerate() {
            if k > 0 {
                at += 1;
            }
            for r in p {
                let len = r.text.chars().count();
                if r.props != CharProps::default() && len > 0 {
                    commands.push(Command::ApplyCharProps {
                        story: e.story,
                        start: at,
                        end: at + len,
                        props: r.props.clone(),
                    });
                }
                at += len;
            }
        }
        if self.exec(Command::Batch {
            label: "paste".into(),
            commands,
        }) && let Some(v) = self.view_mut()
            && let Some(ed) = &mut v.edit
        {
            ed.set(s + n, false);
            ed.goal_x = None;
        }
    }

    /// Paste copied frames onto the current page.
    pub fn paste_payload(&mut self, payload: &ClipPayload, same_doc: bool) {
        let Some(page) = self.page_owner().and_then(|o| match o {
            pubrs_api::core::Owner::Page(p) => Some(p),
            pubrs_api::core::Owner::Master(_) => None,
        }) else {
            return;
        };
        let nudge = if same_doc { 18.0 } else { 0.0 };
        let doc_id = self.doc_id();
        let r = self.session.clip_paste(
            doc_id,
            payload,
            PasteAt {
                parent: Parent::Page(page),
                dx: nudge,
                dy: nudge,
            },
        );
        if let Some(out) = self.report(r) {
            let n = out.frame_ids.len();
            self.select(out.frame_ids);
            self.notify(if n == 1 {
                "Pasted 1 frame".to_string()
            } else {
                format!("Pasted {n} frames")
            });
        }
    }

    /// The page the next pasted frame lands on, and the room it has there.
    fn paste_area(&self) -> Option<(PageId, f64, f64)> {
        let doc = self.doc()?;
        let page = doc.pages.get(self.view()?.page)?;
        Some((page.id, doc.setup.size.width, doc.setup.size.height))
    }

    /// Paste a clipboard picture as a picture frame, scaled down above
    /// [`pubrs_api::io::PASTE_MAX_SIDE`] pixels.
    pub fn paste_image(&mut self, img: ClipImage) {
        let Some((page, pw, ph)) = self.paste_area() else {
            return;
        };
        let (w, h) = (img.width as u32, img.height as u32);
        let png = match pubrs_api::io::rgba_to_png(img.rgba, w, h, pubrs_api::io::PASTE_MAX_SIDE) {
            Ok(p) => p,
            Err(e) => {
                self.notify(format!("Cannot paste the picture: {e}"));
                return;
            }
        };
        // 96 pixels to the inch, shrunk to fit inside the page.
        let (mut fw, mut fh) = (f64::from(w) * 0.75, f64::from(h) * 0.75);
        let k = (pw * 0.8 / fw).min(ph * 0.8 / fh).min(1.0);
        fw = (fw * k).max(12.0);
        fh = (fh * k).max(12.0);
        let doc_id = self.doc_id();
        let r = self.session.frame_add_image(FrameAddImage {
            doc_id,
            page_id: Some(page),
            master_id: None,
            x: (pw - fw) / 2.0,
            y: (ph - fh) / 2.0,
            width: fw,
            height: fh,
            path: None,
            base64: Some(B64.encode(&png.png)),
            svg: None,
            filename: Some("Pasted image.png".into()),
            fit: None,
            name: None,
        });
        if let Some(out) = self.report(r) {
            self.select(vec![out.frame_id]);
            if png.downscaled {
                self.notify(format!(
                    "Pasted picture; it was {}x{} pixels and was scaled down to {}x{} (limit {})",
                    png.original.0,
                    png.original.1,
                    png.width,
                    png.height,
                    pubrs_api::io::PASTE_MAX_SIDE
                ));
            } else {
                self.notify("Pasted picture");
            }
        }
    }

    /// Plain text with no text being edited: a new text frame.
    fn paste_text_frame(&mut self, text: &str) {
        let text = self.capped(text.to_string());
        let text = text.as_str();
        let Some((page, pw, ph)) = self.paste_area() else {
            return;
        };
        let text = text.replace("\r\n", "\n");
        let width = (pw - 144.0).clamp(72.0, 360.0);
        let lines = text.lines().count().max(1) as f64 + (text.len() as f64 / 60.0).floor();
        let height = (lines * 18.0 + 12.0).clamp(36.0, (ph - 72.0).max(36.0));
        let doc_id = self.doc_id();
        let r = self.session.frame_add_text(FrameAddText {
            doc_id,
            page_id: Some(page),
            master_id: None,
            x: (pw - width) / 2.0,
            y: 72.0_f64.min(ph - height).max(0.0),
            width,
            height,
            text: Some(text),
            markup: false,
            style: None,
            font_family: None,
            font_size: None,
            color: None,
            align: None,
            columns: None,
            name: None,
        });
        if let Some(out) = self.report(r) {
            let ids: Vec<FrameId> = vec![out.frame_id];
            self.select(ids);
            self.notify("Pasted text in a new text frame");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paras(h: &str) -> Vec<Vec<RichRun>> {
        html_to_paragraphs(h).paras
    }

    fn flat(p: &[Vec<RichRun>]) -> Vec<(String, bool, bool, bool, Option<f64>)> {
        p.iter()
            .flatten()
            .map(|r| {
                (
                    r.text.clone(),
                    r.props.bold == Some(true),
                    r.props.italic == Some(true),
                    r.props.underline == Some(true),
                    r.props.font_size,
                )
            })
            .collect()
    }

    #[test]
    fn html_reduces_to_runs() {
        let p = paras(
            "<html><head><style>p{color:red}</style></head><body><p>Hello <b>big <i>bold</i></b> \
             <span style=\"font-size: 18px; color:#ff0000\">red</span> &amp; <u>u</u></p><p>Next</p></body></html>",
        );
        assert_eq!(p.len(), 2);
        assert_eq!(
            flat(&p),
            vec![
                ("Hello ".into(), false, false, false, None),
                ("big ".into(), true, false, false, None),
                ("bold".into(), true, true, false, None),
                (" ".into(), false, false, false, None),
                ("red".into(), false, false, false, Some(13.5)),
                (" & ".into(), false, false, false, None),
                ("u".into(), false, false, true, None),
                ("Next".into(), false, false, false, None),
            ]
        );
        assert_eq!(
            p[0][4].props.color,
            Some(Paint::Color(Color::rgb(255, 0, 0)))
        );
        assert_eq!(rich_text(&p), "Hello big bold red & u\nNext");
    }

    #[test]
    fn html_is_tolerant() {
        let p = paras("a<br>b<unknown>c</oops><b>d");
        assert_eq!(rich_text(&p), "a\nbcd");
        let q = paras("<!--x--><div><div>one</div></div><div>two&#33;&#x41;</div>");
        assert_eq!(rich_text(&q), "one\ntwo!A");
        assert_eq!(rich_text(&paras("5 < 6 & more")), "5 < 6 & more");
    }

    #[test]
    fn colour_and_size_parsing() {
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(parse_color("#0f0"), Some(Color::rgb(0, 255, 0)));
        assert_eq!(parse_color("#é12"), None);
        assert_eq!(parse_size("12pt"), Some(12.0));
        assert_eq!(parse_size("2em"), None);
        assert_eq!(parse_size("9999pt"), None);
    }

    #[test]
    fn hostile_html_is_cheap_and_bounded() {
        let t0 = std::time::Instant::now();
        // A megabyte of ampersands: linear, nothing decoded.
        let amps = "&".repeat(1 << 20);
        let r = html_to_paragraphs(&amps);
        assert_eq!(rich_text(&r.paras).chars().count(), MAX_REDUCED_CHARS);
        assert!(r.truncated);
        // Entities that never end.
        let r = html_to_paragraphs(&"&#x".repeat(300_000));
        assert!(r.truncated || rich_text(&r.paras).len() <= 3 * 300_000);
        // 100,000 nested bold tags: formatting stops following at the cap.
        let nest = format!("{}x{}", "<b>".repeat(100_000), "</b>".repeat(100_000));
        let r = html_to_paragraphs(&nest);
        assert_eq!(rich_text(&r.paras), "x");
        // Runs: alternating formatting is capped at MAX_REDUCED_RUNS.
        let many = "<b>a</b>b".repeat(40_000);
        let r = html_to_paragraphs(&many);
        let runs: usize = r.paras.iter().map(Vec::len).sum();
        assert!(runs <= MAX_REDUCED_RUNS + 1 && r.truncated, "{runs}");
        // A million line breaks.
        let r = html_to_paragraphs(&"<br>".repeat(1 << 20));
        assert!(r.paras.len() <= MAX_REDUCED_CHARS + 1);
        assert!(t0.elapsed().as_secs_f64() < 2.0, "{:?}", t0.elapsed());
    }

    #[test]
    fn big_html_without_a_payload_is_not_reduced() {
        let t0 = std::time::Instant::now();
        let big = format!("<p>{}</p>", "word ".repeat(1_100_000));
        assert!(big.len() > 5 * 1024 * 1024 && !can_reduce(&big));
        assert!(can_reduce("<b>x</b>"));
        assert!(t0.elapsed().as_secs_f64() < 2.0);
    }

    #[test]
    fn payload_html_is_bounded_before_decoding() {
        let huge = format!("<!--pubrs:{}-->", "A".repeat(MAX_CLIP_BASE64 + 4));
        assert!(payload_from_html(&huge).is_none());
        assert!(payload_from_html("<!--pubrs:not base64!-->").is_none());
        assert!(payload_from_html("plain").is_none());
    }
}
