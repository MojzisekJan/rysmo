//! Native SKP interchange through a portable Rust parser and writer.
//! Rysmo documents remain the authoritative editable/save format.
// TODO: Validate exported 2017/2026 files in the target desktop application on
// Windows/macOS with real projects, not only this reader's round-trip tests.
// TODO: Extend conversion for annotations, per-scene overrides and custom data;
// preserve the existing explicit warnings until each feature is implemented.
mod budget;
mod geometry;
mod mapping;
mod settings;
mod write;

use crate::import::{ImportError, Imported, Options};
use rysmo_mesh::{DefinitionId, ImageId, InstanceId, MaterialId, TagId};
use rysmo_model::{Definition, Document, Image, Instance, Material, Tag, Texture};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub use write::{Export, Version, export};

fn failure(message: impl Into<String>) -> ImportError {
    ImportError(message.into())
}

pub fn read(bytes: &[u8], options: Options) -> crate::import::Result<Imported> {
    let scale = options.unit_scale.unwrap_or(1.);
    if !scale.is_finite() || !(1e-9..=1e9).contains(&scale) {
        return Err(failure("Invalid SKP scale."));
    }
    budget::check(bytes).map_err(failure)?;
    let mut source = skpwright::Model::parse(bytes).map_err(|e| failure(format!("SKP: {e}")))?;
    let mut warnings = BTreeSet::new();
    if source.diagnostics.iter().any(skpwright::Diagnostic::is_desync) {
        warnings.insert("The SKP parser recovered incomplete records. Some source data may be missing.".into());
    }
    let total = source
        .geometry
        .iter()
        .try_fold((0usize, 0usize, 0usize, 0usize), |(v, e, f, i), run| {
            Some((
                v.checked_add(run.mesh.vertices.len())?,
                e.checked_add(run.mesh.edges.len())?,
                f.checked_add(run.mesh.faces.len())?,
                i.checked_add(run.placed.len())?,
            ))
        })
        .ok_or_else(|| failure("SKP geometry count overflow."))?;
    if total.0 > 200_000
        || total.1 > 300_000
        || total.2 > 100_000
        || total.3 > 50_000
        || source.definitions.len() > 10_000
    {
        return Err(failure("SKP exceeds the import geometry or component limits."));
    }
    for &(slot, index) in &source.material_links {
        if index >= source.materials.len() || slot == 0 {
            return Err(failure("Invalid SKP material reference table."));
        }
    }
    for &(_, index) in &source.layer_links {
        if index >= source.layers.len() {
            return Err(failure("Invalid SKP tag reference table."));
        }
    }
    let hidden_layers: BTreeSet<_> = source
        .layer_links
        .iter()
        .filter(|&&(_, index)| !source.layers[index].visible && source.active_layer != Some(index))
        .map(|&(slot, _)| slot)
        .collect();
    let mut document = Document::default();
    let material_ids = read_materials(&mut document, &mut source, scale, &mut warnings).map_err(failure)?;
    let materials: BTreeMap<_, _> =
        source.material_links.iter().map(|&(slot, index)| (slot, material_ids[index])).collect();
    let mut tag_ids = Vec::new();
    let untagged = source.layer_links.iter().find(|&&(slot, _)| slot == 0).map(|&(_, index)| index).unwrap_or(0);
    for (index, layer) in source.layers.iter().enumerate() {
        if index == untagged {
            tag_ids.push(None);
            continue;
        }
        let id = TagId(document.reserve_id());
        document
            .tags
            .insert(id, Tag { name: layer.name.clone(), visible: layer.visible || source.active_layer == Some(index) });
        tag_ids.push(Some(id));
    }
    let tags: BTreeMap<_, _> = source
        .layer_links
        .iter()
        .filter(|&&(slot, _)| slot != 0)
        .filter_map(|&(slot, index)| tag_ids[index].map(|id| (slot, id)))
        .collect();
    let mut definitions = BTreeMap::new();
    let mut refs = BTreeMap::new();
    let mut definition_ids = Vec::new();
    for definition in &source.definitions {
        let id = DefinitionId(document.reserve_id());
        let mut value = Definition {
            name: definition.name.clone(),
            group: definition.kind == skpwright::DefinitionKind::Group,
            ..Default::default()
        };
        value.component.description = definition.description.clone();
        use rysmo_model::components::GlueMode;
        value.component.glue = if definition.behaviour.glues_to_surface {
            match definition.behaviour.glue_plane {
                1 => GlueMode::Horizontal,
                2 => GlueMode::Vertical,
                3 => GlueMode::Sloped,
                _ => GlueMode::Any,
            }
        } else {
            GlueMode::None
        };
        value.component.cut_opening = definition.behaviour.cuts_opening;
        if definition.behaviour.cuts_opening {
            warnings.insert(
                "Existing glue attachments are not converted. Opening settings are retained for new placements.".into(),
            );
        }
        if definition.behaviour.always_faces_camera {
            warnings.insert("Camera-facing components are imported as fixed geometry.".into());
        }
        definitions.insert(id, value);
        definition_ids.push(id);
        if let Some(reference) = definition.map_index {
            refs.insert(reference, id);
        }
    }
    for &(reference, index) in &source.definition_links {
        let id = *definition_ids.get(index).ok_or_else(|| failure("Invalid SKP definition reference table."))?;
        if refs.insert(reference as usize, id).is_some_and(|old| old != id) {
            return Err(failure("Ambiguous SKP definition reference."));
        }
    }
    let instance_names: BTreeMap<_, _> = source
        .instances
        .iter()
        .filter(|i| i.pid != 0)
        .filter_map(|i| i.name.as_ref().map(|name| (i.pid, name)))
        .collect();
    for reference in source.geometry.iter().filter_map(|run| run.def_index) {
        refs.entry(reference).or_insert_with(|| {
            let id = DefinitionId(document.reserve_id());
            definitions.insert(id, Definition { name: "Imported component".into(), ..Default::default() });
            id
        });
    }
    let mut guide_owners: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for guide in &source.guides {
        guide_owners.entry(guide.def_index).or_default().push(guide);
    }
    let mut root = None;
    let mut owners = BTreeSet::new();
    for run in &source.geometry {
        let owner = match run.def_index {
            Some(reference) => {
                let id = *refs.entry(reference).or_insert_with(|| {
                    let id = DefinitionId(document.reserve_id());
                    definitions.insert(id, Definition { name: "Imported component".into(), ..Default::default() });
                    id
                });
                Some(id)
            }
            None => None,
        };
        if !owners.insert(owner) {
            return Err(failure("Multiple SKP geometry containers share one owner."));
        }
        let mesh =
            geometry::read(&mut document, &hidden_layers, run, &materials, scale, &mut warnings).map_err(failure)?;
        let mut definition = owner
            .and_then(|id| definitions.remove(&id))
            .unwrap_or_else(|| Definition { name: "Model".into(), ..Default::default() });
        definition.mesh = mesh;
        for placed in &run.placed {
            let target = *refs
                .get(&(placed.defref as usize))
                .ok_or_else(|| failure("SKP instance refers to an unknown component."))?;
            let transform = mapping::transform(placed.transform, scale)
                .ok_or_else(|| failure("SKP instance has an invalid transform."))?;
            let id = InstanceId(document.reserve_id());
            let name = instance_names
                .get(&placed.pid)
                .map(|name| (*name).clone())
                .unwrap_or_else(|| definitions.get(&target).map(|d| d.name.clone()).unwrap_or_default());
            definition.instances.insert(
                id,
                Instance {
                    definition: target,
                    transform,
                    name,
                    tag: tags.get(&placed.layer).copied(),
                    material: materials.get(&placed.material).copied(),
                    hidden: placed.hidden,
                    glued_to: None,
                    attributes: Default::default(),
                },
            );
            if placed.locked {
                warnings.insert("Imported locked instances are editable in Rysmo.".into());
            }
        }
        settings::read_run(
            &mut document,
            &mut definition,
            guide_owners.get(&run.def_index).map(Vec::as_slice).unwrap_or(&[]),
            &hidden_layers,
            run,
            scale,
            &mut warnings,
        )
        .map_err(failure)?;
        if let Some(id) = owner {
            definitions.insert(id, definition);
        } else {
            root = Some(definition);
        }
    }
    document.root = Arc::new(root.ok_or_else(|| failure("SKP has no model geometry container."))?);
    document.definitions = definitions.into_iter().map(|(id, definition)| (id, Arc::new(definition))).collect();
    settings::read_document(&mut document, &source, scale, &mut warnings).map_err(failure)?;
    crate::import::validate_expansion(&document)?;
    document.fix_next_id().map_err(|e| failure(e.to_string()))?;
    if !source.features.is_empty() {
        warnings
            .insert("Advanced SKP rendering, procedural and environment data may not be represented in Rysmo.".into());
    }
    if !source.dictionaries.is_empty() {
        warnings.insert("Custom attribute dictionaries and dynamic behaviours are not imported.".into());
    }
    if !source.watermarks.is_empty() {
        warnings.insert("Watermarks are not imported.".into());
    }
    Ok(Imported { document, warnings: warnings.into_iter().collect() })
}

fn read_materials(
    doc: &mut Document,
    source: &mut skpwright::Model,
    scale: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<Vec<MaterialId>, String> {
    let mut ids = Vec::new();
    let mut shared_images = std::collections::HashMap::<(usize, u32), Vec<ImageId>>::new();
    for value in &mut source.materials {
        let (name, rgba, opacity) = match value {
            skpwright::Material::Solid { name, rgba, opacity, .. } => (name.clone(), *rgba, *opacity),
            skpwright::Material::Textured { name, avg_rgba, opacity, .. } => {
                (name.clone(), avg_rgba.unwrap_or([255; 4]), *opacity)
            }
        };
        if !opacity.is_finite() || !(0. ..=1.).contains(&opacity) {
            return Err("Invalid SKP material opacity.".into());
        }
        let mut material = Material {
            name: name.clone(),
            color: [rgba[0] as f32 / 255., rgba[1] as f32 / 255., rgba[2] as f32 / 255., opacity as f32],
            texture: None,
            glow: 0.,
            pbr: Default::default(),
            section_hatch: None,
        };
        if let skpwright::Material::Textured { texture, applied_size_in, image_bytes, colorized, .. } = value {
            if let Some(bytes) = image_bytes.take() {
                let format = image::guess_format(&bytes).map_err(|e| format!("SKP texture: {e}"))?;
                let mime = match format {
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    _ => return Err("Unsupported SKP texture encoding.".into()),
                };
                let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
                    .into_dimensions()
                    .map_err(|e| format!("SKP texture: {e}"))?;
                if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_777_216 {
                    return Err("SKP textures are limited to 16 million pixels each.".into());
                }
                let (w, h) = applied_size_in.unwrap_or((1., 1.));
                let size = [w / skpwright::INCH * scale, h / skpwright::INCH * scale];
                if size.iter().any(|n| !n.is_finite() || !(1e-9..=1e9).contains(n)) {
                    return Err("Invalid SKP texture dimensions.".into());
                }
                let key = (bytes.len(), crc32fast::hash(&bytes));
                let known = shared_images
                    .get(&key)
                    .and_then(|ids| ids.iter().copied().find(|id| doc.images[id].bytes.as_slice() == bytes.as_slice()));
                let id = if let Some(id) = known {
                    id
                } else {
                    let id = ImageId(doc.reserve_id());
                    doc.images.insert(
                        id,
                        Image { name: texture.clone().unwrap_or(name), mime: mime.into(), bytes: Arc::new(bytes) },
                    );
                    shared_images.entry(key).or_default().push(id);
                    id
                };
                material.texture = Some(Texture { image: id, size });
                material.pbr.tint_texture = *colorized;
            } else {
                warnings.insert("A textured material has no embedded image and was imported as a solid colour.".into());
            }
        }
        let id = MaterialId(doc.reserve_id());
        doc.materials.insert(id, material);
        ids.push(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
