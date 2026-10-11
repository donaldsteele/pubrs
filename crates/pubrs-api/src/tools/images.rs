//! Image tools: image_place, image_crop, image_fit, image_replace.

use base64::Engine;
use pubrs_core::{AssetId, Command, Crop, DocumentId, FrameId, ImageFit, Rect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::session::{Changed, Session};

/// Largest image accepted, in bytes.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImagePlace {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// An image (picture) frame; create one with frame_add_image.
    pub frame_id: FrameId,
    /// PNG, JPEG or SVG file path. Or give base64.
    #[serde(default)]
    pub path: Option<String>,
    /// Image bytes, base64-encoded.
    #[serde(default)]
    pub base64: Option<String>,
    /// SVG markup as text (e.g. "<svg xmlns=..."). Handy for drawn artwork
    /// when no photo is available; scales without losing quality.
    #[serde(default)]
    pub svg: Option<String>,
    /// Display name for base64 images.
    #[serde(default)]
    pub filename: Option<String>,
    /// fill (default), fit or stretch.
    #[serde(default)]
    pub fit: Option<ImageFit>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImagePlaceOut {
    pub frame_id: FrameId,
    pub asset_id: AssetId,
    /// Pixel size of the image.
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// Print resolution in this frame (warnings below 150).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_dpi: Option<f64>,
    #[serde(flatten)]
    pub change: Changed,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageCrop {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// Fraction (0..1) of the image width trimmed from the left.
    #[serde(default)]
    pub left: f64,
    /// Fraction (0..1) of the image height trimmed from the top.
    #[serde(default)]
    pub top: f64,
    /// Fraction (0..1) of the image width trimmed from the right.
    #[serde(default)]
    pub right: f64,
    /// Fraction (0..1) of the image height trimmed from the bottom.
    #[serde(default)]
    pub bottom: f64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageFitReq {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    pub frame_id: FrameId,
    /// fill: cover the frame (may crop); fit: whole image visible; stretch: distort to fill.
    pub mode: ImageFit,
    /// Also resize the frame (keeping its width) to the cropped image's aspect ratio.
    #[serde(default)]
    pub resize_frame: bool,
}

impl Session {
    /// Read image bytes from a path or base64 and build an `add_asset` command.
    pub(crate) fn import_asset(
        &mut self,
        path: Option<&str>,
        base64: Option<&str>,
        svg: Option<&str>,
        filename: Option<&str>,
    ) -> ApiResult<(Command, AssetId)> {
        let (name, bytes) = match (path, base64) {
            _ if svg.is_some() => (
                filename.unwrap_or("artwork.svg").to_string(),
                svg.unwrap_or_default().as_bytes().to_vec(),
            ),
            (Some(p), _) => {
                let p = &self.resolve_path(p).display().to_string();
                let meta = std::fs::metadata(p).map_err(|e| {
                    ApiError::new(
                        "file_not_found",
                        format!("cannot read image {p}: {e}"),
                        "check the path; it is resolved on the machine running pubrs",
                    )
                })?;
                if meta.len() as usize > MAX_IMAGE_BYTES {
                    return Err(ApiError::invalid(
                        "image is larger than 64 MB",
                        "resize it first",
                    ));
                }
                let bytes = std::fs::read(p).map_err(|e| {
                    ApiError::new(
                        "io_error",
                        format!("cannot read {p}: {e}"),
                        "check permissions",
                    )
                })?;
                let name = std::path::Path::new(p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "image".into());
                (name, bytes)
            }
            (None, Some(b64)) => {
                let cleaned: String = b64
                    .trim()
                    .trim_start_matches("data:")
                    .split_once("base64,")
                    .map_or(b64.trim(), |(_, d)| d)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(cleaned.as_bytes())
                    .map_err(|e| {
                        ApiError::invalid(format!("invalid base64: {e}"), "send standard base64")
                    })?;
                if bytes.len() > MAX_IMAGE_BYTES {
                    return Err(ApiError::invalid(
                        "image is larger than 64 MB",
                        "resize it first",
                    ));
                }
                (filename.unwrap_or("image").to_string(), bytes)
            }
            (None, None) => {
                return Err(ApiError::invalid(
                    "give path, base64 or svg",
                    "pass a PNG/JPEG/SVG file path, base64 bytes, or SVG markup",
                ));
            }
        };
        let (asset, data) = pubrs_io::import_image(&mut *self.ids, &name, bytes)?;
        let id = asset.id;
        Ok((Command::AddAsset { asset, data }, id))
    }

    fn place(&mut self, req: ImagePlace, label: &str) -> ApiResult<ImagePlaceOut> {
        let doc = self.get(req.doc_id)?.doc();
        let frame = doc.frame(req.frame_id).ok_or_else(|| {
            ApiError::not_found(
                "frame",
                &req.frame_id.to_string(),
                "list frames with frame_list",
            )
        })?;
        if frame.as_image().is_none() {
            return Err(ApiError::invalid(
                "frame is not a picture frame",
                "create one with frame_add_image, or pick an image placeholder",
            ));
        }
        let (add, asset_id) = self.import_asset(
            req.path.as_deref(),
            req.base64.as_deref(),
            req.svg.as_deref(),
            req.filename.as_deref(),
        )?;
        let (pw, ph) = match &add {
            Command::AddAsset { asset, .. } => (asset.pixel_width, asset.pixel_height),
            _ => (0, 0),
        };
        let mut commands = vec![
            add,
            Command::SetImage {
                frame: req.frame_id,
                asset: Some(asset_id),
            },
        ];
        if let Some(fit) = req.fit {
            commands.push(Command::SetImageFit {
                frame: req.frame_id,
                fit,
            });
        }
        let change = self.exec(
            req.doc_id,
            Command::Batch {
                label: label.into(),
                commands,
            },
        )?;
        let dpi = pubrs_core::validate::effective_dpi(self.get(req.doc_id)?.doc(), req.frame_id);
        Ok(ImagePlaceOut {
            frame_id: req.frame_id,
            asset_id,
            pixel_width: pw,
            pixel_height: ph,
            effective_dpi: dpi,
            change,
        })
    }

    pub fn image_place(&mut self, req: ImagePlace) -> ApiResult<ImagePlaceOut> {
        self.place(req, "place image")
    }

    pub fn image_replace(&mut self, req: ImagePlace) -> ApiResult<ImagePlaceOut> {
        let has = self
            .get(req.doc_id)?
            .doc()
            .frame(req.frame_id)
            .and_then(|f| f.as_image())
            .is_some_and(|i| i.asset.is_some());
        if !has {
            return Err(ApiError::invalid(
                "frame has no image to replace",
                "use image_place for empty picture frames",
            ));
        }
        self.place(req, "replace image")
    }

    pub fn image_crop(&mut self, req: ImageCrop) -> ApiResult<Changed> {
        self.exec(
            req.doc_id,
            Command::SetImageCrop {
                frame: req.frame_id,
                crop: Crop {
                    left: req.left,
                    top: req.top,
                    right: req.right,
                    bottom: req.bottom,
                },
            },
        )
    }

    pub fn image_fit(&mut self, req: ImageFitReq) -> ApiResult<Changed> {
        let mut commands = vec![Command::SetImageFit {
            frame: req.frame_id,
            fit: req.mode,
        }];
        if req.resize_frame {
            let doc = self.get(req.doc_id)?.doc();
            let f = doc.frame(req.frame_id).ok_or_else(|| {
                ApiError::not_found(
                    "frame",
                    &req.frame_id.to_string(),
                    "list frames with frame_list",
                )
            })?;
            let img = f.as_image().ok_or_else(|| {
                ApiError::invalid("frame is not a picture frame", "pick an image frame")
            })?;
            let asset = img.asset.and_then(|a| doc.assets.get(&a)).ok_or_else(|| {
                ApiError::invalid("frame has no image", "place one with image_place")
            })?;
            let c = img.crop;
            let w = f64::from(asset.pixel_width) * (1.0 - c.left - c.right);
            let h = f64::from(asset.pixel_height) * (1.0 - c.top - c.bottom);
            let new_h = f.rect.width * h / w.max(1e-6);
            commands.push(Command::SetFrameRect {
                frame: req.frame_id,
                rect: Rect::new(f.rect.x, f.rect.y, f.rect.width, new_h),
            });
        }
        self.exec(
            req.doc_id,
            Command::Batch {
                label: "image fit".into(),
                commands,
            },
        )
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageSetAltText {
    /// Document id (default: the most recently used document).
    #[serde(default)]
    pub doc_id: Option<DocumentId>,
    /// An image (picture) frame.
    pub frame_id: FrameId,
    /// What the picture shows, for screen readers (written to tagged PDF as
    /// /Alt). An empty string clears it.
    pub alt_text: String,
}

impl Session {
    pub fn image_set_alt_text(&mut self, req: ImageSetAltText) -> ApiResult<Changed> {
        if req.alt_text.chars().count() > 1000 {
            return Err(ApiError::invalid(
                "alt text is longer than 1000 characters",
                "describe the picture in a sentence or two",
            ));
        }
        self.exec(
            req.doc_id,
            Command::SetImageAlt {
                frame: req.frame_id,
                alt_text: req.alt_text.trim().to_string(),
            },
        )
    }
}
