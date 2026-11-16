//! Textures saved as assets (`Texture2D`, class 28, and `Texture2DArray`,
//! class 187): generated or baked images, rather than image files with
//! import settings.

use crate::{Error, UnityObject, Value};

/// A texture asset's top mip level as RGBA8, rows from the top (as image
/// files have them; Unity keeps them from the bottom).
///
/// ```
/// let text = "%YAML 1.1\n--- !u!28 &2800000\nTexture2D:\n  m_Name: Dot\n  m_Width: 1\n  m_Height: 2\n  m_TextureFormat: 3\n  image data: 6\n  _typelessdata: ff000000ff00\n";
/// let file = ponko_unity::UnityFile::parse(text).unwrap();
/// let t = ponko_unity::UnityTexture::from_object(&file.objects[0]).unwrap();
/// assert_eq!((t.width, t.height, t.layers), (1, 2, 1));
/// assert_eq!(t.rgba, [0, 255, 0, 255, 255, 0, 0, 255], "green on top: the bottom row comes first in Unity");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct UnityTexture {
    /// `m_Name`.
    pub name: String,
    /// Width in texels (`m_Width`).
    pub width: u32,
    /// Height in texels (`m_Height`), per layer.
    pub height: u32,
    /// Layers of an array texture, one after the other; 1 for a texture.
    pub layers: u32,
    /// `width * height * layers` RGBA8 texels.
    pub rgba: Vec<u8>,
}

impl UnityTexture {
    /// Reads a `Texture2D` or `Texture2DArray` saved uncompressed: Alpha8,
    /// R8, RG16, RGB24, RGBA32, ARGB32 or BGRA32 (`m_TextureFormat`), or
    /// the 8-bit R, RG, RGB and RGBA graphics formats of arrays
    /// (`m_Format`).
    ///
    /// # Errors
    ///
    /// If it isn't a texture, its format is compressed or not one of
    /// those, or its data is shorter than its size says.
    pub fn from_object(object: &UnityObject) -> Result<Self, Error> {
        let b = &object.body;
        let (layers, layout) = match object.class_id {
            28 => (1, b["m_TextureFormat"].as_i64().and_then(texture_format)),
            187 => (
                u32::try_from(b["m_Depth"].as_i64().unwrap_or(1).max(1)).unwrap_or(u32::MAX),
                b["m_Format"].as_i64().and_then(graphics_format),
            ),
            _ => {
                return Err(Error::invalid(format!(
                    "object {} is a {}, not a texture",
                    object.file_id, object.type_name
                )));
            }
        };
        let name = b["m_Name"].as_str().unwrap_or("").to_string();
        let layout =
            layout.ok_or_else(|| Error::unsupported(format!("{name}: a compressed or unknown texture format")))?;
        let size = |key: &str| u32::try_from(b[key].as_i64().unwrap_or(0)).unwrap_or(0);
        let (width, height) = (size("m_Width"), size("m_Height"));
        let data: Vec<u8> = match &b["image data"] {
            Value::Bytes(bytes) => bytes.clone(),
            Value::Numbers(n) => n.iter().map(|&v| v as u8).collect(),
            _ => crate::mesh::hex(b["_typelessdata"].as_str().unwrap_or_default()),
        };
        // Each layer holds its mips; only the top one is wanted.
        // Checked against the data below; 0 if it can't even be counted.
        let level = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(layout.size))
            .unwrap_or(0);
        let per_layer = if layers > 1 {
            data.len() / layers as usize
        } else {
            data.len()
        };
        if level == 0 || per_layer < level {
            return Err(Error::invalid(format!(
                "{name}: {} bytes of data for {width}x{height}x{layers}",
                data.len()
            )));
        }
        let mut rgba = Vec::with_capacity(level / layout.size * 4 * layers as usize);
        for layer in 0..layers as usize {
            let top = &data[layer * per_layer..layer * per_layer + level];
            let row = width as usize * layout.size;
            for y in (0..height as usize).rev() {
                for px in top[y * row..(y + 1) * row].chunks_exact(layout.size) {
                    rgba.extend((layout.to_rgba)(px));
                }
            }
        }
        Ok(Self {
            name,
            width,
            height,
            layers,
            rgba,
        })
    }
}

struct Layout {
    size: usize,
    to_rgba: fn(&[u8]) -> [u8; 4],
}

/// Unity's `TextureFormat`s that are plain bytes.
fn texture_format(f: i64) -> Option<Layout> {
    let l = |size, to_rgba| Some(Layout { size, to_rgba });
    match f {
        1 => l(1, |p| [255, 255, 255, p[0]]),     // Alpha8
        63 => l(1, |p| [p[0], p[0], p[0], 255]),  // R8
        62 => l(2, |p| [p[0], p[1], 0, 255]),     // RG16
        3 => l(3, |p| [p[0], p[1], p[2], 255]),   // RGB24
        4 => l(4, |p| [p[0], p[1], p[2], p[3]]),  // RGBA32
        5 => l(4, |p| [p[1], p[2], p[3], p[0]]),  // ARGB32
        14 => l(4, |p| [p[2], p[1], p[0], p[3]]), // BGRA32
        _ => None,
    }
}

/// Unity's `GraphicsFormat`s that are plain bytes (sRGB or not).
fn graphics_format(f: i64) -> Option<Layout> {
    match f {
        1 | 5 => texture_format(63),
        2 | 6 => texture_format(62),
        3 | 7 => texture_format(3),
        4 | 8 => texture_format(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_keep_each_layers_top_mip() {
        // Two 1x1 RGBA layers, each with a (made up) second mip byte run.
        let text = "--- !u!187 &1\nTexture2DArray:\n  m_Name: Layers\n  m_Width: 1\n  m_Height: 1\n  m_Depth: 2\n  m_Format: 8\n  image data: 16\n  _typelessdata: 0102030405060708090a0b0c0d0e0f10\n";
        let file = crate::UnityFile::parse(text).unwrap();
        let t = UnityTexture::from_object(&file.objects[0]).unwrap();
        assert_eq!(t.layers, 2);
        assert_eq!(t.rgba, [1, 2, 3, 4, 9, 10, 11, 12]);
    }

    #[test]
    fn compressed_and_short_textures_are_errors() {
        let dxt = "--- !u!28 &1\nTexture2D:\n  m_Name: Dxt\n  m_Width: 4\n  m_Height: 4\n  m_TextureFormat: 10\n  _typelessdata: 00\n";
        let file = crate::UnityFile::parse(dxt).unwrap();
        assert!(
            UnityTexture::from_object(&file.objects[0])
                .unwrap_err()
                .message()
                .contains("compressed")
        );
        let short = dxt.replace("m_TextureFormat: 10", "m_TextureFormat: 4");
        let file = crate::UnityFile::parse(&short).unwrap();
        assert!(
            UnityTexture::from_object(&file.objects[0])
                .unwrap_err()
                .message()
                .contains("bytes of data")
        );
    }
}
