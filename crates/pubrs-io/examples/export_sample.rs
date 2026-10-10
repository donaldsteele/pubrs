//! Build a sample two-page document and export it to PDF and PNG:
//! `cargo run -p pubrs-io --example export_sample -- out_dir`

use pubrs_core::*;
use pubrs_io::ExportContext;
use pubrs_layout::FontLibrary;
use pubrs_render::{PdfOptions, RasterOptions};

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120">
<circle cx="60" cy="60" r="56" fill="#B45309"/><text x="60" y="72" text-anchor="middle"
font-family="Liberation Serif" font-size="34" fill="#FEF3C7">GC</text></svg>"##;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out)?;
    let mut b = DocBuilder::new(SeqIds::new(42), PageSetup::default());
    b.exec(Command::SetBleed { bleed: 9.0 })?;
    b.exec(Command::SetMeta {
        title: Some("Golden Crust".into()),
        author: None,
    })?;
    let p1 = b.page(0);
    let p2 = b.add_page()?;
    let band = b.shape(
        p1,
        Rect::new(-9.0, -9.0, 630.0, 180.0),
        Shape::Rectangle { corner_radius: 0.0 },
    )?;
    b.style(
        band,
        FrameStyle {
            fill: Some(Color::rgb(30, 58, 138)),
            stroke: None,
            opacity: 1.0,
        },
    )?;
    let (logo, data) = pubrs_io::import_image(&mut b.ids, "logo.svg", SVG.as_bytes().to_vec())?;
    let logo_id = logo.id;
    b.exec(Command::AddAsset { asset: logo, data })?;
    b.image(p1, Rect::new(470.0, 30.0, 110.0, 110.0), Some(logo_id))?;
    let title = b.text(
        p1,
        Rect::new(36.0, 40.0, 420.0, 100.0),
        "Golden Crust Bakery",
    )?;
    let ts = b.story_of(title);
    b.exec(Command::ApplyCharProps {
        story: ts,
        start: 0,
        end: 19,
        props: CharProps {
            font_size: Some(40.0),
            bold: Some(true),
            color: Some(Color::WHITE),
            ..Default::default()
        },
    })?;
    let body = "Fresh sourdough, rye and seeded loaves baked every morning from 6am. ".repeat(30);
    let a = b.text(p1, Rect::new(36.0, 200.0, 260.0, 540.0), &body)?;
    let c = b.text(p1, Rect::new(316.0, 200.0, 260.0, 540.0), "")?;
    let d = b.text(p2, Rect::new(36.0, 36.0, 540.0, 300.0), "")?;
    b.link(a, c)?;
    b.link(c, d)?;
    let doc = b.finish();
    let mut ctx = ExportContext::new(FontLibrary::bundled());
    let pdf = ctx.export_pdf(
        &doc,
        &PdfOptions {
            bleed: true,
            crop_marks: true,
            ..Default::default()
        },
    )?;
    std::fs::write(out.join("sample.pdf"), pdf)?;
    let png = ctx.page_png(
        &doc,
        0,
        &RasterOptions {
            dpi: 72.0,
            ..Default::default()
        },
    )?;
    std::fs::write(out.join("sample-page1.png"), png)?;
    pubrs_io::save(&doc, &out.join("sample.pubrs"), None)?;
    println!(
        "wrote sample.pdf, sample-page1.png and sample.pubrs to {}",
        out.display()
    );
    Ok(())
}
