//! MCP acceptance test (PLAN.md): an rmcp client, in-process, no GUI:
//! apply trifold template → fill all placeholders → place two images → link
//! frames and verify text_overflow_status is clean → validate with zero
//! warnings → export_pdf → save as .pubrs → doc_open the saved file →
//! doc_to_json equals original.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use pubrs_api::{Session, SessionConfig};
use pubrs_mcp::PubrsServer;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, GetPromptRequestParams, ReadResourceRequestParams,
    ResourceContents,
};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Harness {
    client: RunningService<RoleClient, ()>,
    out: PathBuf,
}

async fn start(name: &str) -> Harness {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let session = Session::new(SessionConfig {
        id_seed: Some(2026),
        font_dir: Some(root().join("tests/fonts")),
        output_dir: Some(out.clone()),
        user_templates_dir: Some(out.join("templates")),
        ..Default::default()
    });
    let server = PubrsServer::new(session);
    let (server_io, client_io) = tokio::io::duplex(1 << 22);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server starts");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client connects");
    Harness { client, out }
}

impl Harness {
    async fn raw(&self, name: &str, args: Value) -> CallToolResult {
        let params = CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().cloned().unwrap_or_default());
        self.client.call_tool(params).await.expect("protocol ok")
    }

    /// Call a tool that must succeed; returns its structured result.
    async fn call(&self, name: &str, args: Value) -> Value {
        let r = self.raw(name, args.clone()).await;
        assert_ne!(
            r.is_error,
            Some(true),
            "{name}({args}) failed: {:?}",
            r.content
                .first()
                .and_then(|c| c.as_text())
                .map(|t| t.text.clone())
        );
        r.structured_content.expect("structured content")
    }

    /// Call a tool that must fail with `code`.
    async fn fail(&self, name: &str, args: Value, code: &str) -> Value {
        let r = self.raw(name, args).await;
        assert_eq!(r.is_error, Some(true), "{name} should fail");
        let text = r.content[0].as_text().expect("text").text.clone();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["code"], code, "{v}");
        assert!(!v["hint"].as_str().unwrap_or_default().is_empty());
        v
    }
}

fn bakery_copy(name: &str) -> &'static str {
    match name {
        "cover_title" => "Golden Crust Bakery",
        "cover_subtitle" => "Real bread, baked before sunrise",
        "flap_heading" => "About Us",
        "flap_text" => {
            "Golden Crust has been baking in the same stone oven on Mill Lane since 1987. \
            We mill our own rye, feed a sourdough starter older than the shop, and shape every loaf by hand. \
            Come early: the first batch leaves the oven at six."
        }
        "contact_heading" => "Visit Us",
        "contact" => {
            "12 Mill Lane\nRiverton RT1 4BB\n01234 567890\nOpen daily 7am – 3pm\ngoldencrust.example"
        }
        "inside_heading" => "Our Story",
        "inside_body" => {
            "It started with one oven and a borrowed recipe for country sourdough. \
            Neighbours queued around the corner, so we kept baking.\n\
            Today the bakery is still family run. We use flour from farms within forty miles, \
            long slow fermentation for flavour and keeping quality, and nothing you cannot pronounce.\n\
            Every morning we bake sourdough, rye, seeded and spelt loaves, buttery croissants and \
            cinnamon buns. On Fridays and Saturdays we add gluten-free and vegan bakes.\n\
            Celebration cakes are made to order: tell us the occasion three days ahead and we will \
            design something special. We also supply cafés and restaurants across the valley, \
            delivering before eight so their bread is as fresh as ours.\n\
            Baking classes run on the first Sunday of every month. You will mix, shape and bake \
            your own loaf, learn how to keep a starter alive, and take home everything you made, \
            along with a jar of our starter to begin your own.\n\
            Thank you for keeping a small bakery busy for more than thirty years."
        }
        "features_heading" => "What We Offer",
        "features" => {
            "Sourdough, rye and seeded loaves\nCroissants and pastries\nCelebration cakes to order\nGluten-free Fridays\nWholesale for cafés"
        }
        other => panic!("unexpected placeholder {other}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trifold_acceptance() {
    let h = start("mcp-acceptance").await;

    // Every registry tool is published with schemas.
    let tools = h.client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), pubrs_api::all_tools().len());
    assert!(tools.iter().all(|t| t.output_schema.is_some()));

    // 1. Apply the trifold template.
    let applied = h
        .call(
            "template_apply",
            json!({"name": "trifold_brochure", "title": "Golden Crust Bakery"}),
        )
        .await;
    let doc_id = applied["doc_id"].as_str().unwrap().to_string();
    let placeholders = applied["placeholders"].as_array().unwrap().clone();
    assert!(placeholders.len() >= 12);

    // 2. Fill every text placeholder.
    for p in placeholders.iter().filter(|p| p["kind"] == "text") {
        let name = p["name"].as_str().unwrap();
        let out = h
            .call(
                "text_set",
                json!({"doc_id": doc_id, "frame_id": p["frame_id"], "text": bakery_copy(name)}),
            )
            .await;
        assert_eq!(out["doc_id"], json!(doc_id));
    }

    // 3. Place two images into the image placeholders.
    let images: Vec<&Value> = placeholders
        .iter()
        .filter(|p| p["kind"] == "image")
        .collect();
    assert_eq!(images.len(), 2);
    let files = [
        root().join("examples/assets/bread.jpg"),
        root().join("examples/assets/coffee.png"),
    ];
    for (p, file) in images.iter().zip(&files) {
        let out = h
            .call(
                "image_place",
                json!({"frame_id": p["frame_id"], "path": file.display().to_string()}),
            )
            .await;
        assert!(out["effective_dpi"].as_f64().unwrap() >= 150.0, "{out}");
        // The design checker wants alt text on every picture.
        h.call(
            "image_set_alt_text",
            json!({"frame_id": p["frame_id"], "alt_text": "A photo for the brochure"}),
        )
        .await;
    }

    // 4. Link the story into the continuation frame; overflow must be clean.
    let before = h.call("text_overflow_status", json!({})).await;
    assert_eq!(
        before["any_overflow"], true,
        "the long story overflows one panel"
    );
    let inside_body = placeholders
        .iter()
        .find(|p| p["name"] == "inside_body")
        .unwrap();
    let cont = &applied["named_frames"]["inside_body_continued"];
    h.call(
        "text_link_frames",
        json!({"from_frame_id": inside_body["frame_id"], "to_frame_id": cont}),
    )
    .await;
    let status = h
        .call("text_overflow_status", json!({"doc_id": doc_id}))
        .await;
    assert_eq!(status["any_overflow"], false, "{status}");

    // 5. Validate: zero warnings.
    let v = h.call("validate", json!({})).await;
    assert_eq!(v["warnings"], json!([]), "{v}");
    assert_eq!(v["ok"], true);

    // 6. Export the PDF and check it structurally.
    let pdf = h
        .call(
            "export_pdf",
            json!({"path": "bakery-trifold.pdf", "crop_marks": true}),
        )
        .await;
    let pdf_path = PathBuf::from(pdf["path"].as_str().unwrap());
    assert_eq!(pdf_path, h.out.join("bakery-trifold.pdf"));
    let parsed = lopdf::Document::load(&pdf_path).unwrap();
    assert_eq!(parsed.get_pages().len(), 2);

    // 7. Save as .pubrs, reopen, compare.
    let saved = h
        .call("doc_save_as", json!({"path": "bakery-trifold"}))
        .await;
    let saved_path = saved["path"].as_str().unwrap().to_string();
    assert!(saved_path.ends_with("bakery-trifold.pubrs"));
    let original = h.call("doc_to_json", json!({"doc_id": doc_id})).await;
    let reopened = h.call("doc_open", json!({"path": saved_path})).await;
    let reopened_id = reopened["doc_id"].as_str().unwrap();
    assert_ne!(reopened_id, doc_id);
    let copy = h.call("doc_to_json", json!({"doc_id": reopened_id})).await;
    assert_eq!(
        copy["document"], original["document"],
        "doc_to_json after reopen must be identical"
    );

    // A preview comes back as an image the agent can look at.
    let r = h.raw("render_preview", json!({"doc_id": doc_id})).await;
    assert!(
        r.content
            .iter()
            .any(|c| c.as_image().is_some_and(|i| i.mime_type == "image/png"))
    );

    h.client.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resources_prompts_and_errors() {
    let h = start("mcp-resources").await;
    let doc = h.call("doc_new", json!({"title": "Res"})).await;
    let id = doc["doc_id"].as_str().unwrap();

    let resources = h.client.list_all_resources().await.unwrap();
    let uris: Vec<String> = resources.iter().map(|r| r.uri.clone()).collect();
    assert!(uris.contains(&"pubrs://fonts".to_string()));
    assert!(uris.contains(&"pubrs://templates/trifold_brochure".to_string()));
    assert!(uris.contains(&format!("pubrs://doc/{id}")));
    assert!(uris.contains(&format!("pubrs://doc/{id}/page/1.png")));
    assert_eq!(
        h.client.list_all_resource_templates().await.unwrap().len(),
        4
    );
    assert!(uris.contains(&format!("pubrs://doc/{id}/swatches")));

    let read = |uri: String| {
        let c = &h.client;
        async move { c.read_resource(ReadResourceRequestParams::new(uri)).await }
    };
    let doc_json = read(format!("pubrs://doc/{id}")).await.unwrap();
    match &doc_json.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => {
            let v: Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["meta"]["title"], "Res");
        }
        other => panic!("expected text, got {other:?}"),
    }
    h.call(
        "swatch_create",
        json!({"name": "Ink", "value": {"cmyk": {"c": 0, "m": 0, "y": 0, "k": 100}}}),
    )
    .await;
    let sw = read(format!("pubrs://doc/{id}/swatches")).await.unwrap();
    match &sw.contents[0] {
        ResourceContents::TextResourceContents { text, .. } => {
            let v: Value = serde_json::from_str(text).unwrap();
            assert_eq!(v["swatches"][0]["name"], "Ink");
            assert_eq!(v["swatches"][0]["screen"], "#000000");
        }
        other => panic!("expected text, got {other:?}"),
    }
    let png = read(format!("pubrs://doc/{id}/page/1.png")).await.unwrap();
    assert!(matches!(
        &png.contents[0],
        ResourceContents::BlobResourceContents { .. }
    ));
    let tpl = read("pubrs://templates/business_card".into())
        .await
        .unwrap();
    assert!(matches!(
        &tpl.contents[0],
        ResourceContents::TextResourceContents { .. }
    ));
    assert!(read(format!("pubrs://doc/{id}/page/9.png")).await.is_err());
    assert!(read("pubrs://nope".into()).await.is_err());

    let prompts = h.client.list_all_prompts().await.unwrap();
    let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "make_flyer",
            "make_trifold",
            "make_business_card",
            "make_newsletter",
            "make_greeting_card",
            "make_mail_merge",
            "make_labels"
        ]
    );
    let p = h
        .client
        .get_prompt(
            GetPromptRequestParams::new("make_trifold").with_arguments(
                json!({"business": "a bakery"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
    let text = serde_json::to_string(&p.messages).unwrap();
    assert!(text.contains("template_apply") && text.contains("a bakery"));
    for (name, key, value, needs) in [
        (
            "make_newsletter",
            "topic",
            "a community garden",
            "events_table",
        ),
        (
            "make_greeting_card",
            "occasion",
            "a birthday",
            "front_greeting",
        ),
    ] {
        let p = h
            .client
            .get_prompt(
                GetPromptRequestParams::new(name)
                    .with_arguments(json!({ key: value }).as_object().cloned().unwrap()),
            )
            .await
            .unwrap();
        let text = serde_json::to_string(&p.messages).unwrap();
        assert!(
            text.contains("template_apply") && text.contains(value),
            "{name}"
        );
        assert!(
            text.contains(needs) && text.contains("image_set_alt_text"),
            "{name}"
        );
    }

    for (prompt, args, tools) in [
        (
            "make_mail_merge",
            json!({"piece": "invitation postcards", "data": "guests.xlsx", "sheet": "Guests"}),
            [
                "doc_new",
                "merge_set_source",
                "merge_fields_list",
                "frame_add_text",
                "merge_insert_field",
                "merge_preview",
                "validate",
                "merge_export",
            ],
        ),
        (
            "make_labels",
            json!({"data": "addresses.csv"}),
            [
                "sheet_presets_list",
                "doc_new",
                "doc_set_sheet",
                "merge_set_source",
                "frame_add_text",
                "merge_insert_field",
                "merge_preview",
                "merge_export",
            ],
        ),
    ] {
        let p = h
            .client
            .get_prompt(
                GetPromptRequestParams::new(prompt)
                    .with_arguments(args.as_object().cloned().unwrap()),
            )
            .await
            .unwrap();
        let text = serde_json::to_string(&p.messages).unwrap();
        // The tools are named in the order the steps use them.
        let mut at = 0;
        for t in tools {
            let found = text[at..].find(t);
            assert!(found.is_some(), "{prompt}: {t} missing or out of order");
            at += found.unwrap();
        }
    }
    // Tool errors are structured and readable; unknown tools are protocol errors.
    h.fail(
        "frame_delete",
        json!({"frame_id": "00000000-0000-0000-0000-000000000009", "confirm": true}),
        "not_found",
    )
    .await;
    h.fail(
        "page_delete",
        json!({"page_id": doc["page_ids"][0]}),
        "confirmation_required",
    )
    .await;
    h.fail(
        "frame_add_text",
        json!({"x": 1, "y": 1, "width": 1, "height": 1, "wat": 2}),
        "invalid_argument",
    )
    .await;
    let unknown = h
        .client
        .call_tool(CallToolRequestParams::new("not_a_tool"))
        .await;
    assert!(unknown.is_err());
    h.client.cancel().await.unwrap();
}
