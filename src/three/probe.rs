//! Local, axis-aligned reflection volumes. Captures are explicit
//! ([`Graphics::bake_reflection_probe`]) or follow a probe's
//! [`ReflectionRefresh3D`]: then the engine captures it again by itself, a
//! step per frame, and blends the new capture in over the old one.
use crate::d3::Transform as Transform3D;
use crate::gfx::Graphics;
use crate::texture::Texture;
use glam::Vec3;
use ponko_ecs::{Component, Entity, World};

/// A local environment at an entity's [`Transform3D::position`]. Rotation and
/// scale are ignored. Four baked environments can be resident; zero content
/// falls back to sky lighting. Capture excludes transparent objects.
/// ```
/// use ponko::prelude::*;
/// let probe = ponko::d3::ReflectionProbe { size: vec3(12.0, 4.0, 8.0), ..Default::default() };
/// assert_eq!(probe.intensity, 1.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
pub struct ReflectionProbe3D {
    /// Full axis-aligned box size in world units. Default 10 on each axis.
    pub size: Vec3,
    /// Fade width inside the box. Default 1; zero gives a hard boundary.
    pub blend_distance: f32,
    /// Multiplier on captured linear radiance. Default 1.
    pub intensity: f32,
    /// Opaque environment index assigned by the context; zero means unbaked.
    pub content: u32,
    /// Correct reflections for the box's walls. Default true.
    pub box_projection: bool,
}
impl Default for ReflectionProbe3D {
    fn default() -> Self {
        Self {
            size: Vec3::splat(10.0),
            blend_distance: 1.0,
            intensity: 1.0,
            content: 0,
            box_projection: true,
        }
    }
}
impl ReflectionProbe3D {
    /// Influence at `point`, fading to zero at the boundary.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::d3::ReflectionProbe::default().weight(Vec3::ZERO, Vec3::ZERO), 1.0);
    /// ```
    pub fn weight(&self, center: Vec3, point: Vec3) -> f32 {
        if !self.size.is_finite()
            || self.size.min_element() <= 0.0
            || !self.blend_distance.is_finite()
            || !self.intensity.is_finite()
            || self.intensity <= 0.0
            || !center.is_finite()
            || !point.is_finite()
        {
            return 0.0;
        }
        let edge = (self.size * 0.5 - (point - center).abs()).min_element();
        if edge <= 0.0 {
            0.0
        } else if self.blend_distance <= 0.0 {
            1.0
        } else {
            (edge / self.blend_distance).min(1.0)
        }
    }
    /// Direction from the capture center to the ray's exit from this box.
    /// Returns the input direction outside the box or when correction is off.
    /// ```
    /// use ponko::prelude::*;
    /// let p = ponko::d3::ReflectionProbe::default();
    /// assert_eq!(p.project(Vec3::ZERO, Vec3::ZERO, Vec3::Z), Vec3::Z * 5.0);
    /// ```
    pub fn project(&self, center: Vec3, point: Vec3, direction: Vec3) -> Vec3 {
        if !self.box_projection || self.weight(center, point) == 0.0 || !direction.is_finite() {
            return direction;
        }
        let local = point - center;
        let mut distance = f32::INFINITY;
        for axis in 0..3 {
            let d = direction[axis];
            if d.abs() > 1e-7 {
                distance = distance.min((self.size[axis] * 0.5 * d.signum() - local[axis]) / d);
            }
        }
        if distance.is_finite() {
            local + direction * distance
        } else {
            direction
        }
    }
}

/// When the engine captures a [`ReflectionProbe3D`] again by itself.
///
/// ```
/// use ponko::d3::ProbeRefresh;
/// let every = ProbeRefresh::Every(2.0); // seconds
/// assert_ne!(every, ProbeRefresh::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum ProbeRefresh {
    /// Never: the probe holds what the game baked or loaded.
    #[default]
    Never,
    /// Once when the probe has no content yet, then on each
    /// [`Graphics::refresh_reflection_probe`].
    OnDemand,
    /// Again every this many seconds of frame time, counted from the start
    /// of the previous capture.
    Every(f32),
    /// Again when the sun or the sky's light has changed by more than this
    /// share since the previous capture (0.02 is about one degree of sun or
    /// two percent of a light's color). A sky shader counts as the sky's
    /// light: switching [`Sky::shader`](crate::render::Sky::shader), or a
    /// change of its [`shader_params`](crate::render::Sky::shader_params),
    /// refreshes the probe too.
    LightingChanged(f32),
}

/// Keeps the [`ReflectionProbe3D`] on the same entity up to date by itself.
///
/// A refresh is spread over frames: each frame does at most one step for
/// every probe together (renders one of the six 128² faces, filters one
/// face of the new environment, or makes its diffuse light in two steps:
/// [`REFRESH_STEPS`](Self::REFRESH_STEPS) in all), into a copy the probe
/// doesn't show yet. Then the probe blends from its old capture to the new
/// one over [`blend_seconds`](Self::blend_seconds), easing in and out, so a
/// changing sky never pops. A probe with no content gets a slot on the next
/// frame drawn and starts from the sky's environment; one that can't (four
/// are in use) stays on the sky. Each refreshing probe keeps a 2.5 MiB copy
/// of its maps on the GPU.
/// [`Graphics::bake_reflection_probe`] and
/// [`load_reflection_probe`](Graphics::load_reflection_probe) still replace
/// the content at once, cancelling a refresh in progress.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::{ProbeRefresh, ReflectionProbe, ReflectionRefresh, Transform};
/// Game::new("probe", 1, 1).run_headless(0, |ctx| {
///     ctx.world.spawn((
///         Transform::default(),
///         ReflectionProbe::default(),
///         ReflectionRefresh { when: ProbeRefresh::LightingChanged(0.02), ..Default::default() },
///     ));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
pub struct ReflectionRefresh3D {
    /// When to capture again. Default [`ProbeRefresh::OnDemand`].
    pub when: ProbeRefresh,
    /// Seconds the probe takes to blend from its old capture to a new one.
    /// Default 1; zero switches at once.
    pub blend_seconds: f32,
}

impl Default for ReflectionRefresh3D {
    fn default() -> Self {
        Self {
            when: ProbeRefresh::OnDemand,
            blend_seconds: 1.0,
        }
    }
}

impl ReflectionRefresh3D {
    /// Frames one refresh's capture takes before it starts blending in:
    /// six faces rendered, six filtered, one for the spherical harmonics and
    /// one for the diffuse light from them.
    /// Every frame does at most one such step for all probes together.
    pub const REFRESH_STEPS: u32 = 14;
}

/// Frames of a refresh that render a face, then those that filter one.
const CAPTURE_STEPS: u8 = 6;

/// The sun and sky values a [`ProbeRefresh::LightingChanged`] probe watches.
/// The sun, the sky's light and colors, then a sky shader (its id plus one,
/// 0 for none), its authored-ambient flag and its sixteen parameters.
pub(crate) type LightingKey = [f32; 39];

/// What a probe's capture depends on in `lighting` and `sky`.
pub(crate) fn lighting_key(lighting: &super::Lighting, sky: &super::Sky) -> LightingKey {
    let rgb = |c| {
        let [r, g, b, _] = super::linear(c);
        [r, g, b]
    };
    let d = lighting.sun_direction.normalize_or_zero();
    let sun = rgb(lighting.sun_color).map(|c| c * lighting.sun_intensity);
    let [sr, sg, sb] = rgb(sky.sky_color);
    let [gr, gg, gb] = rgb(sky.ground_color);
    let [er, eg, eb] = sky.equator_color.map_or([0.0; 3], rgb);
    let [nr, ng, nb] = rgb(sky.night_light);
    let mut key = [0.0; 39];
    let lit = [
        d.x,
        d.y,
        d.z,
        sun[0],
        sun[1],
        sun[2],
        lighting.ambient_intensity,
        sr,
        sg,
        sb,
        gr,
        gg,
        gb,
        er,
        eg,
        eb,
        nr,
        ng,
        nb,
        sky.haze,
        f32::from(sky.atmosphere),
    ];
    key[..lit.len()].copy_from_slice(&lit);
    // A sky shader's sky is what a probe captures behind its scene: a new
    // shader, or new parameters, change it as the sun does.
    if let Some(shader) = sky.shader {
        key[21] = shader.id as f32 + 1.0;
        key[22] = f32::from(sky.authored_ambient);
        for (k, v) in key[23..]
            .iter_mut()
            .zip(sky.shader_params.iter().flat_map(|p| p.to_array()))
        {
            *k = v;
        }
    }
    key
}

/// The largest change between two lighting keys, relative to the values
/// (or to 1, for values below 1). NaN counts as a full change.
pub(crate) fn lighting_change(a: &LightingKey, b: &LightingKey) -> f32 {
    a.iter().zip(b).fold(0.0, |most: f32, (a, b)| {
        let change = (a - b).abs() / a.abs().max(b.abs()).max(1.0);
        if change.is_nan() {
            f32::INFINITY
        } else {
            most.max(change)
        }
    })
}

/// One piece of an automatic refresh, for the renderer (slots count from 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RefreshStep {
    /// A new slot: the sky's maps become its content (no work counted).
    Seed(u32),
    /// Face `face` is rendered into render target `target` (the capture
    /// camera is a target camera of this frame).
    Capture { slot: u32, face: u32, target: u32 },
    /// Face `face` of the new maps is filtered; face 0 first lays the six
    /// captured faces (render targets `faces`) out as the probe's cube.
    Filter { slot: u32, face: u32, faces: [u32; 6] },
    /// The new capture's spherical harmonics.
    Harmonics(u32),
    /// The new maps' diffuse light, from the harmonics.
    Finish(u32),
    /// The shown maps move `weight` of the way to the new ones.
    Blend { slot: u32, weight: f32 },
}

/// A slot's automatic refresh.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Refreshing {
    /// Steps done of the refresh in progress.
    pub step: Option<u8>,
    /// Where the refresh in progress captures from.
    pub center: Vec3,
    /// Seconds of blending left; zero when not blending.
    pub blend_left: f32,
    /// Seconds the blend in progress takes in all.
    pub blend_total: f32,
    /// Seconds since the last refresh started.
    pub since: f32,
    /// The lighting the last refresh started under.
    pub lighting: Option<LightingKey>,
    /// A refresh is due.
    pub requested: bool,
    /// The slot's maps hold something (baked, loaded or seeded).
    pub filled: bool,
}

/// The scheduler's state, copied before a frame so a frame that isn't
/// drawn can be undone.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RefreshState {
    pub slots: [Refreshing; 4],
    /// The slot whose refresh is in progress.
    pub active: Option<u32>,
    /// Where the search for the next due refresh starts (round robin).
    pub next: u32,
}

/// One probe as the scheduler sees it this frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProbeSeen {
    pub slot: u32,
    pub center: Vec3,
    pub refresh: Option<ReflectionRefresh3D>,
}

impl RefreshState {
    /// One frame: decides which refreshes are due, advances the one in
    /// progress by one step, and the blends by `dt`. Appends the frame's
    /// steps to `steps`; `faces` gives a slot's six capture targets.
    pub fn frame(
        &mut self,
        dt: f32,
        lighting: &LightingKey,
        probes: &[ProbeSeen],
        faces: impl Fn(u32) -> [u32; 6],
        steps: &mut Vec<RefreshStep>,
    ) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        for probe in probes {
            let Some(state) = self.slots.get_mut(probe.slot as usize - 1) else {
                continue;
            };
            state.since += dt;
            let Some(refresh) = probe.refresh else {
                continue;
            };
            let due = match refresh.when {
                ProbeRefresh::Never => false,
                ProbeRefresh::OnDemand => !state.filled,
                ProbeRefresh::Every(seconds) => !state.filled || state.since >= seconds * 0.999,
                ProbeRefresh::LightingChanged(threshold) => {
                    !state.filled
                        || state
                            .lighting
                            .is_none_or(|before| lighting_change(&before, lighting) > threshold)
                }
            };
            state.requested |= due && state.step.is_none();
        }
        for probe in probes {
            let Some(state) = self.slots.get_mut(probe.slot as usize - 1) else {
                continue;
            };
            if !state.filled && state.requested {
                steps.push(RefreshStep::Seed(probe.slot));
                state.filled = true;
            }
        }
        if self.active.is_none() {
            let start = self.next;
            for i in 0..4 {
                let slot = (start + i) % 4 + 1;
                let Some(probe) = probes.iter().find(|p| p.slot == slot) else {
                    continue;
                };
                let state = &mut self.slots[slot as usize - 1];
                // A blend may still be running: the faces are captured
                // meanwhile, and filtering waits for it (below).
                if state.requested {
                    *state = Refreshing {
                        step: Some(0),
                        center: probe.center,
                        since: 0.0,
                        lighting: Some(*lighting),
                        requested: false,
                        filled: true,
                        ..*state
                    };
                    self.active = Some(slot);
                    self.next = slot % 4;
                    break;
                }
            }
        }
        if let Some(slot) = self.active {
            let blend = probes
                .iter()
                .find(|p| p.slot == slot)
                .map_or(1.0, |p| p.refresh.unwrap_or_default().blend_seconds);
            let state = &mut self.slots[slot as usize - 1];
            let step = state.step.unwrap_or(0);
            // Filtering writes the copy the slot's blend reads: it waits
            // for the blend to end (no work this frame).
            let stalled = step >= CAPTURE_STEPS && state.blend_left > 0.0;
            if !stalled {
                steps.push(match step {
                    s if s < CAPTURE_STEPS => RefreshStep::Capture {
                        slot,
                        face: u32::from(s),
                        target: faces(slot)[usize::from(s)],
                    },
                    s if s < 2 * CAPTURE_STEPS => RefreshStep::Filter {
                        slot,
                        face: u32::from(s - CAPTURE_STEPS),
                        faces: faces(slot),
                    },
                    s if s == 2 * CAPTURE_STEPS => RefreshStep::Harmonics(slot),
                    _ => RefreshStep::Finish(slot),
                });
                if u32::from(step) + 1 < ReflectionRefresh3D::REFRESH_STEPS {
                    state.step = Some(step + 1);
                } else {
                    state.step = None;
                    self.active = None;
                    if blend.is_finite() && blend > 0.0 {
                        state.blend_left = blend;
                        state.blend_total = blend;
                    } else {
                        steps.push(RefreshStep::Blend { slot, weight: 1.0 });
                    }
                }
            }
        }
        for (i, state) in self.slots.iter_mut().enumerate() {
            if state.blend_left > 0.0 {
                // The shown share of the new maps eases in and out over the
                // blend (a smoothstep of the time): no sudden start or stop,
                // which a linear blend has where a colour leaves or reaches
                // black. Each frame moves the rest of the way by as much as
                // the share grows. (Within a thousandth: sums of frame
                // times drift.)
                let total = state.blend_total.max(state.blend_left);
                let ease = |t: f32| {
                    let t = t.clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                let before = ease(1.0 - state.blend_left / total);
                let weight = if dt >= state.blend_left * 0.999 {
                    1.0
                } else {
                    let after = ease(1.0 - (state.blend_left - dt) / total);
                    (after - before) / (1.0 - before)
                };
                state.blend_left = if weight >= 1.0 { 0.0 } else { state.blend_left - dt };
                if weight > 0.0 {
                    steps.push(RefreshStep::Blend {
                        slot: i as u32 + 1,
                        weight,
                    });
                }
            }
        }
    }

    /// The game asked for a refresh of `slot`.
    pub fn request(&mut self, slot: u32) {
        if let Some(state) = self.slots.get_mut(slot as usize - 1) {
            state.requested = true;
        }
    }

    /// `slot` was baked or loaded at once: a refresh or blend in progress
    /// would bring back older content.
    pub fn replaced(&mut self, slot: u32) {
        if let Some(state) = self.slots.get_mut(slot as usize - 1) {
            state.step = None;
            state.blend_left = 0.0;
            state.blend_total = 0.0;
            state.filled = true;
            if self.active == Some(slot) {
                self.active = None;
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct ProbeAssets {
    pub captures: Vec<[Texture; 6]>,
    pub pending: Vec<(u32, Vec3)>,
    pub uploads: Vec<(u32, std::sync::Arc<Vec<u16>>)>,
    /// What the frame being drawn took from the two lists above, kept until
    /// it is known to have been drawn.
    pub in_flight: Vec<(u32, Vec3)>,
    pub in_flight_uploads: Vec<(u32, std::sync::Arc<Vec<u16>>)>,
    /// Automatic refreshes ([`ReflectionRefresh3D`]).
    pub refresh: RefreshState,
    /// `refresh` before the frame being drawn, to undo it if it isn't.
    pub refresh_before: Option<RefreshState>,
    /// Reused each frame: the probes the scheduler sees, and new ones that
    /// need a slot.
    pub seen: Vec<ProbeSeen>,
    pub fresh: Vec<Entity>,
}

impl ProbeAssets {
    /// Takes this frame's requests (see `Ctx::collect_views`).
    pub fn take_requests(&mut self) {
        self.in_flight.clear();
        self.in_flight_uploads.clear();
        self.in_flight.append(&mut self.pending);
        self.in_flight_uploads.append(&mut self.uploads);
    }
    /// After a frame: if it was drawn the requests are done; if not (hidden
    /// window), ask again next frame, unless the game has asked for that
    /// environment again since.
    pub fn settle(&mut self, drawn: bool) {
        if let Some(before) = self.refresh_before.take()
            && !drawn
        {
            self.refresh = before;
        }
        if drawn {
            self.in_flight.clear();
            self.in_flight_uploads.clear();
            return;
        }
        for (slot, center) in std::mem::take(&mut self.in_flight) {
            let asked = self.pending.iter().any(|&(s, _)| s == slot) || self.uploads.iter().any(|(s, _)| *s == slot);
            if !asked {
                self.pending.push((slot, center));
            }
        }
        for (slot, data) in std::mem::take(&mut self.in_flight_uploads) {
            let asked = self.pending.iter().any(|&(s, _)| s == slot) || self.uploads.iter().any(|(s, _)| *s == slot);
            if !asked {
                self.uploads.push((slot, data));
            }
        }
    }
}

impl Graphics {
    /// Queue six 128² HDR scene views and GGX filtering on the next rendered
    /// frame, for the probe on `entity` of `world` (the game's is `ctx.world`). Calling again refreshes the same content. At most four captures
    /// can be allocated in a context. Headless simulation queues no GPU work.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("probe", 1, 1).run_headless(0, |ctx| {
    ///     let e = ctx.world.spawn((ponko::d3::Transform::default(), ponko::d3::ReflectionProbe::default()));
    ///     ctx.gfx.bake_reflection_probe(&mut ctx.world, e).unwrap();
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if `entity` has no
    /// probe or no transform, [`Invalid`](crate::error::ErrorKind::Invalid) if the
    /// probe's volume or intensity is zero or its content is unknown,
    /// [`Limit`](crate::error::ErrorKind::Limit) if four captures are already
    /// allocated.
    pub fn bake_reflection_probe(&mut self, world: &mut World, entity: Entity) -> Result<(), crate::error::Error> {
        let (slot, center) = self.probe_slot(world, entity, true)?;
        self.assets3d.probes.pending.retain(|&(s, _)| s != slot);
        self.assets3d.probes.pending.push((slot, center));
        self.assets3d.probes.refresh.replaced(slot);
        Ok(())
    }

    /// Capture the probe on `entity` of `world` again over the next frames
    /// and blend it in, as its [`ReflectionRefresh3D`] does (a probe without
    /// one blends over a second). Use it for
    /// [`ProbeRefresh::OnDemand`]; [`bake_reflection_probe`](Self::bake_reflection_probe)
    /// replaces the content at once instead. A refresh already in progress
    /// finishes first. Headless simulation queues no GPU work.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("probe", 1, 1).run_headless(0, |ctx| {
    ///     let e = ctx.world.spawn((ponko::d3::Transform::default(), ponko::d3::ReflectionProbe::default()));
    ///     ctx.gfx.refresh_reflection_probe(&mut ctx.world, e).unwrap();
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// The same as [`bake_reflection_probe`](Self::bake_reflection_probe).
    pub fn refresh_reflection_probe(&mut self, world: &mut World, entity: Entity) -> Result<(), crate::error::Error> {
        let (slot, _) = self.probe_slot(world, entity, false)?;
        self.assets3d.probes.refresh.request(slot);
        Ok(())
    }

    /// The probe's slot (allocated when it has none: `filled` says whether
    /// its maps will hold something this frame) and its center.
    fn probe_slot(
        &mut self,
        world: &mut World,
        entity: Entity,
        filled: bool,
    ) -> Result<(u32, Vec3), crate::error::Error> {
        use crate::error::{Error, ErrorKind};
        let probe = *world
            .get::<ReflectionProbe3D>(entity)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "entity has no reflection probe"))?;
        let center = world
            .get::<Transform3D>(entity)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "probe has no transform"))?
            .position;
        if probe.weight(center, center) == 0.0 {
            return Err(Error::new(ErrorKind::Invalid, "probe has invalid volume or intensity"));
        }
        if probe.content != 0 {
            if probe.content as usize > self.assets3d.probes.captures.len() {
                return Err(Error::new(ErrorKind::Invalid, "invalid reflection content"));
            }
            return Ok((probe.content, center));
        }
        if self.assets3d.probes.captures.len() == 4 {
            return Err(Error::new(
                ErrorKind::Limit,
                "four reflection environments are already allocated",
            ));
        }
        let faces = std::array::from_fn(|_| {
            self.make_render_target(128, 128)
                .unwrap_or_else(|e| panic!("{e}"))
                .texture
        });
        self.assets3d.probes.captures.push(faces);
        let slot = self.assets3d.probes.captures.len() as u32;
        world.get_mut::<ReflectionProbe3D>(entity).unwrap().content = slot;
        self.assets3d.probes.refresh.slots[slot as usize - 1] = Refreshing {
            filled,
            ..Default::default()
        };
        Ok((slot, center))
    }

    /// Engine side, before collecting the frame's views: gives new
    /// refreshing probes a slot, then runs the refresh scheduler for one
    /// frame of `dt` seconds into `steps`.
    pub(crate) fn schedule_probe_refresh(
        &mut self,
        world: &mut World,
        dt: f32,
        lighting: &LightingKey,
        steps: &mut Vec<RefreshStep>,
    ) {
        let mut fresh = std::mem::take(&mut self.assets3d.probes.fresh);
        fresh.clear();
        // A probe that can't have a slot (four are in use, or its box is
        // empty) stays on the sky, as an unbaked probe does, and isn't
        // tried again every frame.
        if self.assets3d.probes.captures.len() < 4 {
            world
                .query::<(Entity, &Transform3D, &ReflectionProbe3D, &ReflectionRefresh3D)>()
                .for_each(|(e, t, p, r)| {
                    if p.content == 0 && r.when != ProbeRefresh::Never && p.weight(t.position, t.position) > 0.0 {
                        fresh.push(e);
                    }
                });
        }
        for &e in &fresh {
            let _ = self.probe_slot(world, e, false);
        }
        self.assets3d.probes.fresh = fresh;
        let probes = &mut self.assets3d.probes;
        probes.seen.clear();
        let seen = &mut probes.seen;
        let count = probes.captures.len();
        world
            .query::<(&Transform3D, &ReflectionProbe3D, Option<&ReflectionRefresh3D>)>()
            .for_each(|(t, p, r)| {
                if (1..=count).contains(&(p.content as usize)) && p.weight(t.position, t.position) > 0.0 {
                    seen.push(ProbeSeen {
                        slot: p.content,
                        center: t.position,
                        refresh: r.copied(),
                    });
                }
            });
        probes.refresh_before = Some(probes.refresh);
        let captures = &probes.captures;
        probes.refresh.frame(
            dt,
            lighting,
            &probes.seen,
            |slot| captures[slot as usize - 1].map(|t| t.id),
            steps,
        );
    }
}

/// Packed frame metadata; also the shader uniform layout.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ProbeUniform {
    pub center: [f32; 4],
    pub size: [f32; 4],
    pub params: [f32; 4],
}

impl Graphics {
    /// Load an HDR/EXR latitude-longitude image, from the [added asset
    /// sources](crate::assets::Assets::add_source) or `assets/`, as the baked content
    /// of the probe on `entity` of `world` (the game's is `ctx.world`).
    /// Pixels are linear radiance, with +Y at the top and +X at the center.
    /// The file must have a 2:1 aspect ratio. Filtering runs next rendered frame.
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn load(ctx: &mut Ctx, probe: Entity) {
    /// ctx.gfx.load_reflection_probe(&mut ctx.world, probe, "interior.exr");
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the file is missing or not a 2:1 image, or the probe can't be
    /// baked; use [`try_load_reflection_probe`](Self::try_load_reflection_probe)
    /// to handle that.
    pub fn load_reflection_probe(&mut self, world: &mut World, entity: Entity, name: &str) {
        self.try_load_reflection_probe(world, entity, name)
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// [`load_reflection_probe`](Self::load_reflection_probe) with errors as
    /// values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) if the file
    /// is missing, [`Io`](crate::error::ErrorKind::Io) if it can't be read,
    /// [`Decode`](crate::error::ErrorKind::Decode) if it isn't an image or
    /// holds non-finite radiance, [`Invalid`](crate::error::ErrorKind::Invalid)
    /// if it isn't 2:1, and the errors of
    /// [`bake_reflection_probe`](Self::bake_reflection_probe).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("probe", 1, 1).run_headless(0, |ctx| {
    ///     let e = ctx.world.spawn((ponko::d3::Transform::default(), ponko::d3::ReflectionProbe::default()));
    ///     let err = ctx.gfx.try_load_reflection_probe(&mut ctx.world, e, "no/such.exr").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    pub fn try_load_reflection_probe(
        &mut self,
        world: &mut World,
        entity: Entity,
        name: &str,
    ) -> Result<(), crate::error::Error> {
        let pixels = self.try_load_environment(name)?.0;
        self.bake_reflection_probe(world, entity)?;
        let slot = world.get::<ReflectionProbe3D>(entity).unwrap().content;
        self.assets3d.probes.pending.retain(|&(s, _)| s != slot);
        self.assets3d.probes.uploads.retain(|(s, _)| *s != slot);
        self.assets3d.probes.uploads.push((slot, pixels));
        self.assets3d.probes.refresh.replaced(slot);
        Ok(())
    }
}

pub(crate) fn cube_direction(face: u32, u: f32, v: f32) -> Vec3 {
    let x = u * 2.0 - 1.0;
    let y = v * 2.0 - 1.0;
    match face {
        0 => Vec3::new(1.0, -y, -x),
        1 => Vec3::new(-1.0, -y, x),
        2 => Vec3::new(x, 1.0, y),
        3 => Vec3::new(x, -1.0, -y),
        4 => Vec3::new(x, -y, 1.0),
        _ => Vec3::new(-x, -y, -1.0),
    }
    .normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(slot: u32, when: ProbeRefresh, blend_seconds: f32) -> ProbeSeen {
        ProbeSeen {
            slot,
            center: Vec3::ZERO,
            refresh: Some(ReflectionRefresh3D { when, blend_seconds }),
        }
    }

    fn faces(slot: u32) -> [u32; 6] {
        std::array::from_fn(|i| slot * 10 + i as u32)
    }

    /// Steps that cost a face's work (captures, filters, the finish).
    fn work(steps: &[RefreshStep]) -> usize {
        steps
            .iter()
            .filter(|s| !matches!(s, RefreshStep::Seed(_) | RefreshStep::Blend { .. }))
            .count()
    }

    /// Runs `frames` frames; returns each frame's steps.
    fn run(
        state: &mut RefreshState,
        frames: usize,
        dt: f32,
        key: &LightingKey,
        probes: &[ProbeSeen],
    ) -> Vec<Vec<RefreshStep>> {
        (0..frames)
            .map(|_| {
                let mut steps = Vec::new();
                state.frame(dt, key, probes, faces, &mut steps);
                steps
            })
            .collect()
    }

    #[test]
    fn a_refresh_renders_and_filters_one_face_a_frame_then_eases_in() {
        let mut state = RefreshState::default();
        let probes = [seen(1, ProbeRefresh::OnDemand, 0.5)];
        let frames = run(&mut state, 20, 0.1, &[0.0; 39], &probes);
        assert_eq!(frames[0][0], RefreshStep::Seed(1), "a new probe starts from the sky");
        for (face, frame) in frames[..6].iter().enumerate() {
            assert!(frame.contains(&RefreshStep::Capture {
                slot: 1,
                face: face as u32,
                target: 10 + face as u32
            }));
        }
        for (face, frame) in frames[6..12].iter().enumerate() {
            assert!(frame.contains(&RefreshStep::Filter {
                slot: 1,
                face: face as u32,
                faces: faces(1)
            }));
        }
        assert!(frames[12].contains(&RefreshStep::Harmonics(1)));
        assert!(frames[13].contains(&RefreshStep::Finish(1)));
        assert!(frames.iter().all(|f| work(f) <= 1), "at most one face's work a frame");
        // From the finish on, the shown share of the new capture follows a
        // smoothstep of the time over the 0.5 s blend (0.1 s frames):
        // slow at both ends, never a jump.
        let mut shown = 0.0;
        for (i, frame) in frames[13..18].iter().enumerate() {
            let RefreshStep::Blend { weight, .. } = *frame.last().unwrap() else {
                panic!("frame {i} blends: {frame:?}");
            };
            shown += (1.0 - shown) * weight;
            let t = 0.2 * (i + 1) as f32;
            let expected = t * t * (3.0 - 2.0 * t);
            assert!((shown - expected).abs() < 1e-4, "frame {i}: {shown} vs {expected}");
        }
        assert_eq!(shown, 1.0);
        assert!(
            frames[18..].iter().all(Vec::is_empty),
            "on demand: nothing more until asked"
        );
        state.request(1);
        let again = run(&mut state, 1, 0.1, &[0.0; 39], &probes);
        assert!(again[0].contains(&RefreshStep::Capture {
            slot: 1,
            face: 0,
            target: 10
        }));
    }

    #[test]
    fn never_captures_nothing_by_itself_but_a_request_still_refreshes() {
        let mut state = RefreshState::default();
        state.slots[0].filled = true;
        let probes = [seen(1, ProbeRefresh::Never, 1.0)];
        assert!(run(&mut state, 30, 0.1, &[0.0; 39], &probes).iter().all(Vec::is_empty));
        state.request(1);
        assert_eq!(work(&run(&mut state, 1, 0.1, &[0.0; 39], &probes)[0]), 1);
    }

    #[test]
    fn every_n_seconds_counts_from_the_start_of_the_last_capture() {
        let mut state = RefreshState::default();
        state.slots[0].filled = true;
        let probes = [seen(1, ProbeRefresh::Every(3.0), 0.0)];
        let frames = run(&mut state, 100, 0.1, &[0.0; 39], &probes);
        let starts: Vec<_> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.iter().any(|s| matches!(s, RefreshStep::Capture { face: 0, .. })))
            .map(|(i, _)| i)
            .collect();
        // Due once 3 s have passed: the 30th frame after the last start.
        assert_eq!(starts, [29, 59, 89]);
    }

    #[test]
    fn lighting_changed_refreshes_only_past_the_threshold() {
        let mut state = RefreshState::default();
        let probes = [seen(1, ProbeRefresh::LightingChanged(0.05), 0.0)];
        let mut key = [0.5; 39];
        let first = run(&mut state, 15, 0.1, &key, &probes);
        assert_eq!(
            first.iter().map(|f| work(f)).sum::<usize>(),
            14,
            "one capture to start with"
        );
        key[0] = 0.54; // the sun moved a little
        assert!(run(&mut state, 20, 0.1, &key, &probes).iter().all(Vec::is_empty));
        key[0] = 0.56;
        assert_eq!(work(&run(&mut state, 1, 0.1, &key, &probes)[0]), 1);
        assert!(lighting_change(&[0.0; 39], &[f32::NAN; 39]).is_infinite());
    }

    #[test]
    fn a_sky_shader_and_its_parameters_count_as_a_lighting_change() {
        let lighting = super::super::Lighting::default();
        let built_in = super::super::Sky::default();
        let shader = super::super::Sky {
            shader: Some(super::super::SkyShader { id: 0 }),
            ..built_in
        };
        let base = lighting_key(&lighting, &built_in);
        assert!(
            lighting_change(&base, &lighting_key(&lighting, &shader)) > 0.5,
            "a new sky"
        );
        let mut dimmer = shader;
        dimmer.shader_params[1].y = 0.5;
        assert!(
            lighting_change(&lighting_key(&lighting, &shader), &lighting_key(&lighting, &dimmer)) >= 0.5,
            "a parameter changed"
        );
        let other = super::super::Sky {
            shader: Some(super::super::SkyShader { id: 1 }),
            ..shader
        };
        assert!(lighting_change(&lighting_key(&lighting, &shader), &lighting_key(&lighting, &other)) > 0.3);
        // Negative control: the same sky is no change.
        assert_eq!(
            lighting_change(&lighting_key(&lighting, &dimmer), &lighting_key(&lighting, &dimmer)),
            0.0
        );
    }

    #[test]
    fn two_probes_share_the_one_step_a_frame_and_both_finish() {
        let mut state = RefreshState::default();
        let probes = [
            seen(1, ProbeRefresh::OnDemand, 0.0),
            seen(2, ProbeRefresh::OnDemand, 0.0),
        ];
        let frames = run(&mut state, 30, 0.1, &[0.0; 39], &probes);
        assert!(frames.iter().all(|f| work(f) <= 1));
        for slot in [1, 2] {
            assert!(frames.iter().flatten().any(|s| *s == RefreshStep::Finish(slot)));
            assert!(
                frames
                    .iter()
                    .flatten()
                    .any(|s| *s == RefreshStep::Blend { slot, weight: 1.0 })
            );
        }
    }

    #[test]
    fn a_new_capture_filters_only_after_the_blend_it_would_overwrite() {
        let mut state = RefreshState::default();
        let probes = [seen(1, ProbeRefresh::OnDemand, 1.0)];
        run(&mut state, 14, 0.1, &[0.0; 39], &probes);
        state.request(1);
        let frames = run(&mut state, 30, 0.1, &[0.0; 39], &probes);
        let capture = frames
            .iter()
            .position(|f| f.iter().any(|s| matches!(s, RefreshStep::Capture { .. })))
            .unwrap();
        let filter = frames
            .iter()
            .position(|f| f.iter().any(|s| matches!(s, RefreshStep::Filter { .. })))
            .unwrap();
        let last_blend = frames
            .iter()
            .position(|f| f.contains(&RefreshStep::Blend { slot: 1, weight: 1.0 }))
            .unwrap();
        assert_eq!(capture, 0, "the faces are captured while the blend runs");
        assert!(filter > last_blend, "filter at {filter}, blend until {last_blend}");
        assert!(frames.iter().all(|f| work(f) <= 1));
    }

    #[test]
    fn a_bake_cancels_the_refresh_in_progress() {
        let mut state = RefreshState::default();
        let probes = [seen(1, ProbeRefresh::OnDemand, 1.0)];
        run(&mut state, 3, 0.1, &[0.0; 39], &probes);
        state.replaced(1);
        assert_eq!(state.active, None);
        assert!(run(&mut state, 20, 0.1, &[0.0; 39], &probes).iter().all(Vec::is_empty));
    }

    #[test]
    fn a_frame_that_was_not_drawn_undoes_its_refresh_steps() {
        let mut probes = ProbeAssets::default();
        probes.refresh.request(1);
        probes.refresh_before = Some(probes.refresh);
        let mut steps = Vec::new();
        probes.refresh.frame(
            0.1,
            &[0.0; 39],
            &[seen(1, ProbeRefresh::OnDemand, 1.0)],
            faces,
            &mut steps,
        );
        assert_eq!(probes.refresh.active, Some(1));
        probes.settle(false);
        assert_eq!(probes.refresh.active, None);
        assert!(probes.refresh.slots[0].requested);
    }

    #[test]
    fn a_refreshing_probe_gets_a_slot_by_itself_unless_none_is_free() {
        let mut ctx = crate::prelude::Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_headless(true);
        let refreshing = || {
            (
                Transform3D::default(),
                ReflectionProbe3D::default(),
                ReflectionRefresh3D::default(),
            )
        };
        let first = ctx.world.spawn(refreshing());
        let mut steps = Vec::new();
        ctx.gfx
            .schedule_probe_refresh(&mut ctx.world, 0.1, &[0.0; 39], &mut steps);
        let slot = ctx.world.get::<ReflectionProbe3D>(first).unwrap().content;
        assert_eq!(slot, 1);
        assert_eq!(steps[0], RefreshStep::Seed(1), "it starts from the sky: {steps:?}");
        assert!(matches!(steps[1], RefreshStep::Capture { slot: 1, face: 0, .. }));
        for _ in 0..3 {
            let e = ctx.world.spawn((Transform3D::default(), ReflectionProbe3D::default()));
            ctx.gfx.bake_reflection_probe(&mut ctx.world, e).unwrap();
        }
        let fifth = ctx.world.spawn(refreshing());
        for _ in 0..3 {
            steps.clear();
            ctx.gfx
                .schedule_probe_refresh(&mut ctx.world, 0.1, &[0.0; 39], &mut steps);
        }
        assert_eq!(ctx.world.get::<ReflectionProbe3D>(fifth).unwrap().content, 0);
        assert!(steps.iter().all(|s| !matches!(s, RefreshStep::Seed(5))));
    }

    #[test]
    fn a_frame_that_was_not_drawn_hands_its_requests_to_the_next() {
        let mut probes = ProbeAssets::default();
        probes.pending.push((1, Vec3::X));
        probes.uploads.push((2, std::sync::Arc::new(vec![0])));
        probes.take_requests();
        assert!(probes.pending.is_empty() && probes.uploads.is_empty());
        probes.pending.push((1, Vec3::Y)); // asked again meanwhile
        probes.settle(false);
        assert_eq!(probes.pending, vec![(1, Vec3::Y)]);
        assert_eq!(probes.uploads.len(), 1);
        probes.take_requests();
        probes.settle(true);
        assert!(probes.in_flight.is_empty() && probes.in_flight_uploads.is_empty());
        probes.settle(false);
        assert!(probes.pending.is_empty());
    }
}
