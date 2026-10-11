//! The tool registry: every operation clients can call by name with JSON.
//! The MCP server and `pubrs script` use this list 1:1; the GUI calls the
//! typed `Session` methods behind it.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::session::Session;
use crate::tools::{
    color, doc, export, find, frames, history, images, pages, query, sheets, styles, templates,
    text,
};

/// One tool.
pub struct ToolDef {
    pub name: &'static str,
    pub group: &'static str,
    /// Description written for an LLM reader.
    pub description: &'static str,
    /// Does not modify any document or file.
    pub read_only: bool,
    /// May delete user data; requires `confirm: true`.
    pub destructive: bool,
    /// Output field holding a base64 PNG to show as an image, if any.
    pub image_field: Option<&'static str>,
    pub input_schema: fn() -> Value,
    pub output_schema: fn() -> Value,
    pub call: fn(&mut Session, Value) -> ApiResult<Value>,
}

impl std::fmt::Debug for ToolDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

/// JSON Schema (draft 2020-12, subschemas inlined) for a Rust type.
pub fn schema_of<T: JsonSchema>() -> Value {
    let settings = SchemaSettings::draft2020_12().with(|s| {
        s.inline_subschemas = true;
    });
    let schema = settings.into_generator().into_root_schema_for::<T>();
    let mut v = serde_json::to_value(schema).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
        if !o.contains_key("type") {
            o.insert("type".into(), Value::String("object".into()));
        }
        o.entry("properties")
            .or_insert_with(|| Value::Object(Default::default()));
    }
    v
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> ApiResult<T> {
    let v = if v.is_null() {
        Value::Object(Default::default())
    } else {
        v
    };
    serde_json::from_value(v).map_err(|e| {
        ApiError::invalid(
            format!("invalid arguments: {e}"),
            "check field names and types against the tool's input schema",
        )
    })
}

fn to_value<T: serde::Serialize>(t: T) -> ApiResult<Value> {
    serde_json::to_value(t).map_err(|e| ApiError::internal(e.to_string()))
}

macro_rules! tool {
    ($name:literal, $group:literal, $method:ident, $in:ty => $out:ty,
     ro = $ro:literal, destructive = $de:literal, image = $img:expr, $desc:literal) => {
        ToolDef {
            name: $name,
            group: $group,
            description: $desc,
            read_only: $ro,
            destructive: $de,
            image_field: $img,
            input_schema: schema_of::<$in>,
            output_schema: schema_of::<$out>,
            call: |s, v| {
                let input: $in = parse(v)?;
                to_value(s.$method(input)?)
            },
        }
    };
}

use crate::session::Changed;

/// Every tool, in documentation order.
pub static TOOLS: &[ToolDef] = &[
    // ---------------------------------------------------------------- document
    tool!("doc_new", "Document", doc_new, doc::DocNew => doc::DocNewOut, ro = false, destructive = false, image = None,
        "Create a new empty document and make it current. Sizes are in points (72 per inch). Defaults: letter portrait, 36pt margins, no bleed, 1 page. Returns doc_id and page ids."),
    tool!("doc_open", "Document", doc_open, doc::DocOpen => doc::DocOpenOut, ro = false, destructive = false, image = None,
        "Open a .pubrs file from disk and make it current. Returns a new doc_id for this session."),
    tool!("doc_save", "Document", doc_save, doc::DocRef => doc::DocSaveOut, ro = false, destructive = false, image = None,
        "Save the document to the path it was opened from or last saved to. Use doc_save_as for new documents."),
    tool!("doc_save_as", "Document", doc_save_as, doc::DocSaveAs => doc::DocSaveOut, ro = false, destructive = false, image = None,
        "Save the document as a .pubrs file at a new path (the extension is added if missing). The file is written atomically."),
    tool!("doc_close", "Document", doc_close, doc::DocClose => doc::DocCloseOut, ro = false, destructive = true, image = None,
        "Close a document. If it has unsaved changes, confirm: true is required and the changes are discarded."),
    tool!("doc_list", "Document", doc_list, doc::DocRef => doc::DocListOut, ro = true, destructive = false, image = None,
        "List open documents with their ids, titles, paths, revision and unsaved-changes flag."),
    tool!("doc_info", "Document", doc_info, doc::DocRef => doc::DocInfoOut, ro = true, destructive = false, image = None,
        "Describe a document: page size, margins and bleed (points), page ids, master pages, counts of frames, stories, styles and images."),
    tool!("doc_set_page_size", "Document", doc_set_page_size, doc::DocSetPageSize => Changed, ro = false, destructive = false, image = None,
        "Change the trim size of every page: a preset (letter, a4, business_card, ...) with optional landscape orientation, or width/height in points. Frames are not moved."),
    tool!("doc_set_margins", "Document", doc_set_margins, doc::DocSetMargins => Changed, ro = false, destructive = false, image = None,
        "Set the margin guides in points: `all` and/or individual sides. Margins are guides only; nothing is clipped."),
    tool!("doc_set_bleed", "Document", doc_set_bleed, doc::DocSetBleed => Changed, ro = false, destructive = false, image = None,
        "Set the bleed in points (area outside the trim that is printed and cut off). 9pt (0.125in) is standard for print. Frames meant to bleed should extend to -bleed."),
    tool!("doc_set_info", "Document", doc_set_info, doc::DocSetInfo => Changed, ro = false, destructive = false, image = None,
        "Set the document title and/or author (written to exported PDFs)."),
    tool!("doc_set_grid", "Document", doc_set_grid, doc::DocSetGrid => Changed, ro = false, destructive = false, image = None,
        "Configure the layout grid and snapping: spacing in points, show_grid, snap_to_grid, snap_to_guides, snap_tolerance."),
    // ---------------------------------------------------------------- pages
    tool!("page_add", "Pages", page_add, pages::PageAdd => pages::PageAddOut, ro = false, destructive = false, image = None,
        "Add an empty page (optionally at a 0-based index and with a master page). Returns the new page_id."),
    tool!("page_delete", "Pages", page_delete, pages::PageDelete => Changed, ro = false, destructive = true, image = None,
        "Delete a page and every frame on it. Requires confirm: true. The last page cannot be deleted."),
    tool!("page_reorder", "Pages", page_reorder, pages::PageReorder => Changed, ro = false, destructive = false, image = None,
        "Move a page to a new 0-based position."),
    tool!("page_list", "Pages", page_list, doc::DocRef => pages::PageListOut, ro = true, destructive = false, image = None,
        "List pages in order with ids, master page and frame counts."),
    tool!("page_info", "Pages", page_info, pages::PageRef => pages::PageInfoOut, ro = true, destructive = false, image = None,
        "Describe one page: its frames (back to front, with kind, position in points, text preview) and guides."),
    tool!("master_create", "Pages", master_create, pages::MasterCreate => pages::MasterCreateOut, ro = false, destructive = false, image = None,
        "Create an empty master page. Add frames to it with frame_add_* and master_id; apply it with master_apply. Master frames draw beneath page frames."),
    tool!("master_apply", "Pages", master_apply, pages::MasterApply => Changed, ro = false, destructive = false, image = None,
        "Apply a master page to pages (default all), or pass master_id null to remove it."),
    tool!("master_delete", "Pages", master_delete, pages::MasterDelete => Changed, ro = false, destructive = true, image = None,
        "Delete a master page and its frames; pages using it lose their master. Requires confirm: true."),
    tool!("guide_add", "Pages", guide_add, pages::GuideAdd => pages::GuideAddOut, ro = false, destructive = false, image = None,
        "Add a ruler guide to a page or master: axis horizontal (position = y) or vertical (position = x), in points."),
    tool!("guide_move", "Pages", guide_move, pages::GuideMove => Changed, ro = false, destructive = false, image = None,
        "Move a ruler guide to a new position in points (found by guide_id; page_id or master_id optional)."),
    tool!("guide_remove", "Pages", guide_remove, pages::GuideRemove => Changed, ro = false, destructive = false, image = None,
        "Remove a ruler guide."),
    // ---------------------------------------------------------------- frames
    tool!("frame_add_text", "Frames", frame_add_text, frames::FrameAddText => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Add a text frame. Position/size in points from the page's top-left (y down). Optional initial text (\"\\n\" = new paragraph, markup: true for # headings and **bold**/*italic*), paragraph style, font, size, colour, alignment and columns. Returns frame_id and story_id."),
    tool!("frame_add_image", "Frames", frame_add_image, frames::FrameAddImage => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Add a picture frame, optionally with an image from a file path or base64 (PNG, JPEG, SVG). fit: fill (default, covers the frame), fit or stretch."),
    tool!("frame_add_shape", "Frames", frame_add_shape, frames::FrameAddShape => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Add a rectangle (optional corner_radius), ellipse or line. Lines run corner to corner of the box (height 0 = horizontal). fill/stroke are colours or \"none\"."),
    tool!("frame_delete", "Frames", frame_delete, frames::FrameDelete => Changed, ro = false, destructive = true, image = None,
        "Delete a frame (groups delete their members). Requires confirm: true. Locked frames must be unlocked first."),
    tool!("frame_move", "Frames", frame_move, frames::FrameMove => Changed, ro = false, destructive = false, image = None,
        "Move a frame: absolute x/y (top-left, points) or relative dx/dy. snap: true snaps to guides, margins, page edges and other frames."),
    tool!("frame_resize", "Frames", frame_resize, frames::FrameResize => Changed, ro = false, destructive = false, image = None,
        "Resize a frame to width/height in points, anchored at its top-left (default) or centre. Image frames keep their aspect ratio unless keep_aspect is false or a height is given."),
    tool!("frame_rotate", "Frames", frame_rotate, frames::FrameRotate => Changed, ro = false, destructive = false, image = None,
        "Set a frame's clockwise rotation in degrees about its centre (absolute, not relative)."),
    tool!("frame_set_zorder", "Frames", frame_set_zorder, frames::FrameSetZorder => Changed, ro = false, destructive = false, image = None,
        "Change stacking order: bring_to_front, send_to_back, bring_forward, send_backward, or an explicit index (0 = back)."),
    tool!("frame_group", "Frames", frame_group, frames::FrameGroup => frames::FrameGroupOut, ro = false, destructive = false, image = None,
        "Group two or more frames on the same page so they move together. Returns group_id."),
    tool!("frame_ungroup", "Frames", frame_ungroup, frames::FrameUngroup => frames::FrameUngroupOut, ro = false, destructive = false, image = None,
        "Dissolve a group; its members keep their positions."),
    tool!("frame_lock", "Frames", frame_lock, frames::FrameLock => Changed, ro = false, destructive = false, image = None,
        "Lock (locked: true) or unlock a frame. Locked frames cannot be moved, resized, rotated or deleted; their text can still be edited."),
    tool!("frame_align", "Frames", frame_align, frames::FrameAlign => Changed, ro = false, destructive = false, image = None,
        "Align frames' left/center/right/top/middle/bottom edges to the selection's bounds, the page or the margins."),
    tool!("frame_distribute", "Frames", frame_distribute, frames::FrameDistribute => Changed, ro = false, destructive = false, image = None,
        "Space frames evenly horizontally or vertically, within their own span (default), the page or the margins."),
    tool!("frame_set_style", "Frames", frame_set_style, frames::FrameSetStyle => Changed, ro = false, destructive = false, image = None,
        "Set a frame's fill colour, outline (stroke colour, width in points, dash solid/dashed/dotted), opacity (0-1) and display name. Use \"none\" to remove fill or outline."),
    tool!("frame_set_wrap", "Frames", frame_set_wrap, frames::FrameSetWrap => Changed, ro = false, destructive = false, image = None,
        "Set how text in frames below this one wraps around it: none, square (text flows beside it) or top_bottom, with a margin in points. Pictures default to square."),
    tool!("frame_set_placeholder", "Frames", frame_set_placeholder, frames::FrameSetPlaceholder => Changed, ro = false, destructive = false, image = None,
        "Mark a text or picture frame as a named template placeholder (e.g. \"headline\"), or pass name null to clear. Used when saving templates."),
    tool!("frame_list", "Frames", frame_list, frames::FrameList => frames::FrameListOut, ro = true, destructive = false, image = None,
        "List frames (all pages and masters, or one page/master), optionally only one kind: text, image, shape, group. Includes position, z-index, placeholder names and text previews."),
    tool!("frame_get", "Frames", frame_get, frames::FrameRef => frames::FrameGetOut, ro = true, destructive = false, image = None,
        "Get one frame in full (geometry, style, wrap, content), plus overflow for text frames and effective dpi for images."),
    tool!("frame_duplicate", "Frames", frame_duplicate, frames::FrameDuplicate => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Copy a frame (text frames get a copy of their text) offset by dx/dy points, optionally onto another page."),
    // ---------------------------------------------------------------- text
    tool!("text_set", "Text", text_set, text::TextSet => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Replace all text of a text frame's story (the whole linked chain). Keeps the existing first-paragraph formatting, so filling a template placeholder keeps its style. markup: true enables # headings and **bold**/*italic*/__underline__. Reports overflow."),
    tool!("text_insert", "Text", text_insert, text::TextInsert => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Insert text at a character offset (default: end). Offsets count characters; each paragraph break is one \"\\n\"."),
    tool!("text_replace_range", "Text", text_replace_range, text::TextReplaceRange => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Replace characters start..end (end exclusive) with new text; empty text deletes. The replaced text's formatting is kept."),
    tool!("text_apply_char_style", "Text", text_apply_char_style, text::TextApplyCharStyle => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Format characters start..end (default all): character style by name, font_family, font_size (points), bold, italic, underline, colour, letter_spacing. clear: true removes local formatting first."),
    tool!("text_apply_para_style", "Text", text_apply_para_style, text::TextApplyParaStyle => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Format paragraphs touching start..end (default all): paragraph style by name (e.g. \"Heading 1\", \"Body\"), align left/center/right/justify, line_spacing (multiple of font size), space_before/after and indents in points."),
    tool!("text_link_frames", "Text", text_link_frames, text::TextLinkFrames => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Link two text frames so text overflowing from_frame_id continues in to_frame_id (which must not already be linked). Reports whether the chain still overflows."),
    tool!("text_unlink_frames", "Text", text_unlink_frames, text::TextUnlinkFrames => text::TextUnlinkOut, ro = false, destructive = false, image = None,
        "Break the link after a frame. The text stays in the first part of the chain; later frames get a new empty story."),
    tool!("text_find", "Text", text_find, find::TextFind => find::TextFindOut, ro = true, destructive = false, image = None,
        "Find text in the whole document (every story) or one story. Options: case_sensitive, whole_word, and a start/end character range for one story. Returns story_id, the frame showing the match start, character offsets and a context snippet. An empty query is an error."),
    tool!("text_replace_all", "Text", text_replace_all, find::TextReplaceAll => find::TextReplaceAllOut, ro = false, destructive = false, image = None,
        "Replace every match of query with replacement across the document or one story (same options as text_find). One undo step. Returns the count; zero matches changes nothing. An empty query is an error."),
    tool!("text_get", "Text", text_get, text::TextRef => text::TextGetOut, ro = true, destructive = false, image = None,
        "Get a story's text with character offsets per paragraph and run, styles, effective fonts, the range each linked frame shows and how many characters overflow."),
    tool!("text_overflow_status", "Text", text_overflow_status, doc::DocRef => text::OverflowOut, ro = true, destructive = false, image = None,
        "Report every story: its frame chain, whether it overflows and how many characters are hidden. Fix overflow by enlarging the last frame, shortening text or linking another frame."),
    tool!("text_set_frame_options", "Text", text_set_frame_options, text::TextSetFrameOptions => Changed, ro = false, destructive = false, image = None,
        "Set a text frame's columns (1-12), column gap and inner inset in points, and vertical alignment (top/middle/bottom)."),
    // ---------------------------------------------------------------- styles
    tool!("style_list", "Styles", style_list, styles::StyleList => styles::StyleListOut, ro = true, destructive = false, image = None,
        "List paragraph and character styles with their properties and parents. Built-ins: Normal, Title, Heading 1, Heading 2, Body, Caption, Emphasis."),
    tool!("style_create", "Styles", style_create, styles::StyleCreate => styles::StyleCreateOut, ro = false, destructive = false, image = None,
        "Create a named paragraph (default) or character style, optionally based_on another style. Properties: paragraph {align, line_spacing, space_before, space_after, indents}, character {font_family, font_size, bold, italic, underline, color, letter_spacing}."),
    tool!("style_update", "Styles", style_update, styles::StyleUpdate => Changed, ro = false, destructive = false, image = None,
        "Rename a style, change its parent, or merge new paragraph/character properties into it. All text using it updates."),
    tool!("style_delete", "Styles", style_delete, styles::StyleDelete => Changed, ro = false, destructive = true, image = None,
        "Delete a style (not the default Normal style). Text using it falls back. Requires confirm: true."),
    // ---------------------------------------------------------------- colour
    tool!("swatch_list", "Colour", swatch_list, color::SwatchList => color::SwatchListOut, ro = true, destructive = false, image = None,
        "List the document's swatches (rgb, cmyk or spot) with their screen colour, where each is used, and the applied colour scheme. Use a swatch anywhere a colour is accepted as {\"swatch\": id, \"tint\": 0-100}."),
    tool!("swatch_create", "Colour", swatch_create, color::SwatchCreate => color::SwatchCreateOut, ro = false, destructive = false, image = None,
        "Create a named swatch: value {\"rgb\": \"#1E3A8A\"}, {\"cmyk\": {c,m,y,k}} (0-100) or {\"spot\": {name, cmyk}}. A spot colour must have a CMYK alternative. Returns swatch_id."),
    tool!("swatch_update", "Colour", swatch_update, color::SwatchUpdate => Changed, ro = false, destructive = false, image = None,
        "Rename a swatch or change its value; every frame fill, outline and text colour using it recolours at once."),
    tool!("swatch_delete", "Colour", swatch_delete, color::SwatchDelete => Changed, ro = false, destructive = true, image = None,
        "Delete a swatch. One that is in use is refused (the error lists where) unless replace_with gives a colour or swatch to use instead. Scheme slot swatches cannot be deleted. Requires confirm: true."),
    tool!("scheme_list", "Colour", scheme_list, color::SchemeList => color::SchemeListOut, ro = true, destructive = false, image = None,
        "List the twelve built-in colour schemes (main, accent 1-4, hyperlink, followed hyperlink) and which one the document uses."),
    tool!("scheme_apply", "Colour", scheme_apply, color::SchemeApply => Changed, ro = false, destructive = false, image = None,
        "Apply a built-in colour scheme: sets the seven \"Scheme: ...\" slot swatches, so everything that uses them (the built-in templates do) recolours. One undo step."),
    // ---------------------------------------------------------------- images
    tool!("image_place", "Images", image_place, images::ImagePlace => images::ImagePlaceOut, ro = false, destructive = false, image = None,
        "Put an image (file path or base64; PNG, JPEG or SVG) into a picture frame or image placeholder. Returns asset_id and effective print dpi (aim for 150+)."),
    tool!("image_crop", "Images", image_crop, images::ImageCrop => Changed, ro = false, destructive = false, image = None,
        "Crop the image in a picture frame: fractions 0..1 of the image trimmed from left, top, right and bottom (e.g. left 0.1 trims 10%)."),
    tool!("image_fit", "Images", image_fit, images::ImageFitReq => Changed, ro = false, destructive = false, image = None,
        "Set how the image fills its frame: fill (cover, may crop), fit (all visible) or stretch. resize_frame: true makes the frame match the image's aspect ratio."),
    tool!("image_replace", "Images", image_replace, images::ImagePlace => images::ImagePlaceOut, ro = false, destructive = false, image = None,
        "Swap the image in a picture frame for another file/base64 image, keeping the frame and fit mode (crop is reset)."),
    // ---------------------------------------------------------------- templates
    tool!("template_list", "Templates", template_list, templates::TemplateList => templates::TemplateListOut, ro = true, destructive = false, image = None,
        "List templates (built-in: flyer, trifold_brochure, business_card, newsletter; plus user templates) with page size and every placeholder's name, kind, hint and size."),
    tool!("template_apply", "Templates", template_apply, templates::TemplateApply => templates::TemplateApplyOut, ro = false, destructive = false, image = None,
        "Create a new document from a template and make it current. Returns placeholder frame ids by semantic name (e.g. headline, cover_image). Fill text placeholders with text_set (keeps the template styling) and image placeholders with image_place."),
    tool!("template_save", "Templates", template_save, templates::TemplateSave => templates::TemplateSaveOut, ro = false, destructive = false, image = None,
        "Save the document as a reusable user template. Mark frames as placeholders first with frame_set_placeholder."),
    // ---------------------------------------------------------------- labels and booklets
    tool!("sheet_presets_list", "Labels", sheet_presets_list, sheets::SheetPresetsList => sheets::SheetPresetsOut, ro = true, destructive = false, image = None,
        "List label, card and postcard sheet presets (Avery US 5160 ..., A4 L7160 ..., generic 2-up/4-up) with the paper size, grid, and the label size the document page must have."),
    tool!("doc_set_sheet", "Labels", doc_set_sheet, sheets::DocSetSheet => Changed, ro = false, destructive = false, image = None,
        "Print the document as a label or card sheet: PDF export and print put the page N-up on the sheet. Give a preset id from sheet_presets_list or a custom grid (points); omit both to clear. fill: repeat (the page in every cell, default) or sequence (one page per label, in order). The page size must equal the label size, otherwise the call is refused with a hint."),
    tool!("doc_set_booklet", "Labels", doc_set_booklet, sheets::DocSetBooklet => Changed, ro = false, destructive = false, image = None,
        "Print the document as a saddle-stitched booklet: pages are padded to a multiple of 4, reordered and put 2-up on sheets twice the page width (front and back are separate PDF pages). Not combinable with a label sheet."),
    // ---------------------------------------------------------------- export
    tool!("export_pdf", "Export", export_pdf, export::ExportPdf => export::ExportPdfOut, ro = true, destructive = false, image = None,
        "Export the whole document as a print-ready PDF (embedded subset fonts, TrimBox/BleedBox set). Options: bleed, crop_marks, font_embedding subset|outlines, impose (default true: documents with a label sheet or booklet are imposed). Writes to path, or returns base64 when no path is given."),
    tool!("export_png", "Export", export_png, export::ExportPng => export::ExportPngOut, ro = true, destructive = false, image = None,
        "Render one page to PNG at a dpi (default 150), optionally including bleed. Writes to path or returns base64."),
    tool!("render_preview", "Export", render_preview, export::RenderPreview => export::RenderPreviewOut, ro = true, destructive = false, image = Some("png_base64"),
        "Render a low-resolution PNG preview of a page (default 60 dpi) so you can look at your work. Returned as an image."),
    // ---------------------------------------------------------------- history
    tool!("undo", "History", undo, doc::DocRef => history::UndoOut, ro = false, destructive = false, image = None,
        "Undo the last change to the document. Every tool call that changed the document is one step."),
    tool!("redo", "History", redo, doc::DocRef => history::UndoOut, ro = false, destructive = false, image = None,
        "Redo the last undone change."),
    tool!("history_list", "History", history_list, doc::DocRef => history::HistoryOut, ro = true, destructive = false, image = None,
        "List undo history, oldest first; undone entries can be redone."),
    // ---------------------------------------------------------------- query
    tool!("doc_to_json", "Query", doc_to_json, doc::DocRef => query::DocToJsonOut, ro = true, destructive = false, image = None,
        "Return the complete document model as JSON (pages, frames, stories, styles, asset metadata). This is the document.json stored in .pubrs files."),
    tool!("validate", "Query", validate, doc::DocRef => query::ValidateOut, ro = true, destructive = false, image = None,
        "Check print readiness. Warnings: overflow, missing_font, low_resolution_image (<150 dpi), off_page, beyond_bleed, empty_placeholder, empty_image_frame — each with a hint."),
    tool!("fonts_list", "Query", fonts_list, doc::DocRef => query::FontsListOut, ro = true, destructive = false, image = None,
        "List available font families (bundled ones are always available and embed in PDFs)."),
];

/// Find a tool by name.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    all_tools().iter().find(|t| t.name == name)
}

/// All tools, including those registered by later modules (templates).
pub fn all_tools() -> &'static [ToolDef] {
    TOOLS
}

/// Call a tool by name with JSON arguments.
pub fn call_tool(session: &mut Session, name: &str, args: Value) -> ApiResult<Value> {
    let t = tool(name).ok_or_else(|| {
        ApiError::new(
            "unknown_tool",
            format!("no tool named {name:?}"),
            "list tools with tools/list",
        )
    })?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (t.call)(session, args)));
    match result {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            tracing::error!(tool = name, "tool panicked: {msg}");
            Err(ApiError::internal(format!(
                "tool {name} failed unexpectedly: {msg}"
            )))
        }
    }
}
