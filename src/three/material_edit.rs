//! What the editor's materials and shader windows need from the renderer:
//! each material's description as last made or updated, the materials a
//! glTF file made, edited glTF text applied to those materials in place,
//! and a material shader's source checked and swapped in while it's
//! edited. Editor builds only.

use super::{Material, MaterialDesc, Shader, SharedImage, gltf};
use crate::gfx::Graphics;
use crate::render::ShaderDiagnostic;

impl Graphics {
    /// Remembers what `material` was last made or updated with, for the
    /// editor.
    pub(crate) fn note_material_desc(&mut self, material: Material, desc: &MaterialDesc) {
        self.assets3d.material_descs.insert(material.id, *desc);
    }

    /// What live `material` was last made or updated with.
    pub(crate) fn material_desc(&self, material: Material) -> Option<MaterialDesc> {
        if !self.assets3d.materials_live.contains(material.id) {
            return None;
        }
        self.assets3d.material_descs.get(&material.id).copied()
    }

    /// The live materials drawn with `shader`, and what each has now.
    pub(crate) fn materials_with_shader(&self, shader: Shader) -> Vec<(Material, MaterialDesc)> {
        let mut out: Vec<(Material, MaterialDesc)> = self
            .assets3d
            .material_descs
            .iter()
            .filter(|(id, d)| d.shader == Some(shader) && self.assets3d.materials_live.contains(**id))
            .map(|(&id, d)| (Material { id }, *d))
            .collect();
        out.sort_by_key(|(m, _)| m.id);
        out
    }

    /// The materials glTF file `path` made when it was loaded, in the
    /// file's order; `None` if it wasn't loaded.
    #[cfg(test)]
    pub(crate) fn file_material_handles(&self, path: &str) -> Option<Vec<Material>> {
        let origin = self.asset_sources.find(path).ok()?.origin;
        self.assets3d.file_materials.get(&origin).cloned()
    }

    /// Applies glTF text `bytes` (file `path` as edited, maybe not saved)
    /// to the materials the file made, in place, so every model drawn with
    /// them shows the edit: `values` alone keeps each material's textures
    /// and changes its numbers, colours and modes; otherwise the file's
    /// images are read again too. Nothing happens (`Ok(false)`) if the file
    /// wasn't loaded, or no longer has as many materials.
    ///
    /// # Errors
    /// What's wrong with the file.
    pub(crate) fn apply_file_materials(&mut self, path: &str, bytes: &[u8], values: bool) -> Result<bool, String> {
        let origin = self.asset_sources.find(path).map_err(|e| e.to_string())?.origin;
        let Some(kept) = self.assets3d.file_materials.get(&origin).cloned() else {
            return Ok(false);
        };
        let sources = &self.asset_sources;
        // Images aren't decoded when they stay (they come back empty).
        let loaded = gltf::load(path, bytes, &mut |name| sources.read(name), &|_| values)?;
        if loaded.materials.len() != kept.len() {
            return Ok(false);
        }
        if values {
            let textures = self.assets3d.file_textures.get(&origin).cloned().unwrap_or_default();
            for (material, m) in kept.iter().zip(&loaded.materials) {
                if !self.assets3d.materials_live.contains(material.id) {
                    continue;
                }
                let texture = |i: Option<usize>| i.and_then(|i| textures.get(i).copied());
                let before = self.material_desc(*material).unwrap_or_default();
                let desc = MaterialDesc {
                    color: m.color,
                    texture: texture(m.image),
                    roughness: m.roughness.clamp(0.0, 1.0),
                    metallic: m.metallic.clamp(0.0, 1.0),
                    emissive: m.emissive,
                    normal_map: texture(m.normal_map),
                    normal_strength: m.normal_strength,
                    orm_map: texture(m.orm_map),
                    emissive_map: texture(m.emissive_map),
                    blend: m.blend,
                    double_sided: m.double_sided,
                    ..before
                };
                // Custom values and shader from the extras; a material
                // without them keeps what the game gave it.
                let desc = match m.custom {
                    Some(custom) => MaterialDesc {
                        custom,
                        shader: self.extras_shader(path, m),
                        ..desc
                    },
                    None => desc,
                };
                self.update_material(*material, desc);
            }
            return Ok(true);
        }
        let image_keys = loaded
            .image_sources
            .iter()
            .map(|source| source.as_deref().and_then(|s| SharedImage::of(sources, s)))
            .collect();
        let images = (&loaded.images[..], &loaded.blocks[..]);
        self.file_materials(path, &origin, &loaded.materials, images, image_keys, true);
        Ok(true)
    }

    /// The material shader loaded from `path`, if the game loaded it.
    pub(crate) fn loaded_shader(&self, path: &str) -> Option<Shader> {
        let origin = self.asset_sources.find(path).ok()?.origin;
        let shader = self.assets3d.loaded_shaders.get(&origin).copied()?;
        self.assets3d.shaders_live.contains(shader.id).then_some(shader)
    }

    /// Checks material shader `source` (file `path`) without making
    /// anything: where the first error is, and why.
    pub(crate) fn check_material_shader(path: &str, source: &str) -> Result<(), Box<ShaderDiagnostic>> {
        crate::render::diagnose(
            path,
            super::ShaderKind::Material,
            crate::render::FULL_MATERIAL_VARIANT,
            &crate::render::compose_shader(source),
        )
    }

    /// Swaps `source` in as `shader`'s code (loaded from `path`), as a
    /// changed file would be: on an error the last good code stays in use.
    ///
    /// # Errors
    /// The compiler's message.
    pub(crate) fn replace_shader_source(&mut self, shader: Shader, path: &str, source: &str) -> Result<(), String> {
        self.reload_shader(shader.id, path, source)
    }
}
