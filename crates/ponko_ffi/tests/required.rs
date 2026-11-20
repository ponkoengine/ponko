//! Required components through the C API: `ponko_component_require` makes a
//! component bring others, through spawns and sets, with the given values
//! winning, and refuses what it can't copy from bytes.
use ponko_ffi::*;
use std::ptr::null;

use PonkoStatus::*;

/// A readable address for a zero-sized value.
static NOTHING: u32 = 0;

unsafe fn register(w: *mut PonkoWorld, name: &std::ffi::CStr, size: usize) -> u32 {
    let mut id = 0;
    assert_eq!(
        unsafe { ponko_component_register(w, name.as_ptr(), size, size.max(1), &mut id) },
        PONKO_OK
    );
    id
}
unsafe fn require(w: *mut PonkoWorld, of: u32, needed: u32, value: u32) -> PonkoStatus {
    unsafe { ponko_component_require(w, of, needed, (&raw const value).cast()) }
}
unsafe fn spawn(w: *mut PonkoWorld, values: &[(u32, Option<u32>)]) -> u64 {
    let raw: Vec<PonkoValue> = values
        .iter()
        .map(|(id, value)| PonkoValue {
            component: *id,
            data: value
                .as_ref()
                .map_or((&raw const NOTHING).cast(), |v| (v as *const u32).cast()),
            size: if value.is_some() { 4 } else { 0 },
        })
        .collect();
    let mut e = 0;
    assert_eq!(unsafe { ponko_spawn(w, raw.as_ptr(), raw.len(), &mut e) }, PONKO_OK);
    e
}
unsafe fn get(w: *mut PonkoWorld, e: u64, id: u32) -> Option<u32> {
    let mut out = 0u32;
    match unsafe { ponko_get(w, e, id, (&raw mut out).cast(), 4) } {
        PONKO_OK => Some(out),
        _ => None,
    }
}

#[test]
fn a_spawn_brings_the_required_components_and_what_they_require() {
    unsafe {
        let w = ponko_world_new();
        let tank = register(w, c"Tank", 0);
        let fuel = register(w, c"Fuel", 4);
        let heat = register(w, c"Heat", 4);
        assert_eq!(require(w, tank, fuel, 50), PONKO_OK);
        assert_eq!(require(w, fuel, heat, 7), PONKO_OK); // through: a tank gets heat too

        let t = spawn(w, &[(tank, None)]);
        assert_eq!((get(w, t, fuel), get(w, t, heat)), (Some(50), Some(7)));

        // Values the spawn gives win over the defaults.
        let given = spawn(w, &[(tank, None), (fuel, Some(1))]);
        assert_eq!((get(w, given, fuel), get(w, given, heat)), (Some(1), Some(7)));
        ponko_world_free(w);
    }
}

#[test]
fn a_set_that_gives_the_component_brings_the_rest_and_again_replaces_the_value() {
    unsafe {
        let w = ponko_world_new();
        let tank = register(w, c"Tank", 0);
        let fuel = register(w, c"Fuel", 4);
        let armor = register(w, c"Armor", 4);
        assert_eq!(require(w, tank, fuel, 50), PONKO_OK);

        let e = spawn(w, &[(armor, Some(3))]);
        assert_eq!(get(w, e, fuel), None);
        assert_eq!(ponko_set(w, e, tank, (&raw const NOTHING).cast(), 0), PONKO_OK);
        assert_eq!((get(w, e, fuel), get(w, e, armor)), (Some(50), Some(3)));

        // Entities that had the component before are left as they were; the
        // next spawn takes the new value.
        assert_eq!(require(w, tank, fuel, 60), PONKO_OK);
        assert_eq!(get(w, e, fuel), Some(50));
        let later = spawn(w, &[(tank, None)]);
        assert_eq!(get(w, later, fuel), Some(60));
        ponko_world_free(w);
    }
}

#[test]
fn what_cannot_be_copied_from_bytes_or_is_unknown_is_refused() {
    unsafe {
        let w = ponko_world_new();
        let tank = register(w, c"Tank", 0);
        let fuel = register(w, c"Fuel", 4);
        let mut sprite = 0;
        assert_eq!(ponko_sprite_component(w, &mut sprite), PONKO_OK);

        // A converted built-in has no bytes to copy.
        assert_eq!(require(w, tank, sprite, 0), PONKO_INVALID_ARGUMENT);
        // Unknown ids on either side, and a null default for a sized component.
        assert_eq!(require(w, 9999, fuel, 1), PONKO_INVALID_ARGUMENT);
        assert_eq!(require(w, tank, 9999, 1), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_component_require(w, tank, fuel, null()), PONKO_INVALID_ARGUMENT);
        // A null world is an error, not a crash.
        assert_ne!(
            ponko_component_require(std::ptr::null_mut(), tank, fuel, null()),
            PONKO_OK
        );
        ponko_world_free(w);
    }
}

#[test]
fn a_built_in_that_is_plain_bytes_can_be_required() {
    unsafe {
        let w = ponko_world_new();
        let tank = register(w, c"Tank", 0);
        let mut transform = 0;
        assert_eq!(ponko_transform_component(w, &mut transform), PONKO_OK);
        let size = std::mem::size_of::<PonkoTransform2D>();
        let mut value = [0u8; 64];
        assert!(size <= value.len());
        assert_eq!(
            ponko_component_require(w, tank, transform, value.as_mut_ptr().cast()),
            PONKO_OK
        );
        let t = spawn(w, &[(tank, None)]);
        let mut out = [0u8; 64];
        assert_eq!(ponko_get(w, t, transform, out.as_mut_ptr().cast(), size), PONKO_OK);
        ponko_world_free(w);
    }
}
