//! [`Color`].
//!
//! **Maturity: proven**, from the guide chapter `drawing.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows. Tests: the 2D render tests
//! (`draw_order_render_test.rs`, `views2d_render_test.rs`, `atlas_render_test.rs`, ...). Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics: the
//! packaged games run in a window on Linux (software Vulkan under Xvfb), macOS and Windows (DirectX
//! 12, WARP when there is no GPU); Coin Dash also runs in headless Chromium and WebKit on WebGL2;
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found.

/// An RGBA color. Components are 0.0 to 1.0, in sRGB (the values you'd pick
/// in a paint program or see in CSS).
///
/// ```
/// # use ponko::prelude::*;
/// let sky = Color::rgb(0.4, 0.7, 1.0);
/// let ghost = Color::WHITE.with_alpha(0.5);
/// let orange = Color::hex(0xFF8800);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)] // shared with C/C++ (`PonkoColor`)
pub struct Color {
    /// Red, 0 to 1.
    pub r: f32,
    /// Green, 0 to 1.
    pub g: f32,
    /// Blue, 0 to 1.
    pub b: f32,
    /// Opacity: 0 transparent, 1 opaque.
    pub a: f32,
}

impl Color {
    /// White.
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    /// Black.
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    /// Middle gray.
    pub const GRAY: Color = Color::rgb(0.5, 0.5, 0.5);
    /// Red.
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    /// Green.
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    /// Blue.
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    /// Yellow.
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    /// Fully transparent (draws nothing).
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    /// A soft dark blue-gray, the default clear color.
    pub const PONKO_DARK: Color = Color::rgb(0.11, 0.12, 0.15);

    /// An opaque color from red, green and blue (each 0.0 to 1.0).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let teal = Color::rgb(0.0, 0.5, 0.5);
    /// ```
    #[must_use]
    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    /// A color with transparency (`a`: 0.0 invisible, 1.0 opaque).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let shadow = Color::rgba(0.0, 0.0, 0.0, 0.3);
    /// ```
    #[must_use]
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// An opaque color from a `0xRRGGBB` hex number.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let crab_orange = Color::hex(0xE8743B);
    /// ```
    #[must_use]
    pub const fn hex(rgb: u32) -> Color {
        Color::rgb(
            ((rgb >> 16) & 0xFF) as f32 / 255.0,
            ((rgb >> 8) & 0xFF) as f32 / 255.0,
            (rgb & 0xFF) as f32 / 255.0,
        )
    }

    /// The same color with a different alpha.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let faded = Color::RED.with_alpha(0.25);
    /// ```
    #[must_use]
    pub const fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// Packs into 4 bytes (sRGB), the format sprites are sent to the GPU in.
    pub(crate) fn to_rgba8(self) -> [u8; 4] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }

    /// Linear-light values, which is what an sRGB render target expects.
    #[cfg(feature = "window")]
    pub(crate) fn to_linear_wgpu(self) -> wgpu::Color {
        fn lin(c: f32) -> f64 {
            let c = f64::from(c.clamp(0.0, 1.0));
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        wgpu::Color {
            r: lin(self.r),
            g: lin(self.g),
            b: lin(self.b),
            a: f64::from(self.a.clamp(0.0, 1.0)),
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_channels() {
        assert_eq!(Color::hex(0xFF8000).to_rgba8(), [255, 128, 0, 255]);
    }

    #[test]
    fn rgba8_clamps() {
        assert_eq!(Color::rgba(2.0, -1.0, 0.5, 1.0).to_rgba8(), [255, 0, 128, 255]);
    }

    #[test]
    #[cfg(feature = "window")]
    fn linear_conversion_endpoints_and_midpoint() {
        let c = Color::rgb(0.0, 1.0, 0.5).to_linear_wgpu();
        assert_eq!(c.r, 0.0);
        assert!((c.g - 1.0).abs() < 1e-9);
        assert!((c.b - 0.214).abs() < 0.001);
    }
}
