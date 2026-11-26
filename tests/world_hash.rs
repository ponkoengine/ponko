//! `world_hash`: one number for a world's reflected state, for determinism
//! checks. Equal states hash alike however they were built; any change to
//! a reflected value changes it.

use ponko::prelude::*;
use ponko::reflect::world_hash;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Component, Serialize, Deserialize)]
struct Pos(f32, f32);
#[derive(Debug, Clone, Copy, PartialEq, Component, Serialize, Deserialize)]
struct Vel(f32, f32);
#[derive(Debug, Clone, PartialEq, Component, Serialize, Deserialize)]
struct Name(String);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Score {
    points: u32,
    best: Option<u32>,
}
/// Not reflected: not part of the hash.
#[derive(Component)]
struct Scratch(#[allow(dead_code)] u32);

fn reflected() -> World {
    let mut w = World::new();
    w.reflect::<Pos>();
    w.reflect::<Vel>();
    w.reflect::<Name>();
    w.reflect_resource::<Score>();
    w
}

/// Three entities, built archetype by archetype.
fn built_directly() -> World {
    let mut w = reflected();
    w.spawn((Pos(0.0, 1.0),));
    w.spawn((Pos(2.0, 3.0), Vel(1.0, 0.0)));
    w.spawn((Pos(4.0, 5.0), Name("scout".into())));
    w.insert_resource(Score {
        points: 7,
        best: Some(9),
    });
    w
}

#[test]
fn equal_worlds_hash_alike_however_they_were_built() {
    let a = built_directly();
    // The same entities and values, reached through other archetypes, with
    // the components, resources and reflections in another order.
    let mut b = World::new();
    b.reflect_resource::<Score>();
    b.insert_resource(Score {
        points: 7,
        best: Some(9),
    });
    b.reflect::<Name>();
    let e0 = b.spawn((Pos(0.0, 1.0), Scratch(1)));
    let e1 = b.spawn((Vel(1.0, 0.0),));
    let e2 = b.spawn((Name("scout".into()),));
    b.insert(e1, Pos(2.0, 3.0));
    b.insert(e2, Pos(4.0, 5.0));
    b.remove::<Scratch>(e0);
    b.reflect::<Vel>();
    b.reflect::<Pos>();
    assert_eq!(world_hash(&a), world_hash(&b));
    assert_eq!(world_hash(&a), world_hash(&a), "reading changes nothing");
}

#[test]
fn any_reflected_change_changes_the_hash() {
    let base = world_hash(&built_directly());
    let changed = |f: &dyn Fn(&mut World)| {
        let mut w = built_directly();
        f(&mut w);
        world_hash(&w)
    };
    let mut seen = vec![base];
    let mut differs = |name: &str, h: u64| {
        assert!(!seen.contains(&h), "{name} didn't change the hash");
        seen.push(h);
    };
    differs(
        "one bit of a float",
        changed(&|w| {
            w.query::<&mut Pos>().for_each(|p| {
                if p.0 == 2.0 {
                    p.1 = f32::from_bits(p.1.to_bits() + 1);
                }
            })
        }),
    );
    differs(
        "-0.0 for 0.0",
        changed(&|w| {
            w.query::<&mut Pos>()
                .for_each(|p| p.0 = if p.0 == 0.0 { -0.0 } else { p.0 })
        }),
    );
    differs(
        "a component added",
        changed(&|w| {
            let e = w.query::<(Entity, &Pos)>().iter().next().unwrap().0;
            w.insert(e, Vel(0.0, 0.0));
        }),
    );
    differs(
        "an entity despawned",
        changed(&|w| {
            let e = w.query::<(Entity, &Name)>().iter().next().unwrap().0;
            w.despawn(e);
        }),
    );
    differs(
        "a string",
        changed(&|w| w.query::<&mut Name>().for_each(|n| n.0.push('!'))),
    );
    differs("a resource field", changed(&|w| w.resource_mut::<Score>().points += 1));
    differs("None for Some", changed(&|w| w.resource_mut::<Score>().best = None));
    differs(
        "two values swapped between entities",
        changed(&|w| {
            w.query::<&mut Pos>().for_each(|p| {
                if p.0 == 0.0 {
                    *p = Pos(2.0, 3.0);
                } else if p.0 == 2.0 {
                    *p = Pos(0.0, 1.0);
                }
            })
        }),
    );
    // What isn't reflected isn't hashed.
    let mut w = built_directly();
    let e = w.query::<(Entity, &Pos)>().iter().next().unwrap().0;
    w.insert(e, Scratch(3));
    assert_eq!(world_hash(&w), base, "an unreflected component");
}

#[test]
fn a_world_without_reflection_has_one_hash() {
    let mut w = World::new();
    let empty = world_hash(&w);
    w.spawn((Scratch(1),));
    assert_eq!(world_hash(&w), empty);
    assert_eq!(world_hash(&reflected()), world_hash(&reflected()));
}

/// The value is the same on every platform: it reads bits and integers,
/// never the platform's math library or memory layout.
#[test]
fn the_hash_is_the_same_on_every_platform() {
    assert_eq!(world_hash(&built_directly()), 0xe75f_0a49_cc1b_e2f6);
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CUnit {
    hp: f32,
    level: u8,
    spare: u8, // not described: not hashed
    mood: i16,
}

fn c_world() -> (World, ponko::ecs_core::ComponentId) {
    let mut w = World::new();
    // SAFETY: CUnit is plain bytes.
    let id = unsafe {
        w.raw()
            .register_component("CUnit", size_of::<CUnit>(), align_of::<CUnit>(), None)
    };
    let fields = [
        ponko::reflect::Field::new("hp", ponko::reflect::FieldType::F32, std::mem::offset_of!(CUnit, hp)),
        ponko::reflect::Field::new(
            "level",
            ponko::reflect::FieldType::U8,
            std::mem::offset_of!(CUnit, level),
        ),
        ponko::reflect::Field::new(
            "mood",
            ponko::reflect::FieldType::I16,
            std::mem::offset_of!(CUnit, mood),
        ),
    ];
    // SAFETY: every field value makes a valid CUnit.
    unsafe { w.reflect_fields(id, &fields) };
    (w, id)
}

fn c_hash(unit: CUnit) -> u64 {
    let (mut w, id) = c_world();
    // SAFETY: a valid CUnit for `id`.
    unsafe { w.raw().spawn_dynamic(&[(id, (&unit as *const CUnit).cast())]) };
    world_hash(&w)
}

#[test]
fn described_c_components_hash_their_fields_only() {
    let unit = CUnit {
        hp: 2.5,
        level: 3,
        spare: 0,
        mood: -4,
    };
    let base = c_hash(unit);
    assert_eq!(c_hash(CUnit { spare: 99, ..unit }), base, "an undescribed byte");
    assert_ne!(c_hash(CUnit { hp: 2.75, ..unit }), base);
    assert_ne!(c_hash(CUnit { level: 4, ..unit }), base);
    assert_ne!(c_hash(CUnit { mood: 4, ..unit }), base);
    assert_ne!(base, world_hash(&c_world().0), "the entity counts");
}

#[test]
fn engine_components_are_hashed_in_a_game() {
    let hash_with = |color: Color| {
        let world = Game::new("t", 1, 1).run_headless(0, |ctx| {
            let t = ctx.gfx.create_texture(1, 1, &[255; 4]);
            let mut sprite = Sprite::new(t);
            sprite.color = color;
            ctx.world.spawn((sprite, ponko::d2::Transform::default()));
        });
        world_hash(&world)
    };
    assert_eq!(hash_with(Color::WHITE), hash_with(Color::WHITE));
    assert_ne!(hash_with(Color::WHITE), hash_with(Color::RED));
}

/// A small deterministic simulation, hashed after each run: the same seed
/// and steps give the same hash, a different seed doesn't.
#[test]
fn a_deterministic_game_hashes_alike_run_after_run() {
    let run = |seed: u64| {
        let world = Game::new("t", 1, 1)
            .seed(seed)
            .sim_system(|ctx: &mut Ctx| {
                let mut rng = ctx.random().stream(1);
                let jitter: Vec<Vec2> = (0..64).map(|_| rng.unit_vec2()).collect();
                for (p, j) in ctx.world.query::<&mut Pos>().iter().zip(jitter) {
                    p.0 += j.x;
                    p.1 += j.y;
                }
            })
            .run_headless(30, |ctx| {
                ctx.world.reflect::<Pos>();
                for i in 0..64 {
                    ctx.world.spawn((Pos(i as f32, 0.0),));
                }
            });
        world_hash(&world)
    };
    assert_eq!(run(1), run(1));
    assert_ne!(run(1), run(2));
}

#[test]
fn split_hashes_name_the_part_that_diverged() {
    use ponko::reflect::WorldHashParts;
    // Two peers that should agree: one moved a unit a little further.
    let a = built_directly();
    let mut b = built_directly();
    for v in b.query::<&mut Vel>().iter() {
        v.0 += 0.5;
    }
    let (ha, hb) = (WorldHashParts::of(&a), WorldHashParts::of(&b));
    assert_ne!(ha.total, hb.total);
    let differing = ha.differing(&hb);
    assert_eq!(differing.len(), 1, "{differing:?}");
    assert!(differing[0].contains("Vel"), "{differing:?}");
    assert!(ha.differing(&WorldHashParts::of(&built_directly())).is_empty());
}
