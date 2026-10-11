//! `pubrs-render`: draws a laid-out document. Never mutates it.
//!
//! [`scene::build_page_scene`] turns a page into a backend-independent
//! display list; [`raster::RasterRenderer`] rasterises it with tiny-skia
//! (deterministic, used for PNG export and golden tests) and
//! [`tiles::TileRenderer`] does the same in parallel tiles for the GUI's
//! viewport, byte for byte. [`damage::page_damage`] finds what an edit
//! changed on a page. With the `gpu` feature, [`backend::Rasterizer::auto`]
//! prefers vello for one-shot whole-page renders and falls back to tiny-skia
//! when no adapter exists.

pub mod backend;
pub mod damage;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod images;
pub mod pdf;
pub mod raster;
pub mod scene;
pub mod tiles;

pub use backend::Rasterizer;
pub use damage::{PageDamage, PageState, page_damage, page_fingerprint};
pub use images::{
    ImageContent, ImageResolver, ImageSnapshot, NoImages, RasterImage, pixmap_from_rgba,
};
pub use pdf::{FontEmbedding, PdfError, PdfOptions, render_pdf};
pub use raster::{PixelRect, RasterOptions, RasterRenderer, Region, RenderError, pixel_size};
pub use resvg::tiny_skia;
pub use scene::{
    Affine, Node, PageScene, Prim, SceneOptions, build_page_scene, build_page_scene_with,
    frame_node, frame_node_with, frame_transform,
};
pub use tiles::{TileOptions, TileRenderer};
