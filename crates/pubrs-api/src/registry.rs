//! The tool registry: every operation clients can call by name with JSON.
//! The MCP server and `pubrs script` use this list 1:1; the GUI calls the
//! typed `Session` methods behind it.

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::session::Session;
use crate::tools::text::TextChangeOut;
use crate::tools::{
    color, doc, export, find, frames, history, images, import, merge, pages, query, sheets, spell,
    styles, tables, templates, text, wordart,
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
    tool!("doc_import", "Document", doc_import, import::DocImport => import::DocImportOut, ro = false, destructive = false, image = None,
        "Import a Microsoft Publisher (.pub) file as a new, unsaved document and make it current. Reads pages, text frames with their formatting and links, pictures (PNG, JPEG, GIF), rectangles, ellipses and lines with fill and outline, tables, colour scheme swatches and master pages; Windows metafile pictures are left as empty picture frames. The report lists everything dropped or substituted, by kind and count, including fonts that are not installed. Use doc_save_as to keep the result."),
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
        "Set a frame's fill colour, outline (stroke colour, width in points, dash solid/dashed/dotted), opacity (0-1), drop shadow ({offset_x, offset_y, blur, color, opacity}), BorderArt border ({pattern, width}) and display name. Use \"none\" to remove fill, outline, shadow or border art."),
    tool!("frame_add_wordart", "Frames", frame_add_wordart, wordart::FrameAddWordArt => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Add WordArt: one line of decorative text warped as a whole (warp: none, arch_up, arch_down, circle, wave, slant_up, slant_down, inflate) with a fill (colour, swatch or gradient), optional outline and shadow. The text is scaled down, never up, to fit the box; size is its nominal size (default 120, so it fills the box). Edit it with wordart_set."),
    tool!("wordart_set", "Frames", wordart_set, wordart::WordArtSet => Changed, ro = false, destructive = false, image = None,
        "Change a WordArt frame: text, font, size, warp, fill, outline (colour or \"none\", width, dash) and shadow ({offset_x, offset_y, blur, color, opacity} or \"none\"). Only the fields given change. text_find and text_replace_all also reach WordArt text."),
    tool!("border_art_list", "Frames", border_art_list, wordart::BorderArtList => wordart::BorderArtListOut, ro = true, destructive = false, image = None,
        "List the twelve BorderArt patterns and the allowed width range. Apply one with frame_set_style border_art {pattern, width}; the tiles are scaled so a whole number fits each edge and the corners get their own piece."),
    tool!("frame_set_wrap", "Frames", frame_set_wrap, frames::FrameSetWrap => Changed, ro = false, destructive = false, image = None,
        "Set how text in frames below this one wraps around it: none, square (around its bounding box), tight (around its true outline: ellipses, rounded corners, lines, rotated frames and the visible pixels of PNGs with transparency), through (like tight, but into notches and holes of the outline) or top_bottom, with a margin in points. both_sides: true fills every gap beside the frame wider than three em, left to right, instead of only the widest. Pictures default to square."),
    tool!("frame_set_placeholder", "Frames", frame_set_placeholder, frames::FrameSetPlaceholder => Changed, ro = false, destructive = false, image = None,
        "Mark a text or picture frame as a named template placeholder (e.g. \"headline\"), or pass name null to clear. Used when saving templates."),
    tool!("frame_list", "Frames", frame_list, frames::FrameList => frames::FrameListOut, ro = true, destructive = false, image = None,
        "List frames (all pages and masters, or one page/master), optionally only one kind: text, image, shape, group. Includes position, z-index, placeholder names and text previews."),
    tool!("frame_get", "Frames", frame_get, frames::FrameRef => frames::FrameGetOut, ro = true, destructive = false, image = None,
        "Get one frame in full (geometry, style, wrap, content), plus overflow for text frames and effective dpi for images."),
    tool!("frame_duplicate", "Frames", frame_duplicate, frames::FrameDuplicate => frames::FrameAddOut, ro = false, destructive = false, image = None,
        "Copy a frame (text frames get a copy of their text) offset by dx/dy points, optionally onto another page."),
    tool!("frame_copy_to_doc", "Frames", frame_copy_to_doc, frames::FrameCopyToDoc => frames::FrameCopyOut, ro = false, destructive = false, image = None,
        "Copy frames from one open document into another (or the same one): text frames keep their text and chain, picture frames bring their images (an image the target already holds is reused), groups stay groups. Styles are matched by name, missing ones are added; swatches the target lacks become their colours. One undo step in the target. Give the page by page_index (from 0) and an optional offset in points."),
    // ---------------------------------------------------------------- tables
    tool!("table_add", "Tables", table_add, tables::TableAdd => tables::TableAddOut, ro = false, destructive = false, image = None,
        "Add a table: rows x cols cells filling the rectangle (equal columns and rows, 0.5pt black borders, 4pt insets), optionally with a header_fill on the first row. Each cell holds ordinary text: story_ids lists the cells row by row; fill them with table_set_cell_text. A row grows to fit its text up to the frame's bottom; text that no longer fits is reported by text_overflow_status and validate."),
    tool!("table_get", "Tables", table_get, tables::TableRef => tables::TableGetOut, ro = true, destructive = false, image = None,
        "Read a table: column widths, row heights (minimum and as laid out), the grid of story ids, and every cell with its text, span, fill, borders, insets, vertical alignment and whether its text overflows. Use a cell's story_id with text_get, text_set, text_find, style_apply and the other text tools."),
    tool!("table_set_cell_text", "Tables", table_set_cell_text, tables::TableSetCellText => tables::TableCellOut, ro = false, destructive = false, image = None,
        "Replace the text of the cell at (row, col), counting from 0. Any position a merged cell covers addresses that cell. Keeps the cell's first-paragraph formatting; markup: true enables headings and **bold**/*italic*/__underline__. Reports whether the text fits."),
    tool!("table_insert_rows", "Tables", table_insert_rows, tables::TableInsertRows => tables::TableEditOut, ro = false, destructive = false, image = None,
        "Insert empty rows before row `at` (default: at the end). A merged cell that spans the insertion point grows instead of getting new cells; new cells copy the borders, insets and alignment of their neighbours (not the fill)."),
    tool!("table_delete_rows", "Tables", table_delete_rows, tables::TableDeleteRows => tables::TableEditOut, ro = false, destructive = true, image = None,
        "Delete rows and the text in their cells. Requires confirm: true (undo restores them). Merged cells shrink; a table keeps at least one row."),
    tool!("table_insert_cols", "Tables", table_insert_cols, tables::TableInsertCols => tables::TableEditOut, ro = false, destructive = false, image = None,
        "Insert empty columns before column `at` (default: at the end), as wide as the column next to them. The table frame grows by the new columns' width."),
    tool!("table_delete_cols", "Tables", table_delete_cols, tables::TableDeleteCols => tables::TableEditOut, ro = false, destructive = true, image = None,
        "Delete columns and the text in their cells. Requires confirm: true (undo restores them). Merged cells shrink; a table keeps at least one column. The frame narrows by the deleted width."),
    tool!("table_merge_cells", "Tables", table_merge_cells, tables::TableMergeCells => tables::TableEditOut, ro = false, destructive = false, image = None,
        "Merge a rectangle of cells into its top-left cell. The other cells' text is appended to it, each as its own paragraph. The rectangle must not cut through another merged cell."),
    tool!("table_split_cell", "Tables", table_split_cell, tables::TableSplitCell => tables::TableEditOut, ro = false, destructive = false, image = None,
        "Split a merged cell back into single cells. The top-left cell keeps the text; the new cells are empty and copy its style."),
    tool!("table_set_cell_style", "Tables", table_set_cell_style, tables::TableSetCellStyle => Changed, ro = false, destructive = false, image = None,
        "Style a cell, or every cell in a row_span x col_span range: fill (colour, swatch or \"none\"), borders (`border` for all four sides, or border_top / border_right / border_bottom / border_left; each {color, width, dash} or \"none\"), inset in points, valign (top, middle, bottom). Only the fields given change. A border two cells share is drawn once, from the later cell."),
    tool!("table_set_col_width", "Tables", table_set_col_width, tables::TableSetColWidth => Changed, ro = false, destructive = false, image = None,
        "Set one column's width in points. The table frame's width follows the sum of the columns."),
    tool!("table_set_row_height", "Tables", table_set_row_height, tables::TableSetRowHeight => Changed, ro = false, destructive = false, image = None,
        "Set a row's minimum height in points. The row still grows to fit its text, up to the frame's bottom."),
    // ---------------------------------------------------------------- text
    tool!("text_set", "Text", text_set, text::TextSet => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Replace all text of a text frame's story (the whole linked chain). Keeps the existing first-paragraph formatting, so filling a template placeholder keeps its style. markup: true enables # headings and **bold**/*italic*/__underline__. Reports overflow."),
    tool!("text_insert", "Text", text_insert, text::TextInsert => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Insert text at a character offset (default: end). Offsets count characters; each paragraph break is one \"\\n\"."),
    tool!("text_insert_break", "Text", text_insert_break, text::TextInsertBreak => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Insert a column, frame or page break at a character offset (default: end). A break is one character. column continues in the next column or frame; frame in the next frame of the chain; page in the next frame of the chain on a later page (pages are not added: with none, the rest overflows)."),
    tool!("text_replace_range", "Text", text_replace_range, text::TextReplaceRange => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Replace characters start..end (end exclusive) with new text; empty text deletes. The replaced text's formatting is kept."),
    tool!("text_apply_char_style", "Text", text_apply_char_style, text::TextApplyCharStyle => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Format characters start..end (default all): character style by name, font_family, font_size (points), bold, italic, underline, strikethrough, colour, letter_spacing, baseline (normal, superscript, subscript or {shift: points}), caps (none, small, all), ligatures, kerning. clear: true removes local formatting first."),
    tool!("text_apply_para_style", "Text", text_apply_para_style, text::TextApplyParaStyle => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Format paragraphs touching start..end (default all): paragraph style by name (e.g. \"Heading 1\", \"Body\"), align left/center/right/justify, line_spacing (multiple of font size), space_before/after and indents in points, hyphenation, keep_with_next, keep_together, widow_orphan (true = 2 lines), drop_cap {lines, chars}, tab_stops and list (see text_set_tabs and text_apply_list)."),
    tool!("text_set_tabs", "Text", text_set_tabs, text::TextSetTabs => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Set the tab stops of the paragraphs touching start..end (default all): stops [{pos, align left/center/right/decimal, leader none/dot/dash/line}], pos in points from the text edge. An empty list clears them. A tab with no stop ahead goes to the next 36 pt. Decimal stops align on the first . (or , in languages that write a decimal comma). Leaders fill the gap before the text and line up from line to line."),
    tool!("text_apply_list", "Text", text_apply_list, text::TextApplyList => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Make the paragraphs touching start..end (default all) bullet or numbered list items with a hanging indent: kind, format (1. a. A. i. I.), bullet character, level 0-8, indent (points the label hangs left of the text), restart_at (number the first paragraph from this value). Numbering continues across paragraphs and linked frames; a deeper level restarts after a shallower item. Labels are generated text: they are not in the story and do not move offsets (text_get lists them as list_label)."),
    tool!("text_clear_list", "Text", text_clear_list, text::TextClearList => text::TextChangeOut, ro = false, destructive = false, image = None,
        "Turn list items touching start..end (default all) back into plain paragraphs. Resets the left indent to 0 unless keep_indent is true."),
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
    // ---------------------------------------------------------------- spelling
    tool!("spell_check", "Spelling", spell_check, spell::SpellCheck => spell::SpellCheckOut, ro = true, destructive = false, image = None,
        "Find misspelt words in the whole document (reading order) or in one story (frame_id or story_id). Each paragraph is checked in its own language (paragraph `language`, BCP 47): en-US is built in, other languages need a user Hunspell dictionary; paragraphs in a language without one are skipped and listed in unchecked_languages. Words with digits, web and e-mail addresses, merge fields and (by default) words in capitals only are not checked. Returns story character ranges like text_find; fix a word with text_replace_range."),
    tool!("spell_suggest", "Spelling", spell_suggest, spell::SpellSuggest => spell::SpellSuggestOut, ro = true, destructive = false, image = None,
        "Suggest corrections for a word in a language (default en-US), best first, and say whether the word is already correct."),
    tool!("spell_add_word", "Spelling", spell_add_word, spell::SpellAddWord => spell::SpellAddWordOut, ro = false, destructive = false, image = None,
        "Add a word to the user dictionary so spell_check accepts it in every language (capitalised and all-capitals forms too). The word is saved to the user dictionary file when the session has a dictionaries folder (the app and the pubrs CLI do); otherwise it lasts for this session."),
    // ---------------------------------------------------------------- styles
    tool!("style_list", "Styles", style_list, styles::StyleList => styles::StyleListOut, ro = true, destructive = false, image = None,
        "List paragraph and character styles with their properties and parents. Built-ins: Normal, Title, Heading 1, Heading 2, Body, Caption, Emphasis."),
    tool!("style_create", "Styles", style_create, styles::StyleCreate => styles::StyleCreateOut, ro = false, destructive = false, image = None,
        "Create a named paragraph (default) or character style, optionally based_on another style. Properties: paragraph {align, line_spacing, space_before, space_after, indents}, character {font_family, font_size, bold, italic, underline, strikethrough, color, letter_spacing, baseline, caps, ligatures, kerning}."),
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
    tool!("image_set_alt_text", "Images", image_set_alt_text, images::ImageSetAltText => Changed, ro = false, destructive = false, image = None,
        "Set the alt text of a picture frame: what the picture shows, for screen readers. It is written to the PDF as /Alt when export_pdf is called with tagged: true. An empty string clears it."),
    // ---------------------------------------------------------------- templates
    tool!("template_list", "Templates", template_list, templates::TemplateList => templates::TemplateListOut, ro = true, destructive = false, image = None,
        "List templates (built-in: flyer, trifold_brochure, business_card, newsletter, greeting_card, postcard, certificate, menu, letterhead, envelope, calendar, address_labels, business_cards; plus user templates) with page size and every placeholder's name, kind, hint and size. The calendar template takes a year option in template_apply."),
    tool!("template_apply", "Templates", template_apply, templates::TemplateApply => templates::TemplateApplyOut, ro = false, destructive = false, image = None,
        "Create a new document from a template and make it current. Returns placeholder frame ids by semantic name (e.g. headline, cover_image). Fill text placeholders with text_set (keeps the template styling) and image placeholders with image_place (then image_set_alt_text). The calendar template takes year (1600 to 9999, default 2027); year with any other template is an error."),
    tool!("template_save", "Templates", template_save, templates::TemplateSave => templates::TemplateSaveOut, ro = false, destructive = false, image = None,
        "Save the document as a reusable user template. Mark frames as placeholders first with frame_set_placeholder."),
    // ---------------------------------------------------------------- labels and booklets
    tool!("sheet_presets_list", "Labels", sheet_presets_list, sheets::SheetPresetsList => sheets::SheetPresetsOut, ro = true, destructive = false, image = None,
        "List label, card and postcard sheet presets (Avery US 5160 ..., A4 L7160 ..., generic 2-up/4-up) with the paper size, grid, and the label size the document page must have."),
    tool!("doc_set_sheet", "Labels", doc_set_sheet, sheets::DocSetSheet => Changed, ro = false, destructive = false, image = None,
        "Print the document as a label or card sheet: PDF export and print put the page N-up on the sheet. Give a preset id from sheet_presets_list or a custom grid (points); omit both to clear. fill: repeat (the page in every cell, default) or sequence (one page per label, in order). The page size must equal the label size, otherwise the call is refused with a hint."),
    tool!("doc_set_booklet", "Labels", doc_set_booklet, sheets::DocSetBooklet => Changed, ro = false, destructive = false, image = None,
        "Print the document as a saddle-stitched booklet: pages are padded to a multiple of 4, reordered and put 2-up on sheets twice the page width (front and back are separate PDF pages). Not combinable with a label sheet."),
    // ---------------------------------------------------------------- mail merge
    tool!("merge_set_source", "Merge", merge_set_source, merge::MergeSetSource => merge::MergeSourceOut, ro = false, destructive = false, image = None,
        "Load the mail-merge data into the document (replacing any earlier data): a CSV or XLSX file by path, or CSV text as inline_csv. CSV is UTF-8 (a BOM is fine), comma or semicolon separated, with quoted fields; XLSX reads the first sheet or the one named in sheet (cells as displayed, dates as ISO). The first row names the fields. The data is stored in the document, so it reopens without the file; refresh: true reads the last file again. Limits: 100000 records, 1 MB per cell, 512 fields, 5000000 cells, 64 MiB per file."),
    tool!("merge_fields_list", "Merge", merge_fields_list, merge::MergeFieldsList => merge::MergeFieldsOut, ro = true, destructive = false, image = None,
        "List the merge fields of the loaded data with the previewed record's value and how often the document uses each, the record count, the preview record and switch, and the field names the document uses that the data lacks."),
    tool!("merge_insert_field", "Merge", merge_insert_field, merge::MergeInsertField => TextChangeOut, ro = false, destructive = false, image = None,
        "Insert a merge field (one placeholder character that counts as one character in offsets) into a text frame or story at an offset (default: the end). It shows as «Name», or as the previewed record's value after merge_preview. It takes the formatting of the text before it. When data is loaded the field must be one of its fields."),
    tool!("merge_preview", "Merge", merge_preview, merge::MergePreview => merge::MergePreviewOut, ro = false, destructive = false, image = None,
        "Choose which record (0-based index) the fields show and switch the preview on or off (off shows «Field» names). Render with render_preview to look at it. An empty value collapses one adjacent space (the one after the field, else the one before)."),
    tool!("merge_export", "Merge", merge_export, merge::MergeExport => merge::MergeExportOut, ro = true, destructive = false, image = None,
        "Run the merge: format pdf (one PDF with the pages once per record) or publication (a new .pubrs with the pages once per record and the fields replaced by text; open: true also opens it). On a label sheet (doc_set_sheet) a sequence sheet gets one record per label, a repeat sheet one sheet per record. Writes to path, or returns base64 when no path is given. Fails with a hint when there is no data, no records or no fields."),
    tool!("merge_clear", "Merge", merge_clear, merge::MergeClear => Changed, ro = false, destructive = true, image = None,
        "Remove the merge data from the document. Requires confirm: true. Fields already in the text stay and show as «Name»."),
    // ---------------------------------------------------------------- export
    tool!("export_pdf", "Export", export_pdf, export::ExportPdf => export::ExportPdfOut, ro = true, destructive = false, image = None,
        "Export the whole document as a print-ready PDF (embedded subset fonts, TrimBox/BleedBox set). Options: bleed, crop_marks, font_embedding subset|outlines, impose (default true: documents with a label sheet or booklet are imposed), standard none|pdfx4|pdfa2b (pdfx4 = commercial printing: PDF 1.6, CMYK output intent, checked before it is returned; pdfa2b = archive), output_intent_icc (path of a CMYK output profile for pdfx4), tagged (tagged PDF with image alt text). CMYK and spot swatches are written as DeviceCMYK and Separation colours. Writes to path, or returns base64 when no path is given."),
    tool!("export_print_package", "Export", export_print_package, export::ExportPrintPackage => export::ExportPrintPackageOut, ro = true, destructive = false, image = None,
        "Save for a commercial printer: a zip with the PDF/X-4 file, the .pubrs document with its images, and a README listing the fonts used and the colours (swatches with their CMYK and spot values). Options: bleed, crop_marks, output_intent_icc. Writes to path, or returns base64 when no path is given."),
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
    tool!("validate", "Query", validate, query::ValidateIn => query::ValidateOut, ro = true, destructive = false, image = None,
        "Design checker: check print readiness. Warnings (each with a hint, a page and object ids): overflow, missing_font, missing_glyphs (names the characters no font covers), low_resolution_image (<150 dpi), off_page, beyond_bleed, empty_placeholder, empty_image_frame, hyphenation_unavailable, text_too_small (<6 pt), low_contrast_text (contrast ratio <3:1), object_in_unprintable_area (within 0.25 in of the edge, desktop jobs without bleed), empty_table_cells, missing_alt_text (pictures), merge_field_missing (a field that names a column the loaded merge data lacks), and with commercial=true also rgb_in_cmyk_job (an RGB colour in a CMYK job; swatches that are CMYK or spot are fine). commercial=true is for a job going to a commercial printer: it skips the desktop-margin check. merge_field_missing arrives with mail merge."),
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
