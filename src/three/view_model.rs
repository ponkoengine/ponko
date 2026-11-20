//! First-person held items: [`ViewModel`] (`ctx.scene3d.view_model`) and
//! [`HeldItem`], drawn by the view-model pass.
//!
//! The pass is a small addition at the end of the frame's color pass (see
//! `render/gpu3d/view_model.rs`): the held items' models, collected for a
//! camera of their own, drawn into the scene's depth range nearest the eye
//! so nothing in the world can cover them. Without a `ViewModel` nothing of
//! it exists: no collection, no buffer, no draw.

use glam::{Quat, Vec3};
use ponko_ecs::{Component, Entity, World};

use super::{Camera3D, Transform3D};
use crate::camera::RenderLayers;

/// The layer [`ViewModel::default`] draws: the last one, out of the way of
/// the layers games number from 0.
pub const VIEW_MODEL_LAYER: u32 = 31;

/// The first-person view model: held items (a weapon, a torch, hands)
/// drawn with a camera of their own, over the world, so they never clip
/// into a wall the player stands against. Set
/// [`SceneSettings::view_model`](crate::render::SceneSettings::view_model)
/// and give each held item a [`HeldItem`].
///
/// The view-model camera stands where the main camera stands and looks the
/// same way, with its own [`fov_y`](Self::fov_y), [`near`](Self::near) and
/// [`far`](Self::far): a held item keeps its size and shape whatever field
/// of view the player picks. Its models are drawn after the world, into
/// the sliver of the depth range nearest the eye (the world is pushed
/// behind it by less than a hundredth of the near plane's distance), so
/// they cover the world and still hide each other's back faces. They are
/// lit by the world's sun, lights and shadows where they are, and cast
/// their shadows into the world.
///
/// # Cost and limits
///
/// `None` (the default) costs nothing. On, it costs one more collection
/// of the held items (only the models on its layers) and their draws at
/// the end of the main view's color pass: no extra render pass, target or
/// copy. The held items are drawn by the main camera only (not window
/// views or target cameras, which never show them). Screen effects that
/// read the depth (ambient occlusion, reflections, depth of field, fog
/// reconstructed from depth) treat them as being at the near plane:
/// ambient occlusion and screen-space reflections leave them out, depth of
/// field focused far away blurs them. Their motion isn't written for
/// temporal antialiasing, which may smear them a little while the view
/// turns fast. Cut-out materials (alpha cut-off) aren't drawn in the pass;
/// solid and see-through ones are.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::{HeldItem, ViewModel};
/// use ponko::render::{MaterialDesc, MeshData};
///
/// Game::new("Cellar", 800, 600).run_headless(1, |ctx| {
///     ctx.scene3d.view_model = Some(ViewModel::default());
///     let wood = ctx.gfx.create_material(MaterialDesc::color(Color::hex(0x8a5a2b)));
///     let torch = ctx.gfx.create_mesh(&MeshData::cube(0.1), wood);
///     // Down and to the right of the eye, half a metre ahead.
///     ctx.world.spawn((Model::new(torch), HeldItem::at(vec3(0.25, -0.2, -0.5))));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ViewModel {
    /// How tall its view is, in radians. Default 55°.
    pub fov_y: f32,
    /// Nothing of a held item closer than this is drawn. Default 0.02.
    pub near: f32,
    /// Nothing of a held item farther than this is drawn. Default 10.
    pub far: f32,
    /// The [`RenderLayers`] the held items are on: the main camera and the
    /// window views leave them out while the view model is on. Each
    /// [`HeldItem`] is put on them. Default layer [`VIEW_MODEL_LAYER`].
    pub layers: RenderLayers,
}

impl Default for ViewModel {
    fn default() -> Self {
        Self {
            fov_y: 55f32.to_radians(),
            near: 0.02,
            far: 10.0,
            layers: RenderLayers::layer(VIEW_MODEL_LAYER),
        }
    }
}

impl ViewModel {
    /// The same, with this field of view (radians).
    ///
    /// ```
    /// let vm = ponko::d3::ViewModel::default().with_fov(50f32.to_radians());
    /// # let _ = vm;
    /// ```
    #[must_use]
    pub fn with_fov(self, fov_y: f32) -> Self {
        Self { fov_y, ..self }
    }

    /// The camera the held items are drawn with: `main`'s place, turn and
    /// window, with this view model's lens and layers.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("Cellar", 800, 600).run_headless(1, |ctx| {
    ///     let vm = ponko::d3::ViewModel::default();
    ///     let eye = vm.camera(&ctx.scene3d.camera);
    ///     assert_eq!(eye.position, ctx.scene3d.camera.position);
    ///     assert_eq!(eye.fov_y, vm.fov_y);
    /// });
    /// ```
    #[must_use]
    pub fn camera(&self, main: &Camera3D) -> Camera3D {
        let mut eye = *main;
        eye.fov_y = self.fov_y.clamp(0.01, 3.1);
        eye.near = self.near.max(1e-4);
        eye.far = self.far.max(eye.near * 2.0);
        eye.layers = self.layers;
        eye.particles = false;
        eye.occlusion_culling = false;
        eye
    }
}

/// A held item: a model that stays at [`offset`](Self::offset) from the
/// eye (the main 3D camera), drawn by the [`ViewModel`] over the world.
///
/// Every frame the engine sets the entity's `Transform` to the camera's
/// times the offset: at the end of [`Phase::Animate`](crate::game::Phase::Animate),
/// right after the camera rigs placed the camera (so later systems can
/// read where it is), and again just before the 3D frame is drawn (after
/// any later camera move). With a [`ViewModel`] it also puts the entity on
/// the view model's layers. Change the offset to sway, bob or recoil it.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::HeldItem;
/// let sword = HeldItem::at(vec3(0.3, -0.25, -0.6)).rotated(Quat::from_rotation_x(0.4));
/// assert_eq!(sword.offset.position, vec3(0.3, -0.25, -0.6));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Component)]
#[serde(default)]
pub struct HeldItem {
    /// Where it is, turned and scaled, in the eye's space: +X right, +Y
    /// up, -Z ahead.
    pub offset: Transform3D,
}

impl Default for HeldItem {
    fn default() -> Self {
        Self::at(Vec3::new(0.25, -0.2, -0.5))
    }
}

impl HeldItem {
    /// Held at `position` in the eye's space (+X right, +Y up, -Z ahead).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let held = ponko::d3::HeldItem::at(vec3(0.0, -0.3, -0.4));
    /// # let _ = held;
    /// ```
    #[must_use]
    pub fn at(position: Vec3) -> Self {
        Self {
            offset: Transform3D::at(position),
        }
    }

    /// The same, turned by `rotation` in the eye's space.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let held = ponko::d3::HeldItem::default().rotated(Quat::from_rotation_y(0.3));
    /// # let _ = held;
    /// ```
    #[must_use]
    pub fn rotated(mut self, rotation: Quat) -> Self {
        self.offset.rotation = rotation;
        self
    }

    /// Where `camera` puts it in the world.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("Cellar", 800, 600).run_headless(1, |ctx| {
    ///     ctx.scene3d.camera.position = vec3(0.0, 1.7, 0.0);
    ///     ctx.scene3d.camera.rotation = Quat::IDENTITY; // looking along -Z
    ///     let at = ponko::d3::HeldItem::at(vec3(0.2, 0.0, -0.5)).placed(&ctx.scene3d.camera);
    ///     assert!(at.position.abs_diff_eq(vec3(0.2, 1.7, -0.5), 1e-6));
    /// });
    /// ```
    #[must_use]
    pub fn placed(&self, camera: &Camera3D) -> Transform3D {
        Transform3D {
            position: camera.position + camera.rotation * self.offset.position,
            rotation: camera.rotation * self.offset.rotation,
            scale: self.offset.scale,
        }
    }
}

/// Puts every [`HeldItem`] at its place in front of `camera` and, with a
/// view model, on its layers. Inserts layers only for items not on them
/// yet (once per item).
pub(crate) fn place_held_items(world: &mut World, camera: &Camera3D, view_model: Option<&ViewModel>) {
    let mut placed = world.query::<(&HeldItem, &mut Transform3D)>();
    if placed.count() == 0 {
        return;
    }
    placed.for_each(|(held, t)| *t = held.placed(camera));
    let Some(vm) = view_model else { return };
    let missing: Vec<Entity> = world
        .query::<(Entity, &HeldItem, Option<&RenderLayers>)>()
        .iter()
        .filter(|(_, _, layers)| layers.copied() != Some(vm.layers))
        .map(|(e, _, _)| e)
        .collect();
    for e in missing {
        world.insert(e, vm.layers);
    }
}

/// `camera` without the view model's layers: what the main camera and the
/// window views draw while a view model is on.
pub(crate) fn without_view_model(camera: &Camera3D, view_model: Option<&ViewModel>) -> Camera3D {
    let mut camera = *camera;
    if let Some(vm) = view_model {
        camera.layers = RenderLayers(camera.layers.0 & !vm.layers.0);
    }
    camera
}

/// What the view-model camera draws this frame, collected into
/// `main.view_model` (kept between frames); dropped without a view model.
pub(crate) fn collect_view_model(
    world: &mut World,
    assets: &super::Assets3D,
    main: &mut super::Frame3D,
    settings: &super::SceneSettings,
    skins: &super::SkinFrame,
) {
    let Some(vm) = settings.view_model.as_ref() else {
        main.view_model = None;
        return;
    };
    let eye = vm.camera(&settings.camera);
    let lighting = super::Lighting {
        shadows: false,
        gi: super::GlobalIllumination {
            enabled: false,
            ..settings.lighting.gi
        },
        ..settings.lighting
    };
    let frame = main.view_model.get_or_insert_with(Default::default);
    frame.size_limit = main.size_limit;
    frame.lod = settings.lod;
    frame.occlusion = None;
    frame.collect(world, assets, skins, &eye, &lighting);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_items_follow_the_camera_and_join_the_view_model_layers() {
        let mut world = World::new();
        let held = world.spawn((HeldItem::at(Vec3::new(0.0, 0.0, -1.0)), Transform3D::default()));
        let loose = world.spawn((Transform3D::at(Vec3::X),));
        let mut camera = Camera3D::new(glam::Vec2::new(800.0, 600.0));
        camera.position = Vec3::new(1.0, 2.0, 3.0);
        camera.rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2); // looking along -X
        place_held_items(&mut world, &camera, None);
        let at = world.get::<Transform3D>(held).unwrap().position;
        assert!(at.abs_diff_eq(Vec3::new(0.0, 2.0, 3.0), 1e-5), "a metre ahead: {at}");
        assert!(
            world.get::<RenderLayers>(held).is_none(),
            "no view model: layers untouched"
        );
        let vm = ViewModel::default();
        place_held_items(&mut world, &camera, Some(&vm));
        assert_eq!(world.get::<RenderLayers>(held), Some(&vm.layers));
        assert!(world.get::<RenderLayers>(loose).is_none());
        let main = without_view_model(&camera, Some(&vm));
        assert!(!main.layers.intersects(vm.layers) && main.layers.intersects(RenderLayers::DEFAULT));
        assert_eq!(without_view_model(&camera, None).layers, RenderLayers::ALL);
    }
}
