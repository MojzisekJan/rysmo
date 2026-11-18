use rysmo_mesh::{DefinitionId, MaterialId, TagId};
use rysmo_model::{Definition, Document, Guide};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Version {
    #[default]
    V2017,
    V2026,
}
#[must_use]
pub struct Export {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct Ids {
    next: u32,
}
impl Ids {
    fn next(&mut self) -> Result<u32, String> {
        self.next = self.next.checked_add(1).ok_or("Too many SKP entities.")?;
        Ok(self.next)
    }
}

pub fn export(doc: &Document, version: Version) -> Result<Export, String> {
    rysmo_model::limits::validate_document(doc)?;
    let mut model = skpwright::Model::default();
    let mut warnings = BTreeSet::new();
    let mut ids = Ids::default();
    let mut materials = BTreeMap::new();
    for (&id, value) in &doc.materials {
        let slot = u16::try_from(model.materials.len() + 1).map_err(|_| "SKP supports at most 65,535 materials.")?;
        if value.color.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v)) {
            return Err("Invalid material colour for SKP export.".into());
        }
        let rgba = value.color.map(|v| (v * 255.).round() as u8);
        let material = if let Some(texture) = value.texture {
            let image = doc.images.get(&texture.image).ok_or("Material refers to a missing image.")?;
            skpwright::Material::Textured {
                name: value.name.clone(),
                texture: Some(format!(
                    "texture-{}.{}",
                    texture.image.0,
                    if image.mime == "image/jpeg" { "jpg" } else { "png" }
                )),
                applied_size_in: Some((texture.size[0] * skpwright::INCH, texture.size[1] * skpwright::INCH)),
                jpeg_quality: None,
                image_bytes: Some(image.bytes.as_ref().clone()),
                avg_rgba: Some(rgba),
                colorized: value.pbr.tint_texture,
                texture_avg_rgba: None,
                colorize_type: 0,
                opacity: f64::from(value.color[3]),
            }
        } else {
            skpwright::Material::Solid {
                name: value.name.clone(),
                rgba,
                opacity: f64::from(value.color[3]),
                colorize_type: 0,
            }
        };
        model.material_links.push((slot, model.materials.len()));
        model.materials.push(material);
        materials.insert(id, slot);
        if value.glow != 0.
            || (rysmo_model::Pbr { tint_texture: false, ..value.pbr }) != Default::default()
            || value.section_hatch.is_some()
        {
            warnings.insert("Advanced material shading and hatching are not exported.".into());
        }
    }
    model.layers.push(skpwright::Layer { name: "Untagged".into(), visible: true, rgba: [255; 4] });
    model.layer_links.push((0, 0));
    model.active_layer = Some(0);
    let mut tags = BTreeMap::new();
    for (&id, value) in &doc.tags {
        let slot = u16::try_from(model.layers.len()).map_err(|_| "Too many tags for SKP.")?;
        model.layer_links.push((slot, model.layers.len()));
        model.layers.push(skpwright::Layer { name: value.name.clone(), visible: value.visible, rgba: [255; 4] });
        tags.insert(id, slot);
    }
    let mut definitions = BTreeMap::new();
    for (&id, value) in &doc.definitions {
        let reference = u32::try_from(model.definitions.len() + 1).map_err(|_| "Too many definitions for SKP.")?;
        definitions.insert(id, reference);
        model.definition_links.push((reference, model.definitions.len()));
        model.definitions.push(skpwright::Definition {
            name: value.name.clone(),
            description: value.component.description.clone(),
            kind: if value.group { skpwright::DefinitionKind::Group } else { skpwright::DefinitionKind::Component },
            map_index: Some(reference as usize),
            behaviour: stored_behaviour(&value.component),
            ..Default::default()
        });
    }
    for (owner, value) in std::iter::once((None, doc.root.as_ref()))
        .chain(doc.definitions.iter().map(|(&id, value)| (Some(id), value.as_ref())))
    {
        let reference = owner.map(|id| definitions[&id] as usize);
        let run = geometry(doc, value, owner, reference, &definitions, &materials, &tags, &mut ids, &mut warnings)?;
        for (instance, placed) in value.instances.values().zip(&run.placed) {
            model.instances.push(skpwright::Instance {
                guid: String::new(),
                pid: placed.pid,
                definition: Some(doc.definitions[&instance.definition].name.clone()),
                defref: placed.defref,
                offset: run.start,
                translation_m: instance.transform.w_axis.truncate().to_array(),
                transform: placed.transform,
                material: Some(placed.material),
                name: Some(instance.name.clone()),
                is_group: Some(placed.is_group),
                hidden: Some(placed.hidden),
                layer: Some(placed.layer),
                slot: None,
                casts_shadows: Some(true),
                receives_shadows: Some(true),
                locked: Some(false),
            });
        }
        for guide in value.guides.values() {
            if let Guide::Line { origin, dir, hidden } = *guide {
                model.guides.push(skpwright::Guide {
                    def_index: reference,
                    pid: ids.next()?,
                    point_m: origin.to_array(),
                    direction: dir.to_array(),
                    bounds: [-1e30, 1e30],
                    layer: 0,
                    material: None,
                    hidden,
                    casts_shadows: true,
                    receives_shadows: true,
                    stipple: 0x3333,
                });
            }
        }
        model.geometry.push(run);
    }
    model.camera = doc.camera.map(super::settings::stored_camera);
    model.rendering = Some(super::settings::stored_style(doc.style));
    let axes = doc.root.axes;
    model.axes = Some(skpwright::Axes {
        origin_m: axes.origin.to_array(),
        x: axes.x.to_array(),
        y: axes.y.to_array(),
        z: axes.x.cross(axes.y).to_array(),
    });
    model.units.length_format = match doc.units {
        rysmo_model::Units::Architectural => 1,
        rysmo_model::Units::Fractional => 3,
        _ => 0,
    };
    model.units.length_unit = super::settings::unit_code(doc.units);
    model.units.length_precision = u32::from(doc.length_precision.unwrap_or(3));
    model.units.area_unit = doc.measurement.area_units.map(super::settings::unit_code);
    model.units.volume_unit = doc.measurement.volume_units.map(super::settings::unit_code);
    model.units.area_precision = Some(u32::from(doc.measurement.area_precision));
    model.units.volume_precision = Some(u32::from(doc.measurement.volume_precision));
    model.units.angle_precision = u32::from(doc.measurement.angle_precision);
    model.units.suppress_units_display = !doc.measurement.show_unit_markers;
    model.units.force_inch_display = doc.measurement.force_zero_inches;
    model.units.length_snap = doc.measurement.length_snap.is_some();
    model.units.length_snap_length = doc.measurement.length_snap.unwrap_or(0.) * doc.units.per_metre();
    model.units.angle_snap = doc.measurement.angle_snap.is_some();
    model.units.snap_angle = doc.measurement.angle_snap.unwrap_or(15.);
    for scene in &doc.scenes {
        model.scenes.push(skpwright::Scene {
            name: scene.name.clone(),
            description: scene.description.clone(),
            in_animation: scene.include_in_animation,
            camera: scene.properties.camera.then(|| super::settings::stored_camera(scene.camera)),
            rendering: scene.properties.style.then(|| super::settings::stored_style(scene.style)),
            saved: skpwright::SceneProperties(
                u32::from(scene.properties.camera) | (u32::from(scene.properties.style) * 2),
            ),
            ..Default::default()
        });
    }
    if !doc.scenes.is_empty() {
        warnings.insert(
            "Scene cameras and basic appearance are exported; per-scene visibility and section overrides are not converted."
                .into(),
        );
    }
    if !doc.sheets.is_empty() || doc.photo_match.is_some() {
        warnings.insert("Drawing sheets and photo matching are not exported to SKP.".into());
    }
    let since = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or(Duration::ZERO);
    let env = skpwright::WriteEnv {
        unix_time: since.as_secs().min(u64::from(u32::MAX)) as u32,
        seed: since.as_nanos() as u64,
    };
    let (bytes, notes) = match version {
        Version::V2017 => model.to_release_with(skpwright::Release::R2017, env),
        Version::V2026 => model.to_archive_with(skpwright::ArchiveTarget::ByContent, env),
    }
    .map_err(|e| format!("SKP export: {e}"))?;
    warnings.extend(notes);
    Ok(Export { bytes, warnings: warnings.into_iter().collect() })
}

#[allow(clippy::too_many_arguments)]
fn geometry(
    doc: &Document,
    definition: &Definition,
    owner: Option<DefinitionId>,
    reference: Option<usize>,
    definitions: &BTreeMap<DefinitionId, u32>,
    materials: &BTreeMap<MaterialId, u16>,
    tags: &BTreeMap<TagId, u16>,
    ids: &mut Ids,
    warnings: &mut BTreeSet<String>,
) -> Result<skpwright::GeometryRun, String> {
    let mesh = doc.effective_mesh_isolated(owner);
    let start = reference.map(|v| v * 2 + 1).unwrap_or(0);
    let mut run = skpwright::GeometryRun {
        def_index: reference,
        start,
        end: if reference.is_some() { start + 1 } else { start },
        ..Default::default()
    };
    let mut vertices = BTreeMap::new();
    for (id, value) in mesh.vertices() {
        let index = u32::try_from(run.mesh.vertices.len()).map_err(|_| "Too many SKP vertices.")?;
        vertices.insert(id, index);
        run.mesh.vertices.push(value.pos.to_array());
        run.mesh.vertex_pids.push(ids.next()?);
    }
    let mut curve_members = BTreeMap::new();
    for (_, edge) in mesh.edges() {
        if let Some(curve) = edge.curve {
            *curve_members.entry(curve).or_insert(0usize) += 1;
        }
    }
    let mut curves = BTreeMap::new();
    for (id, value) in mesh.curves() {
        let index = u32::try_from(run.curves.len()).map_err(|_| "Too many SKP curves.")?;
        curves.insert(id, index);
        let start = value.start.normalize_or_zero() * value.radius;
        let arc = (value.kind != rysmo_mesh::CurveKind::Polyline).then(|| skpwright::Arc {
            center_m: value.centre.to_array(),
            normal: value.normal.normalize().to_array(),
            x_axis_m: start.to_array(),
            y_axis_m: value.normal.normalize().cross(start).to_array(),
            radius_m: value.radius,
            start_rad: 0.,
            end_rad: value.sweep.to_radians(),
        });
        run.curves.push(skpwright::Curve {
            pid: ids.next()?,
            members: u32::try_from(curve_members.get(&id).copied().unwrap_or(0))
                .map_err(|_| "Too many SKP curve edges.")?,
            arc,
        });
    }
    for (_, edge) in mesh.edges() {
        run.mesh.edges.push(skpwright::MeshEdge {
            pid: ids.next()?,
            v0: vertices[&edge.v[0]],
            v1: vertices[&edge.v[1]],
            soft: edge.soft,
            smooth: edge.smooth,
            hidden: edge.hidden,
            material: edge.material.and_then(|id| materials.get(&id).copied()),
            curve: edge.curve.and_then(|id| curves.get(&id).copied()),
            ..Default::default()
        });
    }
    for (id, face) in mesh.faces() {
        let normal = mesh.face_normal(id).ok_or("Cannot export a degenerate face to SKP.")?;
        let mut value = skpwright::MeshFace {
            pid: ids.next()?,
            outer: face.outer.iter().map(|v| vertices[v]).collect(),
            holes: face.holes.iter().map(|ring| ring.iter().rev().map(|v| vertices[v]).collect()).collect(),
            normal: normal.to_array(),
            front_material: face.material.and_then(|id| materials.get(&id).copied()),
            back_material: face.back_material.and_then(|id| materials.get(&id).copied()),
            hidden: face.hidden,
            ..Default::default()
        };
        let matrix = |material: Option<MaterialId>| -> Result<[f64; 9], String> {
            if let Some(texture) = material.and_then(|id| doc.materials.get(&id)).and_then(|m| m.texture) {
                super::mapping::texture_matrix(
                    normal,
                    mesh.pos(face.outer[0]),
                    texture.size,
                    &face.uv.unwrap_or_default(),
                )
                .ok_or_else(|| "Cannot export a singular face texture mapping to SKP.".into())
            } else {
                Ok([1., 0., 0., 0., 1., 0., 0., 0., 1.])
            }
        };
        if face.material.and_then(|id| doc.materials.get(&id)).is_some_and(|m| m.texture.is_some())
            || face.back_material.and_then(|id| doc.materials.get(&id)).is_some_and(|m| m.texture.is_some())
        {
            value.texture = Some(Box::new(skpwright::FaceTexture {
                front: matrix(face.material)?,
                back: matrix(face.back_material)?,
                front_extra: [0.; 3],
                back_extra: [0.; 3],
                front_pins: Vec::new(),
                back_pins: Vec::new(),
                flags: [0, 1],
            }));
        }
        run.mesh.faces.push(value);
    }
    for instance in definition.instances.values() {
        if instance.glued_to.is_some() {
            warnings.insert(
                "Existing glued openings are baked into static mesh holes. Component opening settings are retained."
                    .into(),
            );
        }
        let target = doc.definitions.get(&instance.definition).ok_or("Missing SKP component definition.")?;
        run.placed.push(skpwright::PlacedInstance {
            pid: ids.next()?,
            defref: definitions[&instance.definition],
            transform: super::mapping::stored_transform(instance.transform)
                .ok_or("Invalid instance transform for SKP.")?,
            is_group: target.group,
            material: instance.material.and_then(|id| materials.get(&id).copied()).unwrap_or(0),
            layer: instance.tag.and_then(|id| tags.get(&id).copied()).unwrap_or(0),
            hidden: instance.hidden,
            ..Default::default()
        });
    }
    for guide in definition.guides.values() {
        match *guide {
            Guide::Point { p, origin, hidden } => run.guide_points.push(skpwright::GuidePoint {
                pid: ids.next()?,
                point_m: p.to_array(),
                anchor_m: origin.unwrap_or(p).to_array(),
                hidden,
                ..Default::default()
            }),
            Guide::Line { .. } => {}
        }
    }
    for (&id, plane) in &definition.sections.planes {
        let pid = ids.next()?;
        run.sections.push(skpwright::SectionPlane {
            pid,
            name: plane.name.clone(),
            plane: [plane.normal.x, plane.normal.y, plane.normal.z, -plane.normal.dot(plane.origin) * skpwright::INCH],
            ..Default::default()
        });
        if definition.sections.active == Some(id) {
            run.active_section = Some(pid);
        }
    }
    if !definition.texts.is_empty() || !definition.dimensions.is_empty() {
        warnings.insert("Text and dimension annotations are not yet exported.".into());
    }
    Ok(run)
}

fn stored_behaviour(settings: &rysmo_model::components::ComponentSettings) -> skpwright::Behaviour {
    use rysmo_model::components::GlueMode;
    skpwright::Behaviour {
        glues_to_surface: settings.glue != GlueMode::None,
        cuts_opening: settings.cut_opening,
        glue_plane: match settings.glue {
            GlueMode::None | GlueMode::Any => 0,
            GlueMode::Horizontal => 1,
            GlueMode::Vertical => 2,
            GlueMode::Sloped => 3,
        },
        ..Default::default()
    }
}
