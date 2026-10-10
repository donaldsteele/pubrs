//! Render every built-in template page to PNG:
//! `cargo run -p pubrs-io --example render_templates -- out_dir`

use pubrs_io::ExportContext;
use pubrs_io::templates::{BUILTIN, builtin};
use pubrs_layout::FontLibrary;
use pubrs_render::RasterOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&out)?;
    let mut ctx = ExportContext::new(FontLibrary::bundled());
    for name in BUILTIN {
        let doc = builtin(name).expect("template");
        for i in 0..doc.pages.len() {
            let png = ctx.page_png(
                &doc,
                i,
                &RasterOptions {
                    dpi: 60.0,
                    include_bleed: false,
                    ..Default::default()
                },
            )?;
            let path = out.join(format!("{name}-{}.png", i + 1));
            std::fs::write(&path, png)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}
