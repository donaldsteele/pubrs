//! PDF/X-4: the output intent, the PDF/X version in the document information and XMP, written
//! as an incremental update of krilla's PDF 1.6 output; and a check of the requirements this
//! exporter is responsible for.
//!
//! Ported from DesignCraft (storytold/designcraft) at commit 16a7365, licensed MIT OR
//! Apache-2.0, "ArtCraft Team and DesignCraft contributors":
//! `crates/pdf/src/pdfx.rs` (`make_pdfx4`, `check_pdfx4`, the RGB walk) and the byte-level
//! helpers of `crates/pdf/src/transitions.rs` (`find`, `rfind`, `int_after`, `object`).
//! Changes: the profile and output condition are parameters, the checker is `check`, the
//! transparency-group rewrite is dropped (krilla writes ICC-based groups here), and the tests
//! are rewritten against pubrs (see `crates/pubrs-io/tests/pdf_export.rs`). See ASSETS.md and
//! docs/adr/0018-pdfx.md.

use std::collections::HashSet;

use hayro_syntax::content::UntypedIter;
use hayro_syntax::object::{Array, Dict, Name, Object, ObjectIdentifier, Stream};
use hayro_syntax::page::Resources;

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The integer after `key` in `s` (e.g. `/Size 14`, `/Root 13 0 R`).
fn int_after(s: &str, key: &str) -> Option<usize> {
    let i = s.find(key)? + key.len();
    let rest = s[i..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Byte range of object `id`'s body (between `obj` and `endobj`), the latest copy.
fn object(pdf: &[u8], id: usize) -> Option<std::ops::Range<usize>> {
    let head = format!("\n{id} 0 obj");
    let start = rfind(pdf, head.as_bytes())? + head.len();
    let end = find(pdf, b"endobj", start)?;
    Some(start..end)
}

/// Escape a PDF literal string.
fn lit(s: &str) -> String {
    let mut o = String::from("(");
    for c in s.chars() {
        if matches!(c, '(' | ')' | '\\') {
            o.push('\\');
        }
        o.push(c);
    }
    o.push(')');
    o
}

/// Turn krilla's output into PDF/X-4. `None` when the file isn't the simple kind this
/// understands (a classic cross-reference table and plain dictionaries).
pub fn make_pdfx4(pdf: &[u8], title: &str, icc: &[u8], cond: &str) -> Option<Vec<u8>> {
    let t_at = rfind(pdf, b"trailer")?;
    let trailer = String::from_utf8_lossy(&pdf[t_at..]).to_string();
    let size = int_after(&trailer, "/Size")?;
    let root = int_after(&trailer, "/Root")?;
    let prev = int_after(&trailer, "startxref")?;
    let info = int_after(&trailer, "/Info");
    let id = trailer.find("/ID").and_then(|i| {
        trailer[i..]
            .find(']')
            .map(|j| trailer[i..i + j + 1].to_string())
    });
    let cat = String::from_utf8_lossy(&pdf[object(pdf, root)?]).to_string();
    let mut out = pdf.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let mut entries: Vec<(usize, usize)> = Vec::new();
    let mut put = |out: &mut Vec<u8>, id: usize, body: &[u8]| {
        entries.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // The output profile and the output intent.
    let (icc_id, oi_id) = (size, size + 1);
    let mut s = format!("<</N 4/Length {}>>\nstream\n", icc.len()).into_bytes();
    s.extend_from_slice(icc);
    s.extend_from_slice(b"\nendstream");
    put(&mut out, icc_id, &s);
    put(
        &mut out,
        oi_id,
        format!(
            "<</Type/OutputIntent/S/GTS_PDFX/OutputConditionIdentifier{}/OutputCondition{}/Info{}/RegistryName(http://www.color.org)/DestOutputProfile {icc_id} 0 R>>",
            lit("Custom"),
            lit(cond),
            lit(cond)
        )
        .as_bytes(),
    );
    // The catalog with the intent.
    let close = cat.rfind(">>")?;
    let cat_new = format!(
        "{}/OutputIntents[{oi_id} 0 R]{}",
        cat[..close].trim(),
        &cat[close..]
    );
    put(&mut out, root, cat_new.trim().as_bytes());
    // Document information: the PDF/X version and trapping state.
    let mut next = size + 2;
    let info_id = match info {
        Some(i) => {
            let body = String::from_utf8_lossy(&pdf[object(pdf, i)?]).to_string();
            let close = body.rfind(">>")?;
            put(
                &mut out,
                i,
                format!(
                    "{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False{}",
                    body[..close].trim(),
                    &body[close..]
                )
                .trim()
                .as_bytes(),
            );
            i
        }
        None => {
            let i = next;
            next += 1;
            put(
                &mut out,
                i,
                format!(
                    "<</Title{}/GTS_PDFXVersion(PDF/X-4)/Trapped/False>>",
                    lit(title)
                )
                .as_bytes(),
            );
            i
        }
    };
    // XMP: the PDF/X identification schema, and the trapping state that Info also states.
    if let Some(meta) = int_after(&cat, "/Metadata") {
        let r = object(pdf, meta)?;
        let body = &pdf[r];
        let s0 = find(body, b"stream", 0)? + b"stream".len();
        let s0 = s0
            + body[s0..]
                .iter()
                .take_while(|c| **c == b'\r' || **c == b'\n')
                .count();
        let s1 = rfind(body, b"endstream")?;
        let xml = String::from_utf8_lossy(&body[s0..s1])
            .trim_end()
            .to_string();
        let dict = String::from_utf8_lossy(&body[..find(body, b"stream", 0)?]).to_string();
        if dict.contains("/Filter") {
            return None;
        }
        let add = concat!(
            "<rdf:Description rdf:about=\"\" xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\"><pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion></rdf:Description>",
            "<rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"><pdf:Trapped>False</pdf:Trapped></rdf:Description>"
        );
        let xml = xml.replacen("</rdf:RDF>", &format!("{add}</rdf:RDF>"), 1);
        let dict = {
            let i = dict.find("/Length")?;
            let rest = &dict[i + 7..];
            let rest = rest.trim_start();
            let end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            format!("{}/Length {}{}", &dict[..i], xml.len(), &rest[end..])
        };
        let mut b = dict.trim().as_bytes().to_vec();
        b.extend_from_slice(b"\nstream\n");
        b.extend_from_slice(xml.as_bytes());
        b.extend_from_slice(b"\nendstream");
        put(&mut out, meta, &b);
    }
    // Cross-reference section and trailer.
    let xref_at = out.len();
    entries.sort();
    let mut x = String::from("xref\n");
    for (oid, off) in &entries {
        x.push_str(&format!("{oid} 1\n{off:010} 00000 n\r\n"));
    }
    let id = id.unwrap_or_default();
    x.push_str(&format!("trailer\n<</Size {next}/Root {root} 0 R/Info {info_id} 0 R{id}/Prev {prev}>>\nstartxref\n{xref_at}\n%%EOF\n"));
    out.extend_from_slice(x.as_bytes());
    Some(out)
}

/// PDF/X-4 checks on an exported file: what's missing or not allowed, device RGB under the CMYK
/// output intent included (empty = passes these checks).
pub fn check(pdf: &[u8]) -> Vec<String> {
    let mut issues = Vec::new();
    let text = String::from_utf8_lossy(pdf);
    if !pdf.starts_with(b"%PDF-1.") || pdf.get(7).is_none_or(|v| *v > b'6') {
        issues.push("the PDF version must be 1.6 or lower".into());
    }
    if !text.contains("/S/GTS_PDFX") {
        issues.push("no PDF/X output intent".into());
    }
    if !text.contains("/DestOutputProfile") {
        issues.push("the output intent has no destination profile".into());
    }
    if !text.contains("/GTS_PDFXVersion(PDF/X-4)") {
        issues.push("the document information doesn't name PDF/X-4".into());
    }
    if !text.contains("pdfxid:GTS_PDFXVersion>PDF/X-4<") {
        issues.push("the XMP metadata doesn't name PDF/X-4".into());
    }
    if text.contains("/Interpolate true") {
        issues.push("an image asks for interpolation".into());
    }
    if text.contains("/Encrypt") {
        issues.push("the file is encrypted".into());
    }
    let pages = text.matches("/Type/Page/").count() + text.matches("/Type/Page>").count();
    let trims = text.matches("/TrimBox").count();
    if pages > 0 && trims < pages {
        issues.push("every page needs a trim box".into());
    }
    if text.contains("/Subtype/Type3") {
        issues.push("Type 3 fonts aren't allowed".into());
    }
    issues.extend(device_rgb(pdf));
    issues
}

/// Device-dependent RGB under a CMYK output intent. PDF/X-4 (ISO 15930-7) lets RGB in only when
/// it is colour-managed: ICCBased RGB, or DeviceRGB remapped by a `DefaultRGB` colour space in the
/// resources. DeviceCMYK and DeviceGray are the output intent's own device values, so they stay.
/// This walks every page's content, the images, forms, patterns, shadings and soft masks it uses,
/// and the transparency groups, and reports each kind of device RGB it finds with the pages it is
/// on. Nothing is reported when the file has no CMYK PDF/X output intent.
fn device_rgb(pdf: &[u8]) -> Vec<String> {
    let Ok(file) = hayro_syntax::Pdf::new(pdf.to_vec()) else {
        return vec!["the file couldn't be read to check its colour spaces".into()];
    };
    if !cmyk_output_intent(&file) {
        return Vec::new();
    }
    let mut found: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, page) in file.pages().iter().enumerate() {
        let mut scan = RgbScan::default();
        let res = page.resources();
        if let Some(group) = page.raw().get::<Dict<'_>>(b"Group") {
            scan.group(&group, res);
        }
        if let Some(content) = page.page_stream() {
            scan.content(content, res, 0);
        }
        for what in scan.found {
            match found.iter_mut().find(|(w, _)| *w == what) {
                Some((_, pages)) => pages.push(i + 1),
                None => found.push((what, vec![i + 1])),
            }
        }
    }
    found
        .into_iter()
        .map(|(what, pages)| {
            let list: Vec<String> = pages.iter().take(10).map(|p| p.to_string()).collect();
            let more = if pages.len() > 10 { ", …" } else { "" };
            let s = if pages.len() == 1 { "" } else { "s" };
            format!(
                "{what} on page{s} {}{more}: under a CMYK output intent RGB needs an ICC profile",
                list.join(", ")
            )
        })
        .collect()
}

/// Whether the file's PDF/X output intent has a four-channel (CMYK) destination profile.
fn cmyk_output_intent(file: &hayro_syntax::Pdf) -> bool {
    let xref = file.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else {
        return false;
    };
    let Some(intents) = catalog.get::<Array<'_>>(b"OutputIntents") else {
        return false;
    };
    intents.iter::<Dict<'_>>().any(|oi| {
        oi.get::<Name<'_>>(b"S")
            .is_some_and(|s| s.as_ref() == b"GTS_PDFX")
            && oi
                .get::<Stream<'_>>(b"DestOutputProfile")
                .and_then(|p| p.dict().get::<i32>(b"N"))
                == Some(4)
    })
}

/// Whether a colour space is device RGB: `/DeviceRGB` (`/RGB` in inline images), or an
/// `/Indexed` space over it.
fn is_device_rgb(cs: &Object<'_>) -> bool {
    match cs {
        Object::Name(n) => matches!(n.as_ref(), b"DeviceRGB" | b"RGB"),
        Object::Array(a) => {
            let mut items = a.iter::<Object<'_>>();
            match items.next() {
                Some(Object::Name(n)) if matches!(n.as_ref(), b"Indexed" | b"I") => {
                    // The base space is a name or a (non-indexed) array; no deeper nesting.
                    matches!(items.next(), Some(Object::Name(b)) if matches!(b.as_ref(), b"DeviceRGB" | b"RGB"))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// How deep forms, patterns and soft masks may nest before the walk stops.
const MAX_DEPTH: usize = 16;

/// What one page uses in device RGB, in the order found.
#[derive(Default)]
struct RgbScan {
    found: Vec<String>,
    /// Forms, images and patterns already looked at on this page.
    seen: HashSet<ObjectIdentifier>,
}

impl RgbScan {
    fn add(&mut self, what: String) {
        if !self.found.contains(&what) {
            self.found.push(what);
        }
    }

    /// `DefaultRGB` in scope makes DeviceRGB colour-managed.
    fn managed(res: &Resources<'_>) -> bool {
        res.get_color_space(&Name::new_unescaped(b"DefaultRGB"))
            .is_some()
    }

    /// A content stream (a page's or a form's) with its resources.
    fn content(&mut self, data: &[u8], res: &Resources<'_>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let managed = Self::managed(res);
        let mut ops = UntypedIter::new(data);
        while let Some(op) = ops.next() {
            let operator: &[u8] = op.operator;
            let last = op.operands().last().cloned();
            let name = || last.clone().and_then(Object::into_name);
            match operator {
                b"rg" | b"RG" if !managed => self.add("RGB fills or strokes".into()),
                b"cs" | b"CS" if !managed => {
                    let rgb = name().is_some_and(|n| {
                        n.as_ref() == b"DeviceRGB"
                            || res.get_color_space(&n).is_some_and(|cs| is_device_rgb(&cs))
                    });
                    if rgb {
                        self.add("RGB fills or strokes".into());
                    }
                }
                b"sh" => {
                    if let Some(sh) = name().and_then(|n| res.get_shading(&n)) {
                        self.shading(sh, managed);
                    }
                }
                b"scn" | b"SCN" => {
                    if let Some(p) = name().and_then(|n| res.get_pattern(&n)) {
                        self.pattern(p, res, managed, depth);
                    }
                }
                b"gs" => {
                    let mask = name()
                        .and_then(|n| res.get_ext_g_state(&n))
                        .and_then(|gs| gs.get::<Dict<'_>>(b"SMask"));
                    if let Some(form) = mask.and_then(|m| m.get::<Stream<'_>>(b"G")) {
                        self.form(&form, res, depth);
                    }
                }
                b"Do" => {
                    if let Some(xo) = name().and_then(|n| res.get_x_object(&n)) {
                        match xo.dict().get::<Name<'_>>(b"Subtype").as_deref() {
                            Some(b"Image") => self.image(&xo, managed),
                            Some(b"Form") => self.form(&xo, res, depth),
                            _ => {}
                        }
                    }
                }
                b"BI" | b"EI" if !managed => {
                    if let Some(Object::Stream(img)) = last {
                        let cs = img
                            .dict()
                            .get::<Object<'_>>(b"CS")
                            .or_else(|| img.dict().get::<Object<'_>>(b"ColorSpace"));
                        let cs = match cs {
                            Some(Object::Name(n))
                                if !matches!(
                                    n.as_ref(),
                                    b"DeviceRGB"
                                        | b"RGB"
                                        | b"G"
                                        | b"DeviceGray"
                                        | b"CMYK"
                                        | b"DeviceCMYK"
                                ) =>
                            {
                                res.get_color_space(&n)
                            }
                            cs => cs,
                        };
                        if cs.is_some_and(|cs| is_device_rgb(&cs)) {
                            self.add("an RGB inline image".into());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn image(&mut self, img: &Stream<'_>, managed: bool) {
        let id = img.obj_id();
        if managed || !self.seen.insert(id) || img.dict().get::<bool>(b"ImageMask") == Some(true) {
            return;
        }
        if img
            .dict()
            .get::<Object<'_>>(b"ColorSpace")
            .is_some_and(|cs| is_device_rgb(&cs))
        {
            let (w, h) = (
                img.dict().get::<i32>(b"Width").unwrap_or(0),
                img.dict().get::<i32>(b"Height").unwrap_or(0),
            );
            self.add(format!(
                "an RGB image (object {}, {w} × {h} px)",
                id.obj_number
            ));
        }
    }

    /// A form XObject (placed PDF page, group, soft mask): its group, then its content.
    fn form(&mut self, form: &Stream<'_>, parent: &Resources<'_>, depth: usize) {
        if depth >= MAX_DEPTH || !self.seen.insert(form.obj_id()) {
            return;
        }
        let res = form
            .dict()
            .get::<Dict<'_>>(b"Resources")
            .map_or_else(|| parent.clone(), Resources::new);
        if let Some(group) = form.dict().get::<Dict<'_>>(b"Group") {
            self.group(&group, &res);
        }
        if let Ok(data) = form.decoded() {
            self.content(&data, &res, depth + 1);
        }
    }

    fn group(&mut self, group: &Dict<'_>, res: &Resources<'_>) {
        if !Self::managed(res)
            && group
                .get::<Object<'_>>(b"CS")
                .is_some_and(|cs| is_device_rgb(&cs))
        {
            self.add("a transparency group blending in RGB".into());
        }
    }

    /// A shading dictionary or stream (gradients).
    fn shading(&mut self, sh: Object<'_>, managed: bool) {
        let dict = match sh {
            Object::Dict(d) => d,
            Object::Stream(s) => s.dict().clone(),
            _ => return,
        };
        if !managed
            && dict
                .get::<Object<'_>>(b"ColorSpace")
                .is_some_and(|cs| is_device_rgb(&cs))
        {
            self.add("RGB gradients".into());
        }
    }

    /// A shading pattern (its shading) or a tiling pattern (its cell, like a form).
    fn pattern(&mut self, p: Object<'_>, res: &Resources<'_>, managed: bool, depth: usize) {
        match p {
            Object::Dict(d) => {
                if let Some(sh) = d.get::<Object<'_>>(b"Shading") {
                    self.shading(sh, managed);
                }
            }
            Object::Stream(s) => self.form(&s, res, depth),
            _ => {}
        }
    }
}
