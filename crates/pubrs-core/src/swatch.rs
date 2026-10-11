//! Swatches, colour schemes and paints.
//!
//! A [`Paint`] is what fills, strokes and text colour store: the old hex
//! string, a reference to a document [`Swatch`] with a tint, or (frame fills
//! only) a gradient. Swatch values are resolved when drawing, never when
//! laying out, so editing a swatch or applying a [`Scheme`] repaints without
//! relayout. Text carries a small `Copy` [`Ink`] for that reason.
//!
//! Screen colour for CMYK is the plain formula (no colour management);
//! [`ResolvedInk::spec`] keeps the exact CMYK or spot values for PDF output.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::color::Color;
use crate::id::SwatchId;
use crate::model::Document;

/// A CMYK colour, each channel 0 to 100 percent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cmyk {
    pub c: f64,
    pub m: f64,
    pub y: f64,
    pub k: f64,
}

impl Cmyk {
    pub fn new(c: f64, m: f64, y: f64, k: f64) -> Self {
        Self { c, m, y, k }
    }

    pub fn is_valid(&self) -> bool {
        [self.c, self.m, self.y, self.k]
            .iter()
            .all(|v| v.is_finite() && (0.0..=100.0).contains(v))
    }

    /// Screen colour by the plain formula: `r = 255 (1 - c)(1 - k)`, and
    /// likewise for green and blue.
    pub fn to_rgb(&self) -> Color {
        let ch = |v: f64| {
            let x = 255.0 * (1.0 - v / 100.0) * (1.0 - self.k / 100.0);
            x.round().clamp(0.0, 255.0) as u8
        };
        Color::rgb(ch(self.c), ch(self.m), ch(self.y))
    }
}

/// What a swatch is made of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SwatchValue {
    /// An sRGB colour.
    Rgb(Color),
    /// Process colour, shown on screen by formula.
    Cmyk(Cmyk),
    /// A named spot (ink) colour. `cmyk` is its process alternative, used for
    /// the screen and for printers that cannot use the spot ink.
    Spot { name: String, cmyk: Cmyk },
}

/// A named colour in the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Swatch {
    pub id: SwatchId,
    pub name: String,
    pub value: SwatchValue,
}

impl Swatch {
    /// Check names and ranges.
    pub fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("a swatch needs a name".into());
        }
        match &self.value {
            SwatchValue::Rgb(_) => Ok(()),
            SwatchValue::Cmyk(c) => c
                .is_valid()
                .then_some(())
                .ok_or_else(|| "CMYK values must be 0 to 100".to_string()),
            SwatchValue::Spot { name, cmyk } => {
                if name.trim().is_empty() {
                    Err("a spot colour needs an ink name".into())
                } else if !cmyk.is_valid() {
                    Err("CMYK values must be 0 to 100".into())
                } else {
                    Ok(())
                }
            }
        }
    }
}

/// The swatch list and scheme together (what a colour patch replaces).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Palette {
    #[serde(default)]
    pub swatches: Arc<Vec<Swatch>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<SchemeId>,
}

/// Which colour scheme a document uses.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SchemeId(pub String);

impl fmt::Debug for SchemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SchemeId({})", self.0)
    }
}

impl fmt::Display for SchemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Gradient shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    Linear,
    Radial,
}

/// One colour stop of a gradient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GradientStop {
    /// Position along the gradient, 0 to 1.
    pub offset: f64,
    /// A hex colour or a swatch (never another gradient).
    pub color: SolidPaint,
}

/// A colour or swatch reference: a [`Paint`] that is not a gradient.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged, deny_unknown_fields)]
pub enum SolidPaint {
    /// `"#RRGGBB"` or `"#RRGGBBAA"`.
    Color(Color),
    /// A document swatch at a tint (0 to 100 percent).
    Swatch {
        swatch: SwatchId,
        #[serde(default = "full_tint", skip_serializing_if = "is_full_tint")]
        tint: f64,
    },
}

impl SolidPaint {
    pub const fn swatch(id: SwatchId) -> SolidPaint {
        SolidPaint::Swatch {
            swatch: id,
            tint: 100.0,
        }
    }

    /// The same colour as a [`Paint`].
    pub fn to_paint(&self) -> Paint {
        match self {
            SolidPaint::Color(c) => Paint::Color(*c),
            SolidPaint::Swatch { swatch, tint } => Paint::Swatch {
                swatch: *swatch,
                tint: *tint,
            },
        }
    }
}

impl From<Color> for SolidPaint {
    fn from(c: Color) -> Self {
        SolidPaint::Color(c)
    }
}

/// Percent of full strength, as stored.
fn full_tint() -> f64 {
    100.0
}

fn is_full_tint(t: &f64) -> bool {
    *t == 100.0
}

/// A colour specification used by fills, strokes and text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged, deny_unknown_fields)]
pub enum Paint {
    /// `"#RRGGBB"` or `"#RRGGBBAA"`.
    Color(Color),
    /// A document swatch at a tint (0 to 100 percent; 100 = full strength).
    Swatch {
        swatch: SwatchId,
        #[serde(default = "full_tint", skip_serializing_if = "is_full_tint")]
        tint: f64,
    },
    /// A gradient (frame fills only). `angle` is degrees clockwise from
    /// left-to-right; radial gradients ignore it.
    Gradient {
        gradient: GradientKind,
        stops: Vec<GradientStop>,
        #[serde(default)]
        angle: f64,
    },
}

impl From<Color> for Paint {
    fn from(c: Color) -> Self {
        Paint::Color(c)
    }
}

impl Paint {
    /// A swatch at full strength.
    pub const fn swatch(id: SwatchId) -> Paint {
        Paint::Swatch {
            swatch: id,
            tint: 100.0,
        }
    }

    pub fn is_gradient(&self) -> bool {
        matches!(self, Paint::Gradient { .. })
    }

    /// The `Copy` form used by text; `None` for gradients.
    pub fn ink(&self) -> Option<Ink> {
        match self {
            Paint::Color(c) => Some(Ink::Rgba(*c)),
            Paint::Swatch { swatch, tint } => Some(Ink::Swatch {
                id: *swatch,
                tint: *tint as f32,
            }),
            Paint::Gradient { .. } => None,
        }
    }

    /// Swatches this paint refers to.
    pub fn swatch_ids(&self) -> Vec<SwatchId> {
        match self {
            Paint::Color(_) => Vec::new(),
            Paint::Swatch { swatch, .. } => vec![*swatch],
            Paint::Gradient { stops, .. } => stops
                .iter()
                .flat_map(|s| s.color.to_paint().swatch_ids())
                .collect(),
        }
    }

    /// Replace references to `from` with `to` (used when deleting a swatch).
    pub fn replace_swatch(&mut self, from: SwatchId, to: &Paint) {
        match self {
            Paint::Swatch { swatch, tint } if *swatch == from => {
                *self = match to {
                    Paint::Swatch {
                        swatch: s2,
                        tint: t2,
                    } => Paint::Swatch {
                        swatch: *s2,
                        tint: *t2 * *tint / 100.0,
                    },
                    other => other.clone(),
                };
            }
            Paint::Gradient { stops, .. } => {
                for s in stops {
                    if !matches!(to, Paint::Gradient { .. }) {
                        let mut p = s.color.to_paint();
                        p.replace_swatch(from, to);
                        match p {
                            Paint::Color(c) => s.color = SolidPaint::Color(c),
                            Paint::Swatch { swatch, tint } => {
                                s.color = SolidPaint::Swatch { swatch, tint }
                            }
                            Paint::Gradient { .. } => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Structural check. `allow_gradient` is true only for frame fills.
    pub fn check(&self, doc: &Document, allow_gradient: bool) -> Result<(), String> {
        match self {
            Paint::Color(_) => Ok(()),
            Paint::Swatch { swatch, tint } => {
                if !(tint.is_finite() && (0.0..=100.0).contains(tint)) {
                    return Err("tint must be 0 to 100 percent".into());
                }
                if doc.swatch(*swatch).is_none() {
                    return Err(format!(
                        "swatch {swatch} does not exist; list swatches with swatch_list"
                    ));
                }
                Ok(())
            }
            Paint::Gradient { stops, angle, .. } => {
                if !allow_gradient {
                    return Err(
                        "gradients are only allowed as frame fills; use a hex colour or a swatch here"
                            .into(),
                    );
                }
                if !angle.is_finite() {
                    return Err("gradient angle must be finite".into());
                }
                if !(2..=16).contains(&stops.len()) {
                    return Err("a gradient needs 2 to 16 stops".into());
                }
                let mut last = 0.0;
                for s in stops {
                    if !(s.offset.is_finite() && (0.0..=1.0).contains(&s.offset)) {
                        return Err("gradient stop offsets must be 0 to 1".into());
                    }
                    if s.offset < last {
                        return Err("gradient stops must be in increasing offset order".into());
                    }
                    last = s.offset;
                    s.color.to_paint().check(doc, false)?;
                }
                Ok(())
            }
        }
    }
}

/// Text colour as layout carries it: `Copy`, resolved at paint time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ink {
    Rgba(Color),
    Swatch { id: SwatchId, tint: f32 },
}

impl Default for Ink {
    fn default() -> Self {
        Ink::Rgba(Color::BLACK)
    }
}

impl From<Ink> for Paint {
    fn from(i: Ink) -> Self {
        match i {
            Ink::Rgba(c) => Paint::Color(c),
            Ink::Swatch { id, tint } => Paint::Swatch {
                swatch: id,
                tint: f64::from(tint),
            },
        }
    }
}

impl From<Color> for Ink {
    fn from(c: Color) -> Self {
        Ink::Rgba(c)
    }
}

/// The exact colour definition behind a resolved ink, for PDF output.
#[derive(Debug, Clone, PartialEq)]
pub enum InkSpec {
    Rgb,
    Cmyk { cmyk: Cmyk, tint: f64 },
    Spot { name: String, cmyk: Cmyk, tint: f64 },
}

/// An ink resolved against the document: the screen colour plus the exact
/// definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedInk {
    pub rgba: Color,
    pub spec: InkSpec,
}

impl From<Color> for ResolvedInk {
    fn from(c: Color) -> Self {
        ResolvedInk {
            rgba: c,
            spec: InkSpec::Rgb,
        }
    }
}

/// Mix towards white: `rgb' = 255 - t (255 - rgb)`, alpha unchanged.
pub fn apply_tint(c: Color, tint: f64) -> Color {
    let t = if tint.is_finite() {
        (tint / 100.0).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let ch = |v: u8| (255.0 - t * (255.0 - v as f64)).round().clamp(0.0, 255.0) as u8;
    Color::rgba(ch(c.r), ch(c.g), ch(c.b), c.a)
}

/// A gradient with its stops resolved to screen colours.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedGradient {
    pub kind: GradientKind,
    pub angle: f64,
    pub stops: Vec<(f64, ResolvedInk)>,
}

/// A fill after resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedPaint {
    Solid(ResolvedInk),
    Gradient(ResolvedGradient),
}

impl ResolvedPaint {
    /// A solid colour for places that cannot draw a gradient (the first stop).
    pub fn solid_rgba(&self) -> Color {
        match self {
            ResolvedPaint::Solid(i) => i.rgba,
            ResolvedPaint::Gradient(g) => g.stops.first().map_or(Color::BLACK, |s| s.1.rgba),
        }
    }
}

/// Shown for a reference to a swatch that no longer exists.
pub const MISSING_SWATCH: Color = Color::rgb(0xFF, 0x00, 0xFF);

impl Document {
    pub fn swatch(&self, id: SwatchId) -> Option<&Swatch> {
        self.swatches.iter().find(|s| s.id == id)
    }

    /// Resolve a text ink.
    pub fn resolve_ink(&self, ink: Ink) -> ResolvedInk {
        match ink {
            Ink::Rgba(c) => ResolvedInk {
                rgba: c,
                spec: InkSpec::Rgb,
            },
            Ink::Swatch { id, tint } => self.resolve_swatch(id, tint as f64),
        }
    }

    /// Resolve a swatch at a tint.
    pub fn resolve_swatch(&self, id: SwatchId, tint: f64) -> ResolvedInk {
        let Some(s) = self.swatch(id) else {
            return ResolvedInk {
                rgba: MISSING_SWATCH,
                spec: InkSpec::Rgb,
            };
        };
        match &s.value {
            SwatchValue::Rgb(c) => ResolvedInk {
                rgba: apply_tint(*c, tint),
                spec: InkSpec::Rgb,
            },
            SwatchValue::Cmyk(c) => ResolvedInk {
                rgba: apply_tint(c.to_rgb(), tint),
                spec: InkSpec::Cmyk {
                    cmyk: *c,
                    tint: tint.clamp(0.0, 100.0),
                },
            },
            SwatchValue::Spot { name, cmyk } => ResolvedInk {
                rgba: apply_tint(cmyk.to_rgb(), tint),
                spec: InkSpec::Spot {
                    name: name.clone(),
                    cmyk: *cmyk,
                    tint: tint.clamp(0.0, 100.0),
                },
            },
        }
    }

    /// The screen colour of a paint, black for none.
    pub fn resolve_paint_or_black(&self, p: Option<&Paint>) -> Color {
        p.map_or(Color::BLACK, |p| self.resolve_paint(p).solid_rgba())
    }

    /// Resolve any paint.
    pub fn resolve_paint(&self, p: &Paint) -> ResolvedPaint {
        match p {
            Paint::Gradient {
                gradient,
                stops,
                angle,
            } => ResolvedPaint::Gradient(ResolvedGradient {
                kind: *gradient,
                angle: *angle,
                stops: stops
                    .iter()
                    .map(|s| (s.offset, self.resolve_solid(&s.color.to_paint())))
                    .collect(),
            }),
            solid => ResolvedPaint::Solid(self.resolve_solid(solid)),
        }
    }

    /// Resolve a solid paint (a gradient resolves to its first stop).
    pub fn resolve_solid(&self, p: &Paint) -> ResolvedInk {
        match p {
            Paint::Color(c) => ResolvedInk {
                rgba: *c,
                spec: InkSpec::Rgb,
            },
            Paint::Swatch { swatch, tint } => self.resolve_swatch(*swatch, *tint),
            Paint::Gradient { stops, .. } => stops.first().map_or(
                ResolvedInk {
                    rgba: Color::BLACK,
                    spec: InkSpec::Rgb,
                },
                |s| self.resolve_solid(&s.color.to_paint()),
            ),
        }
    }
}

impl Document {
    /// Where a swatch is used, for refusal messages.
    pub fn swatch_uses(&self, id: SwatchId) -> Vec<String> {
        let refs = |p: &Paint| p.swatch_ids().contains(&id);
        let mut out = Vec::new();
        for f in self.frames.values() {
            let label = f.name.clone().unwrap_or_else(|| format!("frame {}", f.id));
            if f.style.fill.as_ref().is_some_and(refs) {
                out.push(format!("{label} fill"));
            }
            if f.style.stroke.as_ref().is_some_and(|s| refs(&s.color)) {
                out.push(format!("{label} outline"));
            }
        }
        for s in self.stories.values() {
            let hit = s
                .paragraphs
                .iter()
                .flat_map(|p| &p.runs)
                .filter(|r| r.props.color.as_ref().is_some_and(refs))
                .count();
            if hit > 0 {
                out.push(format!("text in story {} ({hit} runs)", s.id));
            }
        }
        for st in &self.styles.styles {
            if st.character.color.as_ref().is_some_and(refs) {
                out.push(format!("style {:?}", st.name));
            }
        }
        out
    }

    /// Point every use of `from` at `to` instead. A gradient replacement is
    /// refused when the swatch is used by a stroke or by text.
    pub fn replace_swatch_uses(
        &mut self,
        from: SwatchId,
        to: &Paint,
    ) -> Result<(), crate::command::CommandError> {
        use crate::command::CommandError;
        let refs = |p: &Paint| p.swatch_ids().contains(&from);
        let flat = !to.is_gradient();
        let needs_flat = self.frames.values().any(|f| {
            f.style.stroke.as_ref().is_some_and(|s| refs(&s.color))
                || f.style
                    .fill
                    .as_ref()
                    .is_some_and(|p| p.is_gradient() && refs(p))
        }) || self.stories.values().any(|s| {
            s.paragraphs
                .iter()
                .flat_map(|p| &p.runs)
                .any(|r| r.props.color.as_ref().is_some_and(refs))
        }) || self
            .styles
            .styles
            .iter()
            .any(|s| s.character.color.as_ref().is_some_and(refs));
        if !flat && needs_flat {
            return Err(CommandError::invalid(
                "a gradient cannot replace a swatch used by an outline or by text; use a colour or swatch",
            ));
        }
        for f in self.frames.values_mut() {
            let hit = f.style.fill.as_ref().is_some_and(refs)
                || f.style.stroke.as_ref().is_some_and(|s| refs(&s.color));
            if hit {
                let f = Arc::make_mut(f);
                if let Some(p) = &mut f.style.fill {
                    p.replace_swatch(from, to);
                }
                if let Some(s) = &mut f.style.stroke {
                    s.color.replace_swatch(from, to);
                }
            }
        }
        for s in self.stories.values_mut() {
            let hit = s
                .paragraphs
                .iter()
                .flat_map(|p| &p.runs)
                .any(|r| r.props.color.as_ref().is_some_and(refs));
            if hit {
                let s = Arc::make_mut(s);
                for r in s.paragraphs.iter_mut().flat_map(|p| p.runs.iter_mut()) {
                    if let Some(c) = &mut r.props.color {
                        c.replace_swatch(from, to);
                    }
                }
            }
        }
        for st in &mut self.styles.styles {
            if let Some(c) = &mut st.character.color {
                c.replace_swatch(from, to);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// colour schemes

/// The seven colours of a scheme, in slot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemeSlot {
    Main,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Hyperlink,
    FollowedHyperlink,
}

impl SchemeSlot {
    pub const ALL: [SchemeSlot; 7] = [
        SchemeSlot::Main,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Hyperlink,
        SchemeSlot::FollowedHyperlink,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The swatch name, e.g. "Scheme: Accent 1".
    pub fn swatch_name(self) -> &'static str {
        match self {
            SchemeSlot::Main => "Scheme: Main",
            SchemeSlot::Accent1 => "Scheme: Accent 1",
            SchemeSlot::Accent2 => "Scheme: Accent 2",
            SchemeSlot::Accent3 => "Scheme: Accent 3",
            SchemeSlot::Accent4 => "Scheme: Accent 4",
            SchemeSlot::Hyperlink => "Scheme: Hyperlink",
            SchemeSlot::FollowedHyperlink => "Scheme: Followed Hyperlink",
        }
    }

    /// The fixed, well-known id of this slot's swatch in every document.
    pub const fn swatch_id(self) -> SwatchId {
        let mut b = *b"pubrs-scheme-sl\0";
        b[15] = self as u8;
        SwatchId(Uuid::from_bytes(b))
    }

    /// A paint referring to the slot swatch.
    pub const fn paint(self) -> Paint {
        Paint::swatch(self.swatch_id())
    }
}

/// A built-in colour scheme.
#[derive(Debug, Clone, Copy)]
pub struct Scheme {
    pub id: &'static str,
    pub name: &'static str,
    /// Slot colours, in [`SchemeSlot::ALL`] order.
    pub colors: [Color; 7],
}

impl Scheme {
    pub fn color(&self, slot: SchemeSlot) -> Color {
        self.colors[slot.index()]
    }
}

const fn hex(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

macro_rules! scheme {
    ($id:literal, $name:literal, [$a:literal, $b:literal, $c:literal, $d:literal, $e:literal, $f:literal, $g:literal]) => {
        Scheme {
            id: $id,
            name: $name,
            colors: [
                hex($a),
                hex($b),
                hex($c),
                hex($d),
                hex($e),
                hex($f),
                hex($g),
            ],
        }
    };
}

/// The twelve built-in schemes. The first is the default; its colours are the
/// ones the built-in templates were designed with.
pub const SCHEMES: [Scheme; 12] = [
    scheme!(
        "lantern_harbor",
        "Lantern Harbor",
        [
            0x1E3A5F, 0xC2410C, 0xFFF7ED, 0x0F766E, 0xCA8A04, 0x1D4ED8, 0x7E22CE
        ]
    ),
    scheme!(
        "juniper_slate",
        "Juniper Slate",
        [
            0x2D4A43, 0xD97706, 0xF4EFE6, 0x6B8F71, 0x8C4A2F, 0x0F5C8A, 0x6B3F8C
        ]
    ),
    scheme!(
        "meadowlark",
        "Meadowlark",
        [
            0x3F6B2C, 0xE0A526, 0xF6F3DC, 0x8E5B3A, 0x4C8BB3, 0x1B5E8E, 0x7A4B8A
        ]
    ),
    scheme!(
        "copper_dawn",
        "Copper Dawn",
        [
            0x7A2E1D, 0xE6862B, 0xFBEFE0, 0x4A5A6A, 0xB8A05A, 0x1F4E79, 0x6E3B6E
        ]
    ),
    scheme!(
        "glacier_mint",
        "Glacier Mint",
        [
            0x12556B, 0x2BB3A3, 0xE8F6F5, 0xF2994A, 0x7A8FA6, 0x0B63A8, 0x6A4C93
        ]
    ),
    scheme!(
        "plum_orchard",
        "Plum Orchard",
        [
            0x5B2A5C, 0xE4572E, 0xF7ECF3, 0x9BB04F, 0x3E7CB1, 0x2457A6, 0x8A3F7A
        ]
    ),
    scheme!(
        "saffron_market",
        "Saffron Market",
        [
            0xB45309, 0x0E7490, 0xFEF3C7, 0x9F1239, 0x4D7C0F, 0x155E9E, 0x7C3A91
        ]
    ),
    scheme!(
        "midnight_orchid",
        "Midnight Orchid",
        [
            0x1B1F4B, 0xB04FD0, 0xEDEBFA, 0x3FB5A5, 0xF2B134, 0x3151C9, 0x7A3FB5
        ]
    ),
    scheme!(
        "cedar_and_sky",
        "Cedar and Sky",
        [
            0x5C3A21, 0x3A86C8, 0xF3EBDD, 0xC9822B, 0x5E8C61, 0x1E5FA8, 0x74479A
        ]
    ),
    scheme!(
        "coral_reef",
        "Coral Reef",
        [
            0x0F5E73, 0xF26B5B, 0xFDF0E6, 0xF7B538, 0x3D9970, 0x0E6BA8, 0x8B4D9E
        ]
    ),
    scheme!(
        "slate_and_ember",
        "Slate and Ember",
        [
            0x33404D, 0xD9480F, 0xECEFF2, 0x8D99A6, 0xF0A202, 0x2563A8, 0x6B4A93
        ]
    ),
    scheme!(
        "orchard_frost",
        "Orchard Frost",
        [
            0x8A1C2B, 0x3E8E7E, 0xF4F1EA, 0xD4A017, 0x566C86, 0x1D5C9E, 0x7B4A8F
        ]
    ),
];

/// The scheme new template documents start with.
pub const DEFAULT_SCHEME: &str = "lantern_harbor";

/// Look a built-in scheme up by id.
pub fn scheme(id: &str) -> Option<&'static Scheme> {
    SCHEMES.iter().find(|s| s.id == id)
}

/// Is this swatch one of the fixed scheme slots?
pub fn slot_of(id: SwatchId) -> Option<SchemeSlot> {
    SchemeSlot::ALL.into_iter().find(|s| s.swatch_id() == id)
}

/// Write a scheme's colours into the slot swatches, creating missing slots
/// (in slot order, after the existing swatches) and setting `doc.scheme`.
pub fn apply_scheme(doc: &mut Document, scheme: &Scheme) {
    let list = Arc::make_mut(&mut doc.swatches);
    for slot in SchemeSlot::ALL {
        let value = SwatchValue::Rgb(scheme.color(slot));
        match list.iter_mut().find(|s| s.id == slot.swatch_id()) {
            Some(s) => s.value = value,
            None => list.push(Swatch {
                id: slot.swatch_id(),
                name: slot.swatch_name().to_string(),
                value,
            }),
        }
    }
    doc.scheme = Some(SchemeId(scheme.id.to_string()));
}

// ---- reading paints ----
//
// `Paint` and `SolidPaint` are read by hand instead of with an untagged
// derive: serde's untagged enums answer any mistake with "data did not match
// any variant", which hides a typo such as `tint_pct` or a bad hex colour.
// Here a misspelt key names itself and a bad colour says what is wrong.

const PAINT_FIELDS: &[&str] = &["swatch", "tint", "gradient", "stops", "angle"];
const SOLID_FIELDS: &[&str] = &["swatch", "tint"];

struct PaintVisitor {
    gradients: bool,
}

impl<'de> serde::de::Visitor<'de> for PaintVisitor {
    type Value = Paint;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.gradients {
            f.write_str(
                "a colour \"#RRGGBB\" or \"#RRGGBBAA\", a swatch {\"swatch\": id, \"tint\": 0-100} or a gradient {\"gradient\", \"stops\", \"angle\"}",
            )
        } else {
            f.write_str(
                "a colour \"#RRGGBB\" or \"#RRGGBBAA\" or a swatch {\"swatch\": id, \"tint\": 0-100}",
            )
        }
    }

    fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Paint, E> {
        s.parse::<Color>().map(Paint::Color).map_err(|e| {
            E::custom(format!(
                "invalid colour {s:?}: {e}; use \"#RRGGBB\" or \"#RRGGBBAA\" (hex digits only)"
            ))
        })
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Paint, A::Error> {
        use serde::de::Error;
        let fields = if self.gradients {
            PAINT_FIELDS
        } else {
            SOLID_FIELDS
        };
        let mut swatch: Option<SwatchId> = None;
        let mut tint: Option<f64> = None;
        let mut gradient: Option<GradientKind> = None;
        let mut stops: Option<Vec<GradientStop>> = None;
        let mut angle: Option<f64> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "swatch" => swatch = Some(map.next_value()?),
                "tint" => tint = Some(map.next_value()?),
                "gradient" if self.gradients => gradient = Some(map.next_value()?),
                "stops" if self.gradients => stops = Some(map.next_value()?),
                "angle" if self.gradients => angle = Some(map.next_value()?),
                _ => return Err(A::Error::unknown_field(&key, fields)),
            }
        }
        match (swatch, gradient) {
            (Some(_), Some(_)) => Err(A::Error::custom(
                "a paint is either a swatch or a gradient, not both",
            )),
            (Some(swatch), None) => {
                if stops.is_some() || angle.is_some() {
                    return Err(A::Error::custom("`stops` and `angle` belong to a gradient"));
                }
                Ok(Paint::Swatch {
                    swatch,
                    tint: tint.unwrap_or(100.0),
                })
            }
            (None, Some(gradient)) => {
                if tint.is_some() {
                    return Err(A::Error::custom(
                        "`tint` belongs to a swatch, not a gradient",
                    ));
                }
                Ok(Paint::Gradient {
                    gradient,
                    stops: stops.ok_or_else(|| A::Error::missing_field("stops"))?,
                    angle: angle.unwrap_or(0.0),
                })
            }
            (None, None) => Err(A::Error::custom(if self.gradients {
                "a colour object needs `swatch` (with an optional `tint`) or `gradient` (with `stops`)"
            } else {
                "a colour object needs `swatch` (with an optional `tint`)"
            })),
        }
    }
}

impl<'de> Deserialize<'de> for Paint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(PaintVisitor { gradients: true })
    }
}

impl<'de> Deserialize<'de> for SolidPaint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match d.deserialize_any(PaintVisitor { gradients: false })? {
            Paint::Color(c) => Ok(SolidPaint::Color(c)),
            Paint::Swatch { swatch, tint } => Ok(SolidPaint::Swatch { swatch, tint }),
            Paint::Gradient { .. } => Err(serde::de::Error::custom(
                "a gradient stop cannot itself be a gradient",
            )),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmyk_formula() {
        assert_eq!(Cmyk::new(0.0, 0.0, 0.0, 0.0).to_rgb(), Color::WHITE);
        assert_eq!(Cmyk::new(0.0, 0.0, 0.0, 100.0).to_rgb(), Color::BLACK);
        assert_eq!(
            Cmyk::new(100.0, 0.0, 0.0, 0.0).to_rgb(),
            Color::rgb(0, 255, 255)
        );
        assert_eq!(
            Cmyk::new(0.0, 100.0, 100.0, 50.0).to_rgb(),
            Color::rgb(128, 0, 0)
        );
    }

    #[test]
    fn tint_mixes_towards_white() {
        let c = Color::rgb(0, 100, 200);
        assert_eq!(apply_tint(c, 100.0), c);
        assert_eq!(apply_tint(c, 0.0), Color::WHITE);
        assert_eq!(apply_tint(c, 50.0), Color::rgb(128, 178, 228));
    }

    #[test]
    fn old_hex_strings_are_paints() {
        let p: Paint = serde_json::from_str("\"#FF8000\"").unwrap();
        assert_eq!(p, Paint::Color(Color::rgb(255, 128, 0)));
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"#FF8000\"");
    }

    #[test]
    fn swatch_and_gradient_paints_round_trip() {
        let id = SchemeSlot::Main.swatch_id();
        let p: Paint = serde_json::from_str(&format!("{{\"swatch\":\"{id}\"}}")).unwrap();
        assert_eq!(p, Paint::swatch(id));
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            format!("{{\"swatch\":\"{id}\"}}")
        );
        let g = r##"{"gradient":"linear","stops":[{"offset":0.0,"color":"#000000"},{"offset":1.0,"color":"#FFFFFF"}],"angle":90.0}"##;
        let p: Paint = serde_json::from_str(g).unwrap();
        assert!(p.is_gradient());
        let back: Paint = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn schemes_are_distinct_and_the_default_matches_the_templates() {
        assert_eq!(SCHEMES.len(), 12);
        let mut ids: Vec<_> = SCHEMES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 12);
        let d = scheme(DEFAULT_SCHEME).unwrap();
        assert_eq!(d.color(SchemeSlot::Main), Color::rgb(0x1E, 0x3A, 0x5F));
        assert_eq!(d.color(SchemeSlot::Accent1), Color::rgb(0xC2, 0x41, 0x0C));
        assert_eq!(d.color(SchemeSlot::Accent2), Color::rgb(0xFF, 0xF7, 0xED));
    }
}
