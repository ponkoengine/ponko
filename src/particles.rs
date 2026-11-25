//! Particle effects: dust, sparks, smoke and pickup flashes from one
//! component.
//!
//! **Maturity: proven**, from the guide chapter `drawing.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows. Tests: the 2D render tests
//! (`draw_order_render_test.rs`, `views2d_render_test.rs`, `atlas_render_test.rs`, ...). Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics: the
//! packaged games run in a window on Linux (software Vulkan under Xvfb), macOS and Windows (DirectX
//! 12, WARP when there is no GPU); Coin Dash also runs in headless Chromium and WebKit on WebGL2;
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found.
//!
//! A [`d2::ParticleEmitter`](crate::d2::ParticleEmitter) next
//! to a [`d2::Transform`](crate::d2::Transform) spawns small sprites at a rate or in bursts, moves
//! them, and draws them in the sprite batch at the emitter's `z`, with the
//! sprite [`BlendMode`](crate::render::BlendMode)s and
//! [`SpriteMaterial`](crate::render::SpriteMaterial)s. The 3D emitter
//! (`d3::ParticleEmitter`, feature `3d`) shares its vocabulary and the
//! simulation code: [`ParticleCurve`], [`ParticleGradient`] and
//! [`ParticleSpace`], the types of this module, are the same in both.
//!
//! ```
//! use ponko::prelude::*;
//! use ponko::d2::{ParticleEmitter, ParticleSettings, ParticleShape};
//! use ponko::render::BlendMode;
//! use ponko::particles::ParticleGradient;
//!
//! let sparks = ParticleSettings::default()
//!     .with_rate(80.0)
//!     .with_shape(ParticleShape::Cone { angle: 0.4 })
//!     .with_speed(120.0, 240.0)
//!     .with_color_over_life(ParticleGradient::new(vec![
//!         (0.0, Color::rgb(1.0, 0.9, 0.4)),
//!         (1.0, Color::rgba(1.0, 0.3, 0.0, 0.0)),
//!     ]))
//!     .with_blend(BlendMode::Additive)
//!     .with_z(2.0);
//! let mut world = World::new();
//! world.spawn((ponko::d2::Transform::at(vec2(400.0, 300.0)), ParticleEmitter::new(sparks)));
//! ```

#![deny(missing_docs)]

#[doc(inline)]
pub use crate::particles2d::{ParticleCurve, ParticleGradient, ParticleSpace};
