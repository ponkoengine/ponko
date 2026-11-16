//! Spine texture atlases (`.atlas`, the libgdx text format Spine's packer
//! writes): pages (one image each) and named regions on them, packed
//! rotated or trimmed.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec2;

use super::data::{SkeletonError, err};
use super::json::beside;

/// A Spine texture atlas: the page images and the named regions on them.
/// [`SkeletonData::from_spine_json_with_atlas`](super::SkeletonData::from_spine_json_with_atlas)
/// takes the regions for its attachments from it;
/// [`Ctx::load_skeleton2d`](crate::ctx::Ctx::load_skeleton2d) reads the
/// atlas next to a Spine export by itself.
///
/// Both spellings Spine has written are read: 4.x (`bounds`, `offsets`,
/// `rotate: 90`) and 3.x (`xy`, `size`, `orig`, `offset`, `rotate: true`).
/// Regions packed a quarter turn (`rotate`) and trimmed regions (whitespace
/// cut off, `offsets`) draw as they were authored.
///
/// ```
/// use ponko::skeleton2d::SpineAtlas;
/// let atlas = SpineAtlas::parse(
///     "hero.png\nsize: 64, 32\nfilter: Linear, Linear\nhead\n  bounds: 0, 0, 20, 30\n  rotate: 90\n",
///     "hero/hero.atlas",
/// )
/// .unwrap();
/// assert_eq!(atlas.images(), ["hero/hero.png"]);
/// assert!(atlas.has_region("head"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SpineAtlas {
    path: Arc<str>,
    pub(crate) pages: Vec<AtlasPage>,
    regions: HashMap<String, AtlasRegion>,
}

/// One page: an image and how to sample it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AtlasPage {
    /// The image's asset name.
    pub(crate) image: Arc<str>,
    /// Its size in pixels, as the atlas says (`size`), if it says.
    pub(crate) size: Option<Vec2>,
    /// `filter: Nearest`: pixel art, no smoothing.
    pub(crate) nearest: bool,
    /// `pma: true`: colors premultiplied by alpha.
    pub(crate) premultiplied: bool,
}

/// One region, in its page's pixels (y down from the top left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AtlasRegion {
    pub(crate) page: usize,
    /// The packed top-left corner.
    pub(crate) x: f32,
    pub(crate) y: f32,
    /// The kept (trimmed) image's size, unrotated.
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// Packed a quarter turn counter-clockwise (the packed rectangle is
    /// `height` wide and `width` tall).
    pub(crate) rotated: bool,
    /// Where the kept part sits in the whole image: left and *bottom*
    /// margins (Spine's y-up), and the whole image's size.
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
    pub(crate) original_width: f32,
    pub(crate) original_height: f32,
}

impl AtlasRegion {
    /// The packed rectangle in the page: (x, y, width, height).
    pub(crate) fn packed(&self) -> crate::camera::Rect {
        let (w, h) = if self.rotated {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        crate::camera::Rect::new(self.x, self.y, w, h)
    }

    /// A mesh's UV in the whole image (0..1, y down) as a UV in a page of
    /// `page` pixels, as Spine's runtimes map them (trim and rotation).
    pub(crate) fn mesh_uv(&self, uv: Vec2, page: Vec2) -> Vec2 {
        let gap_top = self.original_height - self.offset_y - self.height;
        if self.rotated {
            let gap_right = self.original_width - self.offset_x - self.width;
            let u = (self.x - gap_top) / page.x;
            let v = (self.y - gap_right) / page.y;
            Vec2::new(
                u + uv.y * self.original_height / page.x,
                v + (1.0 - uv.x) * self.original_width / page.y,
            )
        } else {
            let u = (self.x - self.offset_x) / page.x;
            let v = (self.y - gap_top) / page.y;
            Vec2::new(
                u + uv.x * self.original_width / page.x,
                v + uv.y * self.original_height / page.y,
            )
        }
    }
}

/// `key: a, b, c` as the key and its comma-separated values.
fn entry(line: &str) -> Option<(&str, Vec<&str>)> {
    let (key, values) = line.trim().split_once(':')?;
    Some((key.trim(), values.split(',').map(str::trim).collect()))
}

impl SpineAtlas {
    /// Reads an atlas. `path` is its asset name: page images are found
    /// next to it.
    ///
    /// # Errors
    /// A region before any page, a number that isn't one, or a rotation
    /// other than 0 or 90 degrees, naming the file and line.
    pub fn parse(text: &str, path: &str) -> Result<Self, SkeletonError> {
        let mut atlas = SpineAtlas {
            path: path.into(),
            pages: Vec::new(),
            regions: HashMap::new(),
        };
        let mut lines = text.lines().enumerate().peekable();
        let bad = |line: usize, what: &str| err(format!("{path}, line {}: {what}", line + 1));
        let numbers = |line: usize, key: &str, values: &[&str], n: usize| -> Result<Vec<f32>, SkeletonError> {
            let parsed: Option<Vec<f32>> = values.iter().map(|v| v.parse::<f32>().ok()).collect();
            match parsed {
                Some(v) if v.len() >= n && v.iter().all(|x| x.is_finite()) => Ok(v),
                _ => Err(bad(line, &format!("\"{key}\" needs {n} numbers"))),
            }
        };
        // Header entries before the first page (4.x may write some).
        while let Some((_, line)) = lines.peek() {
            if line.trim().is_empty() || entry(line).is_some() {
                lines.next();
            } else {
                break;
            }
        }
        let mut page: Option<usize> = None;
        while let Some((n, line)) = lines.next() {
            if line.trim().is_empty() {
                page = None;
                continue;
            }
            let name = line.trim();
            // The entries under this line: the page's or the region's.
            let mut fields = Vec::new();
            while let Some((m, next)) = lines.peek() {
                match entry(next) {
                    Some((key, values)) if !next.trim().is_empty() => {
                        fields.push((
                            *m,
                            key.to_string(),
                            values.into_iter().map(String::from).collect::<Vec<_>>(),
                        ));
                        lines.next();
                    }
                    _ => break,
                }
            }
            let Some(on) = page else {
                let mut p = AtlasPage {
                    image: beside(path, name).into(),
                    size: None,
                    nearest: false,
                    premultiplied: false,
                };
                for (m, key, values) in &fields {
                    let values: Vec<&str> = values.iter().map(String::as_str).collect();
                    match key.as_str() {
                        "size" => {
                            let v = numbers(*m, key, &values, 2)?;
                            p.size = Some(Vec2::new(v[0], v[1]));
                        }
                        "filter" => p.nearest = values.first().is_some_and(|f| f.eq_ignore_ascii_case("nearest")),
                        "pma" => p.premultiplied = values.first() == Some(&"true"),
                        _ => {}
                    }
                }
                atlas.pages.push(p);
                page = Some(atlas.pages.len() - 1);
                continue;
            };
            let mut r = AtlasRegion {
                page: on,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                rotated: false,
                offset_x: 0.0,
                offset_y: 0.0,
                original_width: 0.0,
                original_height: 0.0,
            };
            for (m, key, values) in &fields {
                let values: Vec<&str> = values.iter().map(String::as_str).collect();
                match key.as_str() {
                    "xy" => {
                        let v = numbers(*m, key, &values, 2)?;
                        (r.x, r.y) = (v[0], v[1]);
                    }
                    "size" => {
                        let v = numbers(*m, key, &values, 2)?;
                        (r.width, r.height) = (v[0], v[1]);
                    }
                    "bounds" => {
                        let v = numbers(*m, key, &values, 4)?;
                        (r.x, r.y, r.width, r.height) = (v[0], v[1], v[2], v[3]);
                    }
                    "offset" => {
                        let v = numbers(*m, key, &values, 2)?;
                        (r.offset_x, r.offset_y) = (v[0], v[1]);
                    }
                    "orig" => {
                        let v = numbers(*m, key, &values, 2)?;
                        (r.original_width, r.original_height) = (v[0], v[1]);
                    }
                    "offsets" => {
                        let v = numbers(*m, key, &values, 4)?;
                        (r.offset_x, r.offset_y, r.original_width, r.original_height) = (v[0], v[1], v[2], v[3]);
                    }
                    "rotate" => {
                        r.rotated = match values.first().copied() {
                            Some("true" | "90") => true,
                            Some("false" | "0") => false,
                            Some(other) => {
                                return Err(bad(
                                    *m,
                                    &format!(
                                        "region \"{name}\" is packed turned {other} degrees; only 0 and 90 are read"
                                    ),
                                ));
                            }
                            None => false,
                        };
                    }
                    _ => {}
                }
            }
            if r.original_width == 0.0 && r.original_height == 0.0 {
                (r.original_width, r.original_height) = (r.width, r.height);
            }
            if r.width <= 0.0 || r.height <= 0.0 {
                return Err(bad(n, &format!("region \"{name}\" has no size")));
            }
            atlas.regions.insert(name.to_string(), r);
        }
        Ok(atlas)
    }

    /// The asset name it was read from.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Its page images' asset names, in page order.
    #[must_use]
    pub fn images(&self) -> Vec<&str> {
        self.pages.iter().map(|p| &*p.image).collect()
    }

    /// Whether it has a region named `name`.
    #[must_use]
    pub fn has_region(&self, name: &str) -> bool {
        self.regions.contains_key(name)
    }

    pub(crate) fn region(&self, name: &str) -> Option<&AtlasRegion> {
        self.regions.get(name)
    }
}
