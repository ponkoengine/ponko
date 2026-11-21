//! The materials and shader windows (filled in below).

use egui::Ui;

use super::Panels;
use crate::ctx::Ctx;

#[derive(Default)]
pub(crate) struct MaterialState {}

impl Panels {
    pub(super) fn open_materials(&mut self, _: &mut Ctx, _: &str) -> Result<(), String> {
        Err("todo".into())
    }
    pub(super) fn open_shader(&mut self, _: &mut Ctx, _: &str) -> Result<(), String> {
        Err("todo".into())
    }
    pub(super) fn material_window(&mut self, _: &mut Ui, _: &mut Ctx) {}
    pub(super) fn shader_window(&mut self, _: &mut Ui, _: &mut Ctx) {}
}
