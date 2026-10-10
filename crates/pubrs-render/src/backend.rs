//! Choosing a raster backend: vello on a GPU when available (feature
//! `gpu`), otherwise tiny-skia. The active path is logged.

use crate::images::ImageResolver;
use crate::raster::{RasterOptions, RasterRenderer, RenderError};
use crate::scene::PageScene;

/// A page rasteriser producing straight-alpha RGBA8.
pub enum Rasterizer {
    Cpu(RasterRenderer),
    #[cfg(feature = "gpu")]
    Gpu(Box<crate::gpu::GpuRenderer>),
}

impl std::fmt::Debug for Rasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

impl Rasterizer {
    /// Software rendering only.
    pub fn cpu() -> Self {
        Rasterizer::Cpu(RasterRenderer::new())
    }

    /// GPU if the `gpu` feature is on and an adapter exists, else software.
    pub fn auto() -> Self {
        #[cfg(feature = "gpu")]
        match crate::gpu::GpuRenderer::new() {
            Ok(g) => {
                tracing::info!(adapter = g.adapter(), "page rendering: vello (GPU)");
                return Rasterizer::Gpu(Box::new(g));
            }
            Err(e) => tracing::warn!("GPU rendering unavailable ({e}); using tiny-skia"),
        }
        tracing::info!("page rendering: tiny-skia (software)");
        Self::cpu()
    }

    /// Human-readable description of the active path.
    pub fn describe(&self) -> String {
        match self {
            Rasterizer::Cpu(_) => "tiny-skia (software)".into(),
            #[cfg(feature = "gpu")]
            Rasterizer::Gpu(g) => format!("vello (GPU: {})", g.adapter()),
        }
    }

    /// Render to (width, height, straight RGBA8). A GPU failure switches
    /// permanently to software rendering.
    pub fn render_rgba(
        &mut self,
        scene: &PageScene,
        images: &dyn ImageResolver,
        opts: &RasterOptions,
    ) -> Result<(u32, u32, Vec<u8>), RenderError> {
        #[cfg(feature = "gpu")]
        if let Rasterizer::Gpu(g) = self {
            match g.render_rgba(scene, images, opts) {
                Ok(out) => return Ok(out),
                Err(crate::gpu::GpuError::Render(e)) => return Err(e),
                Err(e) => {
                    tracing::warn!("GPU render failed ({e}); switching to tiny-skia");
                    *self = Self::cpu();
                }
            }
        }
        match self {
            Rasterizer::Cpu(r) => {
                let pm = r.render(scene, images, opts)?;
                let (w, h) = (pm.width(), pm.height());
                Ok((w, h, pm.take_demultiplied()))
            }
            #[cfg(feature = "gpu")]
            Rasterizer::Gpu(_) => unreachable!("handled above"),
        }
    }
}
