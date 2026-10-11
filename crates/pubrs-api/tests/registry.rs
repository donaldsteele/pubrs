//! Registry sanity: names, schemas and descriptions for every tool.

use std::collections::HashSet;

use pubrs_api::all_tools;

/// Tools the specification requires (PLAN.md, MCP server spec).
const REQUIRED: &[&str] = &[
    "doc_new",
    "doc_open",
    "doc_import",
    "doc_save",
    "doc_save_as",
    "doc_close",
    "doc_list",
    "doc_info",
    "doc_set_page_size",
    "doc_set_margins",
    "doc_set_bleed",
    "page_add",
    "page_delete",
    "page_reorder",
    "page_list",
    "page_info",
    "master_create",
    "master_apply",
    "guide_add",
    "guide_remove",
    "frame_add_text",
    "frame_add_image",
    "frame_add_shape",
    "frame_delete",
    "frame_move",
    "frame_resize",
    "frame_rotate",
    "frame_set_zorder",
    "frame_group",
    "frame_ungroup",
    "frame_lock",
    "frame_align",
    "frame_distribute",
    "frame_set_style",
    "frame_list",
    "frame_get",
    "text_set",
    "text_insert",
    "text_replace_range",
    "text_apply_char_style",
    "text_apply_para_style",
    "text_link_frames",
    "text_unlink_frames",
    "text_get",
    "text_overflow_status",
    "style_list",
    "style_create",
    "style_update",
    "style_delete",
    "image_place",
    "image_crop",
    "image_fit",
    "image_replace",
    "export_pdf",
    "export_png",
    "render_preview",
    "undo",
    "redo",
    "history_list",
    "doc_to_json",
    "validate",
    "fonts_list",
];

#[test]
fn every_required_tool_exists() {
    let names: HashSet<&str> = all_tools().iter().map(|t| t.name).collect();
    for r in REQUIRED {
        assert!(names.contains(r), "missing tool {r}");
    }
    assert_eq!(names.len(), all_tools().len(), "duplicate tool names");
}

#[test]
fn schemas_are_objects_with_documented_fields() {
    for t in all_tools() {
        assert!(t.description.len() > 20, "{} needs a description", t.name);
        let input = (t.input_schema)();
        assert_eq!(input["type"], "object", "{} input", t.name);
        assert!(
            !input.to_string().contains("$ref"),
            "{} input schema must be inlined",
            t.name
        );
        let output = (t.output_schema)();
        assert_eq!(output["type"], "object", "{} output", t.name);
        if t.destructive {
            assert!(
                input["properties"].get("confirm").is_some(),
                "{} is destructive and needs a confirm flag",
                t.name
            );
        }
    }
}

#[test]
fn input_schema_mentions_units_for_geometry() {
    let t = pubrs_api::tool("frame_add_text").unwrap();
    let s = (t.input_schema)().to_string();
    assert!(s.contains("points"), "{s}");
}
