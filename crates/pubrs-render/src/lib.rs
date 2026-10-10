//! `pubrs-render`: draws a laid-out document. Never mutates it.
//!
//! [`scene::build_page_scene`] turns a page into a backend-independent
//! display list; [`raster::RasterRenderer`] rasterises it with tiny-skia
//! (deterministic, used for PNG export and golden tests). With the `gpu`
//! feature, [`backend::Rasterizer::auto`] prefers vello and falls back to
//! tiny-skia when no adapter exists.

pub mod backend;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod images;
pub mod pdf;
pub mod raster;
pub mod scene;

pub use backend::Rasterizer;
pub use images::{ImageContent, ImageResolver, NoImages, RasterImage, pixmap_from_rgba};
pub use pdf::{FontEmbedding, PdfError, PdfOptions, render_pdf};
pub use raster::{RasterOptions, RasterRenderer, RenderError, pixel_size};
pub use resvg::tiny_skia;
pub use scene::{Affine, Node, PageScene, Prim, build_page_scene, frame_node, frame_transform};
