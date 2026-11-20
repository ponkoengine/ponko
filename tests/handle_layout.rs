//! Typed handles cost nothing: every handle kind is its own type, and each
//! is exactly its id (plus the size a texture, a sound or a texture array
//! carries), as before the kinds got types of their own. A
//! `RenderTarget` is the `Texture` it wraps, so whatever holds one
//! (`TargetCamera`, `ComputePass`, `TargetEffect`) keeps the size and
//! alignment it had when it held the texture.
//!
//! These are layout checks, not benchmarks: `ponko_bench`'s draw
//! benchmarks measure the time.

use std::mem::{align_of, size_of};

use ponko::audio::Sound;
use ponko::handle::Handle;
use ponko::prelude::*;
use ponko::render::{SpriteMaterial, SpriteShader};
use ponko::text::Font;

/// Copy, comparable, hashable and shareable across threads, as a handle
/// must stay.
fn is_a_plain_value<H: Handle + Send + Sync + Unpin + 'static>() {}

#[test]
fn every_2d_handle_is_its_id_and_nothing_more() {
    assert_eq!((size_of::<Font>(), align_of::<Font>()), (4, 4));
    assert_eq!((size_of::<SpriteShader>(), align_of::<SpriteShader>()), (4, 4));
    assert_eq!((size_of::<SpriteMaterial>(), align_of::<SpriteMaterial>()), (4, 4));
    // A texture carries its size; a sound its length.
    assert_eq!((size_of::<Texture>(), align_of::<Texture>()), (12, 4));
    assert_eq!((size_of::<Sound>(), align_of::<Sound>()), (8, 4));
    is_a_plain_value::<Texture>();
    is_a_plain_value::<Sound>();
    is_a_plain_value::<Font>();
    is_a_plain_value::<SpriteShader>();
    is_a_plain_value::<SpriteMaterial>();
}

#[cfg(feature = "3d")]
mod three {
    use super::*;
    use ponko::d3::Camera as Camera3D;
    use ponko::render::{
        Compute, ComputePass, Effect, GpuBuffer, InstanceBatch, InstanceCell, Material, Mesh, RenderTarget, Shader,
        TargetCamera, TargetEffect,
    };
    use ponko::terrain::{TerrainTile, TextureArray};

    #[test]
    fn every_3d_handle_is_its_id_and_nothing_more() {
        for (size, align) in [
            (size_of::<Mesh>(), align_of::<Mesh>()),
            (size_of::<Material>(), align_of::<Material>()),
            (size_of::<Shader>(), align_of::<Shader>()),
            (size_of::<Compute>(), align_of::<Compute>()),
            (size_of::<Effect>(), align_of::<Effect>()),
            (size_of::<GpuBuffer>(), align_of::<GpuBuffer>()),
            (size_of::<InstanceBatch>(), align_of::<InstanceBatch>()),
            (size_of::<InstanceCell>(), align_of::<InstanceCell>()),
        ] {
            assert_eq!((size, align), (4, 4));
        }
        // An array carries its size and layer count; a terrain tile is its
        // detail levels' meshes and the distance between them.
        assert_eq!((size_of::<TextureArray>(), align_of::<TextureArray>()), (16, 4));
        assert_eq!(size_of::<TerrainTile>(), 4 * TerrainTile::LODS + 4);
        is_a_plain_value::<Mesh>();
        is_a_plain_value::<Material>();
        is_a_plain_value::<RenderTarget>();
        is_a_plain_value::<TerrainTile>();
    }

    #[test]
    fn a_render_target_has_the_layout_of_the_texture_it_is() {
        assert_eq!(size_of::<RenderTarget>(), size_of::<Texture>());
        assert_eq!(align_of::<RenderTarget>(), align_of::<Texture>());
        assert_eq!(size_of::<Option<RenderTarget>>(), size_of::<Option<Texture>>());
        // And what holds one is as big as when it held the texture.
        #[allow(dead_code)]
        struct CameraIntoATexture(Camera3D, Texture, Option<Vec4>);
        #[allow(dead_code)]
        struct PassIntoATexture(Compute, Texture, Option<GpuBuffer>, Option<Material>);
        #[allow(dead_code)]
        struct EffectIntoATexture(Effect, Texture, Option<Material>);
        assert_eq!(size_of::<TargetCamera>(), size_of::<CameraIntoATexture>());
        assert_eq!(size_of::<ComputePass>(), size_of::<PassIntoATexture>());
        assert_eq!(size_of::<TargetEffect>(), size_of::<EffectIntoATexture>());
    }

    #[test]
    fn reading_a_render_target_as_a_texture_is_the_same_handle() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let target = ctx.gfx.create_render_target(64, 32);
            let texture: Texture = target.texture();
            assert_eq!((texture.width(), texture.height()), (target.width(), target.height()));
            assert_eq!(texture.size(), target.size());
            assert!(ctx.gfx.is_render_target(texture));
            // Const: reading it is free.
            const NONE: Texture = <RenderTarget as Handle>::NONE.texture();
            assert_eq!(NONE, Texture::NONE);
        });
    }
}
