//! The terrain window (filled in below).

use egui::Ui;
use glam::Vec2;

use super::Panels;
use crate::ctx::Ctx;

#[derive(Default)]
pub(crate) struct TerrainState {}

pub(crate) fn is_heightmap_file(_: &str) -> bool {
    false
}

impl Panels {
    pub(super) fn open_terrain(&mut self, _: &mut Ctx, _: &str) -> Result<(), String> {
        Err("todo".into())
    }
    pub(super) fn terrain_window(&mut self, _: &mut Ui, _: &mut Ctx) {}
    pub(super) fn terrain_brush(&mut self, _: &egui::Ui, _: &mut Ctx, _: Vec2) -> bool {
        false
    }
}
