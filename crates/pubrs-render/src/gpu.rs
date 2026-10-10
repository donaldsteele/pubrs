//! GPU rasteriser (vello on wgpu). Same input and output as
//! [`crate::raster::RasterRenderer`]; used by the GUI when an adapter exists.

use std::collections::HashMap;

use pubrs_core::{AssetId, Color};
use resvg::tiny_skia as ts;
use vello::kurbo::{self, Shape as _};
use vello::peniko::{self, Blob, Fill, FontData, ImageAlphaType, ImageBrush, ImageData};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene, wgpu};

use crate::images::{ImageContent, ImageResolver};
use crate::raster::{
    MAX_PIXELS_PER_SIDE, RasterOptions, RenderError, dash_pattern, device_transform, pixel_size,
};
use crate::scene::{Affine, Geom, PageScene, Prim};

/// GPU set-up failures (the caller falls back to tiny-skia).
#[derive(Debug, Clone, thiserror::Error)]
pub enum GpuError {
    #[error("no GPU adapter: {0}")]
    NoAdapter(String),
    #[error("GPU device error: {0}")]
    Device(String),
    #[error(transparent)]
    Render(#[from] RenderError),
}

fn k(a: Affine) -> kurbo::Affine {
    kurbo::Affine::new(a.0)
}

fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn shape(g: Geom) -> kurbo::BezPath {
    match g {
        Geom::Rect { w, h, radius } => {
            kurbo::RoundedRect::new(0.0, 0.0, w, h, radius).to_path(0.01)
        }
        Geom::Ellipse { w, h } => {
            kurbo::Ellipse::from_rect(kurbo::Rect::new(0.0, 0.0, w, h)).to_path(0.01)
        }
        Geom::Line { x0, y0, x1, y1 } => kurbo::Line::new((x0, y0), (x1, y1)).to_path(0.01),
    }
}

/// vello renderer with its own (or a shared) wgpu device.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    adapter: String,
    images: HashMap<AssetId, ImageData>,
}

impl std::fmt::Debug for GpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuRenderer")
            .field("adapter", &self.adapter)
            .finish()
    }
}

impl GpuRenderer {
    /// Create a device on the best available adapter.
    pub fn new() -> Result<Self, GpuError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| GpuError::NoAdapter(e.to_string()))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("pubrs vello"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| GpuError::Device(e.to_string()))?;
        Self::from_device(device, queue, format!("{} ({:?})", info.name, info.backend))
    }

    /// Use an existing device (e.g. the GUI's).
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter: String,
    ) -> Result<Self, GpuError> {
        let renderer = Renderer::new(
            &device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(|e| GpuError::Device(e.to_string()))?;
        Ok(Self {
            device,
            queue,
            renderer,
            adapter,
            images: HashMap::new(),
        })
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Render to straight-alpha RGBA8 (width, height, pixels).
    pub fn render_rgba(
        &mut self,
        page: &PageScene,
        images: &dyn ImageResolver,
        opts: &RasterOptions,
    ) -> Result<(u32, u32, Vec<u8>), GpuError> {
        let (w, h) = pixel_size(page, opts);
        let max = self
            .device
            .limits()
            .max_texture_dimension_2d
            .min(MAX_PIXELS_PER_SIDE);
        if w > max || h > max {
            return Err(RenderError::TooLarge(w, h).into());
        }
        let scene = self.build(page, images, opts, w, h);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pubrs page"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let base = opts.background.map_or(peniko::Color::TRANSPARENT, color);
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                &scene,
                &view,
                &RenderParams {
                    base_color: base,
                    width: w,
                    height: h,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| GpuError::Device(e.to_string()))?;
        let unpadded = w * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pubrs readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| GpuError::Device(e.to_string()))?;
        rx.recv()
            .map_err(|e| GpuError::Device(e.to_string()))?
            .map_err(|e| GpuError::Device(e.to_string()))?;
        let mapped = buf
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuError::Device(e.to_string()))?;
        let mut out = Vec::with_capacity((unpadded * h) as usize);
        for row in mapped.chunks_exact(padded as usize) {
            out.extend_from_slice(&row[..unpadded as usize]);
        }
        drop(mapped);
        buf.unmap();
        Ok((w, h, out))
    }

    fn image_data(&mut self, asset: AssetId, content: &ImageContent) -> Option<ImageData> {
        let ImageContent::Raster(img) = content else {
            return None;
        };
        Some(
            self.images
                .entry(asset)
                .or_insert_with(|| ImageData {
                    data: Blob::from(img.pixmap.data().to_vec()),
                    format: peniko::ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::AlphaPremultiplied,
                    width: img.width,
                    height: img.height,
                })
                .clone(),
        )
    }

    fn build(
        &mut self,
        page: &PageScene,
        images: &dyn ImageResolver,
        opts: &RasterOptions,
        w: u32,
        h: u32,
    ) -> Scene {
        let mut scene = Scene::new();
        let base = device_transform(page, opts);
        let device_rect = kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
        for node in &page.nodes {
            let t = node.transform.then(base);
            let layered = node.opacity < 1.0;
            if layered {
                scene.push_layer(
                    Fill::NonZero,
                    peniko::Mix::Normal,
                    node.opacity as f32,
                    kurbo::Affine::IDENTITY,
                    &device_rect,
                );
            }
            for prim in &node.prims {
                self.draw_prim(&mut scene, prim, t, images, w, h);
            }
            if layered {
                scene.pop_layer();
            }
        }
        scene
    }

    fn draw_prim(
        &mut self,
        scene: &mut Scene,
        prim: &Prim,
        t: Affine,
        images: &dyn ImageResolver,
        w: u32,
        h: u32,
    ) {
        match prim {
            Prim::Fill { geom, color: c } => {
                scene.fill(Fill::NonZero, k(t), color(*c), None, &shape(*geom));
            }
            Prim::Stroke {
                geom,
                color: c,
                width,
                dash,
            } => {
                let mut stroke = kurbo::Stroke::new(*width);
                if let Some(d) = dash_pattern(*dash, *width) {
                    stroke = stroke.with_dashes(0.0, d);
                }
                scene.stroke(&stroke, k(t), color(*c), None, &shape(*geom));
            }
            Prim::Rect { rect, color: c } => {
                let r = kurbo::Rect::new(rect.x, rect.y, rect.right(), rect.bottom());
                scene.fill(Fill::NonZero, k(t), color(*c), None, &r);
            }
            Prim::Image {
                asset,
                transform,
                clip,
            } => {
                let Some(content) = images.image(*asset) else {
                    return;
                };
                let clip_rect = kurbo::Rect::new(clip.x, clip.y, clip.right(), clip.bottom());
                scene.push_clip_layer(Fill::NonZero, k(t), &clip_rect);
                match &content {
                    ImageContent::Raster(_) => {
                        if let Some(data) = self.image_data(*asset, &content) {
                            let brush =
                                ImageBrush::new(data).with_quality(peniko::ImageQuality::High);
                            scene.draw_image(&brush, k(transform.then(t)));
                        }
                    }
                    ImageContent::Svg(tree) => {
                        // vello has no SVG renderer: rasterise at device resolution.
                        if let Some(mut pm) = ts::Pixmap::new(w, h) {
                            resvg::render(
                                tree,
                                crate::raster::to_ts(transform.then(t)),
                                &mut pm.as_mut(),
                            );
                            let data = ImageData {
                                data: Blob::from(pm.take()),
                                format: peniko::ImageFormat::Rgba8,
                                alpha_type: ImageAlphaType::AlphaPremultiplied,
                                width: w,
                                height: h,
                            };
                            scene.draw_image(&ImageBrush::new(data), kurbo::Affine::IDENTITY);
                        }
                    }
                }
                scene.pop_layer();
            }
            Prim::Glyphs(run) => {
                let font = FontData::new(run.font.data.clone(), run.font.index);
                let skew = run
                    .synthetic_skew
                    .map(|d| kurbo::Affine::skew(-d.to_radians().tan(), 0.0));
                let mut draw = scene
                    .draw_glyphs(&font)
                    .font_size(run.font_size as f32)
                    .transform(k(t))
                    .glyph_transform(skew)
                    .hint(false)
                    .brush(color(run.color));
                if run.synthetic_bold {
                    let amount = run.font_size * 0.02;
                    draw = draw.font_embolden(vello::FontEmbolden::new(kurbo::Diagonal2::new(
                        amount, amount,
                    )));
                }
                draw.draw(
                    Fill::NonZero,
                    run.glyphs.iter().map(|g| vello::Glyph {
                        id: g.id,
                        x: g.x as f32,
                        y: g.y as f32,
                    }),
                );
            }
        }
    }
}
