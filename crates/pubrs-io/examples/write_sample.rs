//! Write a small sample `.pubrs` file: `cargo run -p pubrs-io --example write_sample -- out.pubrs`

use pubrs_core::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample.pubrs".to_string());
    let mut ids = SeqIds::new(1);
    let mut ed = Editor::new(Document::new(&mut ids, PageSetup::default()));
    let page = ed.doc().pages[0].id;
    let frame = FrameId::new(&mut ids);
    let story = StoryId::new(&mut ids);
    let mut s = Story::new(story, frame);
    s.insert_text(0, "Hello from pubrs\nA sample document.", None);
    ed.execute(Command::AddFrame {
        parent: Parent::Page(page),
        index: None,
        frame: Frame::new(
            frame,
            Rect::new(72.0, 72.0, 468.0, 144.0),
            FrameContent::Text(TextFrame::new(story)),
        ),
        story: Some(s),
    })?;
    pubrs_io::save(ed.doc(), std::path::Path::new(&path), None)?;
    let back = pubrs_io::load(std::path::Path::new(&path))?;
    assert_eq!(&back, ed.doc());
    println!("wrote {path} and read it back identically");
    Ok(())
}
