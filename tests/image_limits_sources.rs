//! Image limits per asset source: a source (a mod pack, a download cache, a
//! folder) can carry limits of its own. Pictures read from it are decoded
//! under them, on the game thread and on the workers of asset groups and
//! streamed textures alike; everything else follows the game's. A refused
//! picture's error says which limits refused it and where they came from.
//!
//! The game's limits are the process's, and one test lowers them: every test
//! here takes `own()` first (the tests of one file run on several threads).

use ponko::assets::{AssetGroup, GroupState, ImageLimits, MemorySource};
use ponko::error::ErrorKind;
use ponko::prelude::*;
use ponko::streaming::{AssetState, ResidencyKind};

static LIMITS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the lock on the process's limits, which are the defaults whenever it is free.
fn own() -> std::sync::MutexGuard<'static, ()> {
    LIMITS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A `side` x `side` PNG: 4 x side x side bytes decoded.
fn png(side: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(side, side)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn game() -> Game {
    Game::new("source limits", 1, 1).hot_reload(false)
}

/// Mods may send pictures up to 32 pixels a side.
const TIGHT: ImageLimits = ImageLimits::new(32, 1 << 20);

#[test]
fn a_source_with_tight_limits_refuses_its_large_picture_while_the_games_own_loads() {
    let _own = own();
    game().run_headless(0, |ctx| {
        // The same 64 x 64 picture, in a source with tight limits and in one without.
        ctx.assets
            .add_source_with_image_limits(MemorySource::new("mods").with("mod.png", png(64)), TIGHT);
        ctx.assets
            .add_source(MemorySource::new("built-in").with("own.png", png(64)));

        let err = ctx.gfx.try_load_texture("mod.png").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(
            err.message().contains("32 pixels a side, 1 MiB"),
            "names the limits: {}",
            err.message()
        );
        assert!(
            err.message().contains("asset source \"mods\""),
            "names where they came from: {}",
            err.message()
        );

        let own = ctx.gfx.try_load_texture("own.png").unwrap();
        assert_eq!((own.width(), own.height()), (64, 64));
        // A small picture from the limited source still loads.
        ctx.assets
            .add_source(MemorySource::new("small").with("small.png", png(32)));
        ctx.assets.set_source_image_limits("small", TIGHT);
        assert!(ctx.gfx.try_load_texture("small.png").is_ok());
    });
}

#[test]
fn a_picture_past_the_games_limits_says_they_are_the_games() {
    let _own = own();
    game().run_headless(0, |ctx| {
        ctx.assets.set_image_limits(ImageLimits::new(48, 1 << 20));
        ctx.assets.add_source(MemorySource::new("mem").with("big.png", png(64)));
        let err = ctx.gfx.try_load_texture("big.png").unwrap_err();
        ctx.assets.set_image_limits(ImageLimits::default());
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(err.message().contains("48 pixels a side"), "{}", err.message());
        assert!(err.message().contains("the game's limits"), "{}", err.message());
        assert!(!err.message().contains("the limits of asset source"), "{}", err.message());
    });
}

#[test]
fn limits_can_be_set_before_the_source_is_added_and_taken_back() {
    let _own = own();
    game().run_headless(0, |ctx| {
        // A mod source is mounted later by the engine: its id is known now.
        ctx.assets.set_source_image_limits("late", TIGHT);
        assert_eq!(ctx.assets.source_image_limits("late"), Some(TIGHT));
        assert_eq!(ctx.assets.source_image_limits("other"), None);
        ctx.assets
            .add_source(MemorySource::new("late").with("a.png", png(64)));
        assert_eq!(
            ctx.gfx.try_load_texture("a.png").unwrap_err().kind(),
            ErrorKind::Limit
        );
        ctx.assets.clear_source_image_limits("late");
        assert_eq!(ctx.assets.source_image_limits("late"), None);
        assert!(ctx.gfx.try_load_texture("a.png").is_ok(), "follows the game's again");
    });
}

#[test]
fn the_limits_in_force_list_the_games_and_each_sources_own() {
    let _own = own();
    game().run_headless(0, |ctx| {
        assert!(ctx.assets.image_limit_sources().is_empty());
        ctx.assets.set_source_image_limits("zeta", ImageLimits::new(64, 4 << 20));
        ctx.assets.set_source_image_limits("alpha", TIGHT);
        assert_eq!(
            ctx.assets.image_limit_sources(),
            vec![
                ("alpha".to_string(), TIGHT),
                ("zeta".to_string(), ImageLimits::new(64, 4 << 20))
            ],
            "sorted by id"
        );
    });
}

#[test]
fn pictures_of_an_asset_group_decode_on_workers_under_their_sources_limits() {
    let _own = own();
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source_with_image_limits(MemorySource::new("mods").with("mod.png", png(64)), TIGHT);
        ctx.assets
            .add_background_source(MemorySource::new("built-in").with("own.png", png(64)));

        let own = ctx.assets.start_group(AssetGroup::new("own").texture("own.png"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(own), GroupState::Ready);

        let modded = ctx.assets.start_group(AssetGroup::new("mod").texture("mod.png"));
        ctx.finish_uploads();
        assert_eq!(ctx.assets.group_state(modded), GroupState::Failed);
        let err = ctx.assets.group_error(modded).unwrap();
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(err.message().contains("asset source \"mods\""), "{}", err.message());
    });
}

#[test]
fn streamed_textures_decode_under_their_sources_limits() {
    let _own = own();
    game().run_headless(0, |ctx| {
        ctx.assets
            .add_background_source_with_image_limits(MemorySource::new("mods").with("mod.png", png(64)), TIGHT);
        ctx.assets
            .add_background_source(MemorySource::new("built-in").with("own.png", png(64)));

        ctx.gfx.stream_texture("own.png");
        ctx.gfx.stream_texture("mod.png");
        ctx.finish_uploads();
        assert_eq!(
            ctx.gfx.asset_state(ResidencyKind::Texture, "own.png"),
            AssetState::Resident
        );
        assert_eq!(
            ctx.gfx.asset_state(ResidencyKind::Texture, "mod.png"),
            AssetState::Failed
        );
        let err = ctx.gfx.asset_error(ResidencyKind::Texture, "mod.png").unwrap();
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(err.message().contains("asset source \"mods\""), "{}", err.message());
    });
}

#[cfg(feature = "3d")]
#[test]
fn a_texture_array_reads_each_layer_under_its_sources_limits() {
    let _own = own();
    game().run_headless(0, |ctx| {
        ctx.assets.add_source_with_image_limits(
            MemorySource::new("mods").with("a.png", png(64)).with("b.png", png(64)),
            TIGHT,
        );
        let err = ctx
            .gfx
            .try_load_texture_array(&["a.png", "b.png"], ponko::terrain::TextureKind::Color)
            .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Limit, "{}", err.message());
        assert!(err.message().contains("asset source \"mods\""), "{}", err.message());
    });
}
