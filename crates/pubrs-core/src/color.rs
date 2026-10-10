//! sRGB colours, serialized as `#RRGGBB` or `#RRGGBBAA` hex strings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An sRGB colour with alpha, 8 bits per channel.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Channels as 0..=1 floats.
    pub fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| c as f32 / 255.0)
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::BLACK
    }
}

impl fmt::Debug for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Color({})", self.to_hex())
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Error parsing a hex colour.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid colour {0:?}: expected #RRGGBB or #RRGGBBAA")]
pub struct ParseColorError(pub String);

impl FromStr for Color {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseColorError(s.to_string());
        let hex = s.trim().strip_prefix('#').ok_or_else(err)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
            return Err(err());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        Ok(Color {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a: if hex.len() == 8 { byte(6)? } else { 255 },
        })
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Color {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^#[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?$",
            "description": "sRGB colour as #RRGGBB or #RRGGBBAA (alpha last), e.g. \"#1E3A8A\"."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c: Color = "#1e3a8a".parse().unwrap();
        assert_eq!(c, Color::rgb(0x1E, 0x3A, 0x8A));
        assert_eq!(c.to_hex(), "#1E3A8A");
        let t: Color = "#00000080".parse().unwrap();
        assert_eq!(t.a, 0x80);
        assert_eq!(t.to_hex(), "#00000080");
    }

    #[test]
    fn rejects_bad_hex() {
        for s in ["red", "#12345", "#GGGGGG", "123456", "#1234567"] {
            assert!(s.parse::<Color>().is_err(), "{s}");
        }
    }

    #[test]
    fn serde_uses_hex() {
        let json = serde_json::to_string(&Color::rgb(255, 0, 16)).unwrap();
        assert_eq!(json, "\"#FF0010\"");
        let back: Color = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Color::rgb(255, 0, 16));
    }
}
