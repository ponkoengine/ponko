//! Reflects the 3D particle emitter as `"ParticleEmitter3D"`: its settings,
//! in the 2D emitter's JSON words with the 3D shapes, gravity and softness,
//! and the image saved by file name. Living particles aren't saved; a
//! loaded emitter starts empty, with its first burst.

use std::sync::Arc;

use glam::Vec3;
use ponko_ecs::World;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::particles::SpaceJson;
use super::{Reflected, Registry, SchemaSource, TextureNames, check, remove};
use crate::color::Color;
use crate::particles::{ParticleCurve, ParticleGradient, ParticleSpace};
use crate::schema::ValueSchema;
use crate::three::particles::{ParticleBlend3D, ParticleEmitter3D, ParticleSettings3D, ParticleShape3D};

/// The reflected name.
pub(crate) const NAME: &str = "ParticleEmitter3D";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum ShapeJson {
    Point,
    Sphere { radius: f32 },
    Cone { angle: f32 },
    Box { half_extents: Vec3 },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(super) enum BlendJson {
    Alpha,
    Additive,
}

/// A [`ParticleEmitter3D`]'s settings as JSON.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub(super) struct EmitterJson {
    pub rate: f32,
    pub burst: u64,
    pub duration: f32,
    pub looping: bool,
    pub lifetime: (f32, f32),
    pub speed: (f32, f32),
    pub shape: ShapeJson,
    pub gravity: Vec3,
    pub drag: f32,
    pub size: (f32, f32),
    pub rotation: (f32, f32),
    pub size_over_life: Vec<(f32, f32)>,
    pub color_over_life: Vec<(f32, Color)>,
    pub texture: Option<String>,
    pub sheet: (u32, u32),
    pub blend: BlendJson,
    pub max_particles: u64,
    pub space: SpaceJson,
    pub seed: u64,
    pub softness: f32,
}

impl Default for EmitterJson {
    fn default() -> Self {
        to_json(&ParticleSettings3D::default(), None)
    }
}

fn to_json(s: &ParticleSettings3D, texture: Option<String>) -> EmitterJson {
    EmitterJson {
        rate: s.rate,
        burst: s.burst as u64,
        duration: s.duration,
        looping: s.looping,
        lifetime: s.lifetime,
        speed: s.speed,
        shape: match s.shape {
            ParticleShape3D::Sphere { radius } => ShapeJson::Sphere { radius },
            ParticleShape3D::Cone { angle } => ShapeJson::Cone { angle },
            ParticleShape3D::Box { half_extents } => ShapeJson::Box { half_extents },
            _ => ShapeJson::Point,
        },
        gravity: s.gravity,
        drag: s.drag,
        size: s.size,
        rotation: s.rotation,
        size_over_life: s.size_over_life.keys().to_vec(),
        color_over_life: s.color_over_life.keys().to_vec(),
        texture,
        sheet: s.sheet,
        blend: match s.blend {
            ParticleBlend3D::Additive => BlendJson::Additive,
            _ => BlendJson::Alpha,
        },
        max_particles: s.max_particles as u64,
        space: match s.space {
            ParticleSpace::World => SpaceJson::World,
            ParticleSpace::Local => SpaceJson::Local,
        },
        seed: s.seed,
        softness: s.softness,
    }
}

/// `None` when the emitter isn't saved: an image made in code has no name.
fn emitter_json(world: &World, e: &ParticleEmitter3D) -> Option<Result<Value, String>> {
    let s = e.settings();
    let texture = match s.texture {
        Some(t) => Some(world.get_resource::<TextureNames>()?.name(t.id)?),
        None => None,
    };
    Some(serde_json::to_value(to_json(s, texture)).map_err(|err| err.to_string()))
}

/// The largest pool a scene may ask for.
const MAX_POOL: usize = 1 << 24;

pub(crate) fn settings_from_json(world: &World, value: Value) -> Result<ParticleSettings3D, String> {
    let j: EmitterJson = serde_json::from_value(value).map_err(|err| err.to_string())?;
    let texture = match j.texture {
        Some(name) => Some(
            world
                .get_resource::<TextureNames>()
                .and_then(|t| t.get(&name))
                .ok_or_else(|| format!("texture \"{name}\" isn't loaded: ctx.gfx.load_texture(\"{name}\") first"))?,
        ),
        None => None,
    };
    let count = |n: u64, what: &str| usize::try_from(n).map_err(|_| format!("particle {what} is too large"));
    let defaults = ParticleSettings3D::default();
    let s = ParticleSettings3D {
        rate: j.rate,
        burst: count(j.burst, "burst")?,
        duration: j.duration,
        looping: j.looping,
        lifetime: j.lifetime,
        speed: j.speed,
        shape: match j.shape {
            ShapeJson::Point => ParticleShape3D::Point,
            ShapeJson::Sphere { radius } => ParticleShape3D::Sphere { radius },
            ShapeJson::Cone { angle } => ParticleShape3D::Cone { angle },
            ShapeJson::Box { half_extents } => ParticleShape3D::Box { half_extents },
        },
        gravity: j.gravity,
        drag: j.drag,
        size: j.size,
        rotation: j.rotation,
        size_over_life: if j.size_over_life.is_empty() {
            defaults.size_over_life
        } else {
            ParticleCurve::try_new(j.size_over_life)?
        },
        color_over_life: if j.color_over_life.is_empty() {
            defaults.color_over_life
        } else {
            ParticleGradient::try_new(j.color_over_life)?
        },
        texture,
        sheet: j.sheet,
        blend: match j.blend {
            BlendJson::Alpha => ParticleBlend3D::Alpha,
            BlendJson::Additive => ParticleBlend3D::Additive,
        },
        max_particles: count(j.max_particles, "max_particles")?,
        space: match j.space {
            SpaceJson::World => ParticleSpace::World,
            SpaceJson::Local => ParticleSpace::Local,
        },
        seed: j.seed,
        softness: j.softness,
        ..defaults
    };
    if s.max_particles > MAX_POOL {
        return Err(format!("particle max_particles is above {MAX_POOL}"));
    }
    ParticleEmitter3D::check(&s)?;
    Ok(s)
}

fn emitter_from_json(world: &World, value: Value) -> Result<ParticleEmitter3D, String> {
    Ok(ParticleEmitter3D::new(settings_from_json(world, value)?))
}

/// The emitter's JSON shape.
pub(super) fn schema() -> ValueSchema {
    let pair = || ValueSchema::Tuple(vec![ValueSchema::Number; 2]);
    ValueSchema::strict_object([
        ("rate", ValueSchema::Number),
        ("burst", ValueSchema::Unsigned),
        ("duration", ValueSchema::Number),
        ("looping", ValueSchema::Bool),
        ("lifetime", pair()),
        ("speed", pair()),
        (
            "shape",
            ValueSchema::any_of([
                ValueSchema::enum_of(["point"]),
                ValueSchema::strict_object([("sphere", ValueSchema::strict_object([("radius", ValueSchema::Number)]))]),
                ValueSchema::strict_object([("cone", ValueSchema::strict_object([("angle", ValueSchema::Number)]))]),
                ValueSchema::strict_object([(
                    "box",
                    ValueSchema::strict_object([("half_extents", ValueSchema::Vec3)]),
                )]),
            ]),
        ),
        ("gravity", ValueSchema::Vec3),
        ("drag", ValueSchema::Number),
        ("size", pair()),
        ("rotation", pair()),
        ("size_over_life", ValueSchema::array(pair())),
        (
            "color_over_life",
            ValueSchema::array(ValueSchema::Tuple(vec![ValueSchema::Number, ValueSchema::Color])),
        ),
        ("texture", ValueSchema::optional(ValueSchema::String)),
        ("sheet", ValueSchema::Tuple(vec![ValueSchema::Unsigned; 2])),
        ("blend", ValueSchema::enum_of(["alpha", "additive"])),
        ("max_particles", ValueSchema::Unsigned),
        ("space", ValueSchema::enum_of(["world", "local"])),
        ("seed", ValueSchema::Unsigned),
        ("softness", ValueSchema::Number),
    ])
}

/// Reflects [`ParticleEmitter3D`] as `"ParticleEmitter3D"`.
pub(crate) fn reflect_particle_emitter3d(world: &mut World) {
    if world.get_resource::<TextureNames>().is_none() {
        world.insert_resource(TextureNames::default());
    }
    if world.get_resource::<Registry>().is_none() {
        world.insert_resource(Registry::default());
    }
    let reg = world.resource_mut::<Registry>();
    if reg.by_name.contains_key(NAME) {
        return;
    }
    reg.schemas.insert(NAME.into(), (schema(), SchemaSource::Described));
    reg.by_name.insert(NAME.into(), reg.list.len());
    reg.list.push(Reflected {
        name: NAME.into(),
        type_name: std::any::type_name::<ParticleEmitter3D>().into(),
        get: Arc::new(|w: &World, e| w.get::<ParticleEmitter3D>(e).and_then(|p| emitter_json(w, p))),
        set: Arc::new(|w: &mut World, e, v| {
            let emitter = emitter_from_json(w, v)?;
            w.insert(e, emitter);
            Ok(())
        }),
        spawn: Arc::new(|w: &mut World, v| {
            let emitter = emitter_from_json(w, v)?;
            Ok(w.spawn((emitter,)))
        }),
        spawn_at: Arc::new(|w: &mut World, e, v| {
            let emitter = emitter_from_json(w, v)?;
            Ok(w.spawn_at(e, (emitter,)))
        }),
        check: Arc::new(check::<EmitterJson>),
        remove: Arc::new(remove::<ParticleEmitter3D>),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reflect::WorldReflect;

    fn world() -> World {
        let mut world = World::new();
        reflect_particle_emitter3d(&mut world);
        world
    }

    #[test]
    fn emitters_3d_round_trip_through_json() {
        let mut world = world();
        let settings = ParticleSettings3D::default()
            .with_rate(25.0)
            .with_shape(ParticleShape3D::Box {
                half_extents: Vec3::new(1.0, 2.0, 3.0),
            })
            .with_blend(ParticleBlend3D::Additive)
            .with_space(ParticleSpace::Local)
            .with_softness(0.5)
            .with_seed(7);
        let e = world.spawn((ParticleEmitter3D::new(settings),));
        let json = world.get_json(e, NAME).expect("reflected");
        assert_eq!(
            json["shape"],
            serde_json::json!({"box": {"half_extents": [1.0, 2.0, 3.0]}})
        );
        assert_eq!(json["blend"], "additive");
        let copy = world
            .spawn_json(serde_json::json!({ NAME: json.clone() }))
            .expect("spawns");
        assert_eq!(world.get_json(copy, NAME).unwrap(), json);
    }

    #[test]
    fn bad_3d_emitter_json_is_an_error_not_a_panic() {
        let mut world = world();
        for bad in [
            serde_json::json!({"lifetime": [2.0, 1.0]}),
            serde_json::json!({"sheet": [0, 1]}),
            serde_json::json!({"shape": {"sphere": {"radius": f32::NAN}}}),
            serde_json::json!({"texture": "missing.png"}),
            serde_json::json!({"max_particles": u64::MAX}),
            serde_json::json!({"shape": "torus"}),
        ] {
            assert!(
                world.spawn_json(serde_json::json!({ NAME: bad.clone() })).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_3d_schema_matches_the_json() {
        super::super::builtin_schemas::tests::validate(
            &schema(),
            &serde_json::to_value(EmitterJson::default()).unwrap(),
        );
    }
}
