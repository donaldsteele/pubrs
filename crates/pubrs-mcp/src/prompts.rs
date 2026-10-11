//! Guided workflows: make_flyer, make_trifold, make_business_card, make_newsletter, make_greeting_card,
//! make_mail_merge, make_labels.

use rmcp::ErrorData;
use rmcp::model::{GetPromptResult, JsonObject, Prompt, PromptArgument, PromptMessage, Role};

fn arg(name: &str, description: &str, required: bool) -> PromptArgument {
    PromptArgument::new(name)
        .with_description(description)
        .with_required(required)
}

pub fn list() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "make_flyer",
            Some("Create a one-page print flyer from the flyer template."),
            Some(vec![
                arg(
                    "subject",
                    "What the flyer is for, e.g. \"bakery grand opening\"",
                    true,
                ),
                arg(
                    "details",
                    "Facts to include: dates, address, offers, contact",
                    false,
                ),
                arg(
                    "images",
                    "Comma-separated image file paths to use (optional)",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "make_trifold",
            Some(
                "Create a two-sided, three-panel (trifold) brochure from the trifold_brochure template.",
            ),
            Some(vec![
                arg(
                    "business",
                    "Who the brochure is for, e.g. \"a neighbourhood bakery\"",
                    true,
                ),
                arg(
                    "details",
                    "Facts to include: services, history, address, hours, contact",
                    false,
                ),
                arg(
                    "images",
                    "Comma-separated image file paths to use (optional)",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "make_business_card",
            Some("Create a business card from the business_card template."),
            Some(vec![
                arg("name", "Person's name", true),
                arg("job_title", "Job title", false),
                arg("company", "Company name", false),
                arg("contact", "Phone, email and website", false),
                arg(
                    "logo",
                    "Logo file path (optional; otherwise draw a simple SVG mark)",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "make_newsletter",
            Some("Create a four-page newsletter from the newsletter template."),
            Some(vec![
                arg(
                    "topic",
                    "What the newsletter is about, e.g. \"spring term at the community garden\"",
                    true,
                ),
                arg(
                    "audience",
                    "Who reads it, e.g. \"members and neighbours\"",
                    false,
                ),
                arg(
                    "details",
                    "Facts to include: stories, dates, events, hours, contact",
                    false,
                ),
                arg(
                    "images",
                    "Comma-separated image file paths to use (optional)",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "make_greeting_card",
            Some("Create a quarter-fold greeting card from the greeting_card template."),
            Some(vec![
                arg(
                    "occasion",
                    "What the card is for, e.g. \"birthday\" or \"thank you\"",
                    true,
                ),
                arg("recipient", "Who it is for, e.g. \"my grandmother\"", false),
                arg("message", "What the message should say (optional)", false),
                arg(
                    "images",
                    "Comma-separated image file paths to use (optional)",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "make_mail_merge",
            Some(
                "Merge a CSV or XLSX list into a letter, invitation or card: one page per record.",
            ),
            Some(vec![
                arg(
                    "piece",
                    "What is being mailed, e.g. \"invitation postcards\"",
                    true,
                ),
                arg(
                    "data",
                    "Path of the .csv or .xlsx file (first row = field names)",
                    true,
                ),
                arg(
                    "sheet",
                    "Sheet name for an .xlsx file (default: the first)",
                    false,
                ),
                arg("details", "What the text should say", false),
            ]),
        ),
        Prompt::new(
            "make_labels",
            Some("Print address or name labels from a CSV or XLSX list onto a label sheet."),
            Some(vec![
                arg(
                    "data",
                    "Path of the .csv or .xlsx file (first row = field names)",
                    true,
                ),
                arg(
                    "preset",
                    "Label sheet id from sheet_presets_list, e.g. avery-5160 (default)",
                    false,
                ),
                arg(
                    "sheet",
                    "Sheet name for an .xlsx file (default: the first)",
                    false,
                ),
                arg(
                    "lines",
                    "What each label shows, e.g. \"name, street, city and zip\"",
                    false,
                ),
            ]),
        ),
    ]
}

fn get_str<'a>(args: Option<&'a JsonObject>, key: &str) -> Option<&'a str> {
    args.and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

const FINISH: &str = "\
Then check your work:
- text_overflow_status must report any_overflow: false. If text overflows, shorten it, reduce its font size \
with text_apply_char_style, or link the frame to an empty one with text_link_frames.
- validate must return ok: true. Read each warning's hint and fix it.
- Look at every page with render_preview and fix anything that looks wrong (text too long, awkward breaks, \
empty areas).
Finally save with doc_save_as (e.g. path \"<short-name>.pubrs\") and export a print PDF with export_pdf \
(bleed is included automatically). Tell the user the file paths.";

const IMAGES: &str = "\
Images: if the user gave image files, place them with image_place {frame_id, path}. Otherwise draw simple, \
tasteful flat SVG artwork that suits the subject (shapes only, no text, colours matching the template's navy \
#1E3A5F, orange #C2410C and cream #FFF7ED) and place it with image_place {frame_id, svg: \"<svg ...>\"}. \
Give the SVG a viewBox with the same aspect ratio as the placeholder (its width and height are in the \
template_apply result). Every image placeholder must be filled.";

pub fn get(name: &str, args: Option<&JsonObject>) -> Result<GetPromptResult, ErrorData> {
    let details = get_str(args, "details")
        .map(|d| format!("\nFacts to use: {d}"))
        .unwrap_or_default();
    let images = get_str(args, "images")
        .map(|i| format!("\nImage files provided: {i}"))
        .unwrap_or_default();
    let (description, text) = match name {
        "make_flyer" => {
            let subject = get_str(args, "subject").unwrap_or("the user's event or business");
            (
                "Flyer workflow",
                format!(
                    "Create a print-ready flyer for: {subject}.{details}{images}\n\n\
                     1. template_apply {{name: \"flyer\", title: <short title>}}. Keep the placeholder list it returns.\n\
                     2. Fill every text placeholder with text_set {{frame_id, text}} (formatting is kept):\n\
                        headline (2-5 words), subheadline (one line), body (40-80 words, may use two paragraphs \
                     separated by \\n), call_to_action (2-5 words, e.g. \"Open Saturday 9am\"), contact (one line: \
                     address · phone · website).\n\
                     3. {IMAGES} The flyer has one image placeholder: hero_image.\n\
                     4. {FINISH}"
                ),
            )
        }
        "make_trifold" => {
            let business = get_str(args, "business").unwrap_or("the user's business");
            (
                "Trifold brochure workflow",
                format!(
                    "Create a trifold brochure for: {business}.{details}{images}\n\n\
                     The template is US Letter landscape with two sides (page 1 = outside, page 2 = inside), each \
                     folded into three 264pt panels. On the outside the right panel is the front cover, the middle \
                     panel is the back, and the left panel is the flap seen first when the cover is opened.\n\n\
                     1. template_apply {{name: \"trifold_brochure\", title: <short title>}}.\n\
                     2. Fill every text placeholder with text_set {{frame_id, text}}:\n\
                        cover_title (the business name, 1-4 words), cover_subtitle (one tagline), flap_heading + \
                     flap_text (who you are, 60-110 words), contact_heading (e.g. \"Visit Us\") + contact (address, \
                     phone, opening hours, website on separate lines), inside_heading + inside_body (the main story, \
                     120-220 words), features_heading + features (4-6 short lines, one product/service per line).\n\
                     3. Link inside_body to the frame named inside_body_continued (template_apply returns it in \
                     named_frames) with text_link_frames {{from_frame_id, to_frame_id}} so the story can flow into \
                     the middle panel.\n\
                     4. {IMAGES} The brochure has two image placeholders: cover_image and inside_image.\n\
                     5. {FINISH}"
                ),
            )
        }
        "make_business_card" => {
            let person = get_str(args, "name").unwrap_or("the user");
            let line = |k: &str, label: &str| {
                get_str(args, k)
                    .map(|v| format!("\n{label}: {v}"))
                    .unwrap_or_default()
            };
            (
                "Business card workflow",
                format!(
                    "Create a business card for {person}.{}{}{}{}\n\n\
                     1. template_apply {{name: \"business_card\"}} (3.5 x 2 in, 9pt bleed).\n\
                     2. text_set the placeholders: name, job_title, company, contact (up to three short lines: \
                     phone, email, website). Keep every line short; the card is small.\n\
                     3. The logo placeholder is a 60pt square: place the logo file with image_place, or draw a \
                     simple monogram/mark as SVG (square viewBox) and place it with image_place {{frame_id, svg}}.\n\
                     4. {FINISH}",
                    line("job_title", "Job title"),
                    line("company", "Company"),
                    line("contact", "Contact"),
                    line("logo", "Logo file"),
                ),
            )
        }
        "make_newsletter" => {
            let topic = get_str(args, "topic").unwrap_or("the user's group or business");
            let audience = get_str(args, "audience")
                .map(|a| format!("\nAudience: {a}"))
                .unwrap_or_default();
            (
                "Newsletter workflow",
                format!(
                    "Create a four-page newsletter about: {topic}.{audience}{details}{images}\n\n\
                     The template is US Letter portrait. Page 1: masthead, lead headline, photo with caption, an \
                     \"In Brief\" sidebar and the start of the lead story, which flows through two columns and \
                     continues on page 2 (the frames are already linked). Page 2: a pull quote, an \"Upcoming \
                     events\" table and a bulleted list. Page 3: second story with photo, a numbered list and an \
                     opening-hours table. Page 4: back-page story, contact box and a mailing panel.\n\n\
                     1. template_apply {{name: \"newsletter\", title: <short title>}}. Keep the placeholders and \
                     named_frames it returns.\n\
                     2. Fill every text placeholder with text_set {{frame_id, text}} (formatting is kept; \\n \
                     separates paragraphs): masthead (the newsletter name), issue_date, lead_headline, lead_caption, \
                     sidebar_heading + sidebar_text (2-3 short items), lead_story (250-400 words, several \
                     paragraphs; it flows to page 2), pull_quote (one striking sentence from the story), \
                     second_headline, second_caption, second_story (90-140 words), back_headline, back_story \
                     (120-200 words), contact_heading + contact_text, return_address and mailing_address.\n\
                     3. Lists: news_list is a bulleted list and steps is a numbered list. text_set them with one \
                     item per line (\\n); the bullets and numbers are added for you.\n\
                     4. Tables: the frames named events_table (page 2, 4 rows x 3 columns: Date, Event, Where) \
                     and hours_table (page 3, 5 rows x 3 columns: Day, Opens, Closes) come back in named_frames. \
                     Replace the sample text of every cell with table_set_cell_text {{frame_id, row, col, text}}; \
                     keep the header row and leave no cell empty.\n\
                     5. {IMAGES} The newsletter has two image placeholders: lead_image and second_image. After \
                     each image_place call image_set_alt_text {{frame_id, alt_text}} describing the picture.\n\
                     6. {FINISH}"
                ),
            )
        }
        "make_greeting_card" => {
            let occasion = get_str(args, "occasion").unwrap_or("a special occasion");
            let line = |k: &str, label: &str| {
                get_str(args, k)
                    .map(|v| format!("\n{label}: {v}"))
                    .unwrap_or_default()
            };
            (
                "Greeting card workflow",
                format!(
                    "Create a quarter-fold greeting card for: {occasion}.{}{}{images}\n\n\
                     The template is one US Letter portrait page printed on one side and folded twice, with fold \
                     guides at the centre lines. The bottom half is the outside: the back panel on the left and \
                     the front cover on the right. The top half is the inside spread, printed upside down on \
                     purpose (rotated 180 degrees) so it reads correctly after folding; do not rotate it back.\n\n\
                     1. template_apply {{name: \"greeting_card\", title: <short title>}}.\n\
                     2. Fill every text placeholder with text_set {{frame_id, text}}: front_greeting (2-4 words, \
                     e.g. \"Happy Birthday!\"), front_subtitle (one short line), inside_message (30-60 words, \
                     warm and personal), inside_signature (a sign-off line and a name, two lines), \
                     inside_caption (a few words about the inside picture) and back_credit (two short lines).\n\
                     3. {IMAGES} The card has two image placeholders: front_picture (the cover) and \
                     inside_picture. After each image_place call image_set_alt_text {{frame_id, alt_text}}.\n\
                     4. {FINISH} Check the inside text is upside down on the page image: that is expected.",
                    line("recipient", "Recipient"),
                    line("message", "Message to include"),
                ),
            )
        }
        "make_mail_merge" => {
            let piece = get_str(args, "piece").unwrap_or("the mailing");
            let data = get_str(args, "data").unwrap_or("<the data file>");
            let sheet = get_str(args, "sheet")
                .map(|s| format!(", sheet: \"{s}\""))
                .unwrap_or_default();
            (
                "Mail merge workflow",
                format!(
                    "Create {piece} merged from the list in {data}.{details}\n\n\
                     1. doc_new with the page size of the piece (title: <short title>); the page set is printed once \
                     per record, so design ONE piece. For a letter use one page; for a card use its size.\n\
                     2. merge_set_source {{path: \"{data}\"{sheet}}}. Read the fields and record_count it returns, then \
                     merge_fields_list to see a sample value for each field. Fix a bad header in the file rather than \
                     working around it.\n\
                     3. frame_add_text for the text. Write it in reading order by appending: text_insert {{frame_id, \
                     text}} for fixed words, merge_insert_field {{frame_id, field}} for each field (both add at the end \
                     when offset is omitted; a field counts as one character). Use one space between a field and the \
                     next word: an empty value takes one adjacent space with it. Style with text_apply_char_style and \
                     text_apply_para_style as usual.\n\
                     4. merge_preview {{index: 0, on: true}}, then render_preview; step through a few records, and \
                     the longest values (index of the longest name) to check that nothing overflows. text_overflow_status \
                     describes the previewed record only.\n\
                     5. validate: fix every warning; merge_field_missing means a field name that is not in the data.\n\
                     6. Save the design with doc_save_as (the data is stored inside it), then merge_export {{format: \"pdf\", \
                     path: \"<short-name>.pdf\"}}; its page_count must equal the record count. Use format \"publication\" \
                     to get an editable .pubrs with plain text instead. Tell the user the file paths."
                ),
            )
        }
        "make_labels" => {
            let data = get_str(args, "data").unwrap_or("<the data file>");
            let preset = get_str(args, "preset").unwrap_or("avery-5160");
            let sheet = get_str(args, "sheet")
                .map(|s| format!(", sheet: \"{s}\""))
                .unwrap_or_default();
            let lines = get_str(args, "lines")
                .map(|l| format!("\nEach label shows: {l}"))
                .unwrap_or_default();
            (
                "Label sheet workflow",
                format!(
                    "Print labels from the list in {data} on {preset} sheets.{lines}\n\n\
                     1. sheet_presets_list; find {preset} and note label_width and label_height (points).\n\
                     2. doc_new {{page_size: {{width: label_width, height: label_height}}, title: <short title>}}, then \
                     doc_set_sheet {{preset: \"{preset}\", fill: \"sequence\"}}: the one page you design is a single \
                     label, and every record becomes the next label on the sheet.\n\
                     3. merge_set_source {{path: \"{data}\"{sheet}}}; check fields and record_count in the result.\n\
                     4. frame_add_text inset 4pt from the label edge. Build the text in reading order with text_insert \
                     (use \\n for a new line) and merge_insert_field, e.g. name field, newline, street field, newline, \
                     city field, \", \", state field, \" \", zip field. Use a small font (9-11pt) so the longest \
                     address fits the label.\n\
                     5. merge_preview {{index: 0, on: true}}, render_preview, and look at the longest records; \
                     text_overflow_status must report any_overflow: false. validate must return ok: true.\n\
                     6. merge_export {{format: \"pdf\", path: \"<short-name>.pdf\"}}: page_count is the number of sheets \
                     (30 labels per sheet for avery-5160). Save the design with doc_save_as. Tell the user the file \
                     paths."
                ),
            )
        }
        other => {
            return Err(ErrorData::invalid_params(
                format!("unknown prompt: {other}"),
                Some(serde_json::json!({"prompts": [
                    "make_flyer",
                    "make_trifold",
                    "make_business_card",
                    "make_newsletter",
                    "make_greeting_card",
                    "make_mail_merge",
                    "make_labels"
                ]})),
            ));
        }
    };
    Ok(
        GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
            .with_description(description),
    )
}
