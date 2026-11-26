//! `ponko_sprite_set_flip` and `ponko_sprite_flip`: a sprite's `flip_x` and
//! `flip_y`, which `PonkoSprite` (a fixed layout) does not carry.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const SIZE: usize = size_of::<PonkoSprite>();

unsafe fn sprite_id(w: *mut PonkoWorld) -> u32 {
    let mut id = 0;
    assert_eq!(unsafe { ponko_sprite_component(w, &mut id) }, PONKO_OK);
    id
}

unsafe fn spawn_sprite(w: *mut PonkoWorld, sprite: &PonkoSprite) -> u64 {
    let value = PonkoValue {
        component: unsafe { sprite_id(w) },
        data: (sprite as *const PonkoSprite).cast(),
        size: SIZE,
    };
    let mut e = 0;
    assert_eq!(unsafe { ponko_spawn(w, &value, 1, &mut e) }, PONKO_OK);
    e
}

unsafe fn flips(w: *mut PonkoWorld, e: u64) -> (bool, bool) {
    let (mut x, mut y) = (true, true);
    assert_eq!(unsafe { ponko_sprite_flip(w, e, &mut x, &mut y) }, PONKO_OK);
    (x, y)
}

unsafe fn write_back(w: *mut PonkoWorld, e: u64, sprite: &PonkoSprite) -> PonkoStatus {
    unsafe { ponko_set(w, e, sprite_id(w), (sprite as *const PonkoSprite).cast(), SIZE) }
}

#[test]
fn flips_start_off_and_are_read_and_set() {
    unsafe {
        let w = ponko_world_new();
        let e = spawn_sprite(w, &PonkoSprite::default());
        assert_eq!(flips(w, e), (false, false));
        assert_eq!(ponko_sprite_set_flip(w, e, true, false), PONKO_OK);
        assert_eq!(flips(w, e), (true, false));
        assert_eq!(ponko_sprite_set_flip(w, e, false, true), PONKO_OK);
        assert_eq!(flips(w, e), (false, true));
        assert_eq!(ponko_sprite_set_flip(w, e, true, true), PONKO_OK);
        assert_eq!(flips(w, e), (true, true));
        assert_eq!(ponko_sprite_set_flip(w, e, false, false), PONKO_OK);
        assert_eq!(flips(w, e), (false, false));
        // An output you don't need can be null.
        assert_eq!(ponko_sprite_set_flip(w, e, true, false), PONKO_OK);
        let mut y = true;
        assert_eq!(ponko_sprite_flip(w, e, null_mut(), &mut y), PONKO_OK);
        assert!(!y);
        let mut x = false;
        assert_eq!(ponko_sprite_flip(w, e, &mut x, null_mut()), PONKO_OK);
        assert!(x);
        assert_eq!(ponko_sprite_flip(w, e, null_mut(), null_mut()), PONKO_OK);
        assert_eq!(ponko_world_free(w), PONKO_OK);
    }
}

#[test]
fn writing_a_ponko_sprite_back_keeps_the_flips() {
    unsafe {
        let w = ponko_world_new();
        let mut sprite = PonkoSprite::default();
        let e = spawn_sprite(w, &sprite);
        assert_eq!(ponko_sprite_set_flip(w, e, true, true), PONKO_OK);
        // Read, change, write back: the other fields change, the flips stay.
        let mut read = PonkoSprite::default();
        let id = sprite_id(w);
        assert_eq!(
            ponko_get(w, e, id, (&mut read as *mut PonkoSprite).cast(), SIZE),
            PONKO_OK
        );
        read.z = 4.0;
        assert_eq!(write_back(w, e, &read), PONKO_OK);
        assert_eq!(flips(w, e), (true, true));
        let mut again = PonkoSprite::default();
        assert_eq!(
            ponko_get(w, e, id, (&mut again as *mut PonkoSprite).cast(), SIZE),
            PONKO_OK
        );
        assert_eq!(again.z, 4.0);
        // Another value over a flipped sprite keeps them too.
        sprite.z = 9.0;
        assert_eq!(write_back(w, e, &sprite), PONKO_OK);
        assert_eq!(flips(w, e), (true, true));
        // A sprite set on an entity that had none starts unflipped.
        let mut bare = 0;
        assert_eq!(ponko_spawn(w, null(), 0, &mut bare), PONKO_OK);
        assert_eq!(write_back(w, bare, &sprite), PONKO_OK);
        assert_eq!(flips(w, bare), (false, false));
        // Removing the sprite and adding one again also starts unflipped.
        assert_eq!(ponko_remove(w, e, id), PONKO_OK);
        assert_eq!(write_back(w, e, &sprite), PONKO_OK);
        assert_eq!(flips(w, e), (false, false));
        assert_eq!(ponko_world_free(w), PONKO_OK);
    }
}

#[test]
fn missing_sprites_dead_entities_and_nulls_are_errors() {
    unsafe {
        let w = ponko_world_new();
        let sprite = PonkoSprite::default();
        let e = spawn_sprite(w, &sprite);
        let mut bare = 0;
        assert_eq!(ponko_spawn(w, null(), 0, &mut bare), PONKO_OK);
        assert_eq!(ponko_sprite_set_flip(w, bare, true, true), PONKO_NOT_FOUND);
        let (mut x, mut y) = (true, true);
        assert_eq!(ponko_sprite_flip(w, bare, &mut x, &mut y), PONKO_NOT_FOUND);
        assert!(x && y, "outputs stay untouched on failure");
        assert_eq!(ponko_despawn(w, e), PONKO_OK);
        assert_eq!(ponko_sprite_set_flip(w, e, true, true), PONKO_NOT_FOUND);
        assert_eq!(ponko_sprite_flip(w, e, &mut x, &mut y), PONKO_NOT_FOUND);
        assert_eq!(ponko_sprite_set_flip(null_mut(), 0, true, true), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_sprite_flip(null_mut(), 0, &mut x, &mut y), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_world_free(w), PONKO_OK);
    }
}

struct Deferred {
    entity: u64,
    sprite: PonkoSprite,
}

unsafe extern "C" fn set_later(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let d = &mut *user.cast::<Deferred>();
        let w = ponko_ctx_world(ctx);
        d.entity = spawn_sprite(w, &d.sprite);
        assert_eq!(ponko_sprite_set_flip(w, d.entity, false, true), PONKO_OK);
        let value = PonkoValue {
            component: sprite_id(w),
            data: (&d.sprite as *const PonkoSprite).cast(),
            size: SIZE,
        };
        assert_eq!(ponko_ctx_set_later(ctx, d.entity, &value), PONKO_OK);
    }
}

#[test]
fn a_deferred_sprite_write_keeps_the_flips() {
    unsafe {
        let mut d = Deferred {
            entity: 0,
            sprite: PonkoSprite {
                z: 3.0,
                ..Default::default()
            },
        };
        let g = ponko_game_new(c"flips".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(set_later), (&mut d as *mut Deferred).cast(), &mut world),
            PONKO_OK
        );
        assert_eq!(flips(world, d.entity), (false, true));
        assert_eq!(ponko_world_free(world), PONKO_OK);
    }
}
