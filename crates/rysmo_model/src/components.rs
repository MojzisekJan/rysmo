//! Component metadata and insertion coordinates, separate from drawing axes.
use rysmo_math::{DMat4, DrawingAxes};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComponentSettings {
    pub description: String,
    pub insertion_axes: DrawingAxes,
    pub glue: GlueMode,
    pub cut_opening: bool,
    #[serde(default)]
    pub connections: crate::component_data::Connections,
    #[serde(default)]
    pub attributes: crate::component_data::Attributes,
}
impl ComponentSettings {
    pub fn frame(&self) -> DMat4 {
        DMat4::from_translation(self.insertion_axes.origin) * self.insertion_axes.basis()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element};
    use rysmo_math::{DVec3, OrbitCamera};
    fn component() -> (Editor, crate::InstanceId, crate::DefinitionId) {
        let mut e = Editor::new();
        let elements = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = e.apply(Command::MakeComponent { elements, name: Some("Panel".into()) }).unwrap();
        let instance =
            out.created.into_iter().find_map(|e| if let Element::Instance(id) = e { Some(id) } else { None }).unwrap();
        let definition = e.doc().root.instances[&instance].definition;
        (e, instance, definition)
    }
    #[test]
    fn insertion_axes_change_new_placements_without_moving_existing_instances() {
        let (mut e, instance, definition) = component();
        let old = e.doc().root.instances[&instance].clone();
        let settings = ComponentSettings {
            connections: Default::default(),
            attributes: Default::default(),
            glue: GlueMode::None,
            cut_opening: false,
            description: "A panel".into(),
            insertion_axes: DrawingAxes::new(DVec3::new(0.5, 0.5, 0.), DVec3::Y, -DVec3::X).unwrap(),
        };
        e.apply(Command::SetComponentProperties { definition, name: "Panel".into(), settings: settings.clone() })
            .unwrap();
        assert_eq!(e.doc().root.instances[&instance], old);
        let target = DVec3::new(5., 3., 2.);
        let out = e.apply(Command::PlaceInstance { definition, origin: target, rotation: 90., scale: 2. }).unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let placed = &e.doc().root.instances[&id];
        assert!(placed.transform.transform_point3(settings.insertion_axes.origin).distance(target) < 1e-9);
        assert!(placed.transform.transform_vector3(settings.insertion_axes.x).distance(DVec3::Y * 2.) < 1e-9);
        let after = e.doc().clone();
        e.undo();
        assert_eq!(e.doc().root.instances.len(), 1);
        e.redo();
        assert_eq!(e.doc(), &after);
        e.apply(Command::MakeUnique { instance: id }).unwrap();
        assert_eq!(e.doc().definitions[&e.doc().root.instances[&id].definition].component, settings);
    }
    #[test]
    fn hidden_instance_is_not_drawn_and_scene_restore_recalls_visibility() {
        let (mut e, instance, _) = component();
        let before = e.doc().clone();
        e.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: true }).unwrap();
        assert_eq!(e.doc().occurrences(&[]).len(), 1);
        assert!(e.doc().definition_bounds(None).is_none());
        let scene = crate::scenes::Scene::capture(e.doc(), OrbitCamera::default(), "Hidden".into());
        e.apply(Command::AddScene { scene }).unwrap();
        let id = e.doc().scenes[0].id;
        e.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: false }).unwrap();
        assert_eq!(e.doc().occurrences(&[]).len(), 2);
        e.apply(Command::RestoreScene { id }).unwrap();
        assert!(e.doc().root.instances[&instance].hidden);
        e.undo();
        assert!(!e.doc().root.instances[&instance].hidden);
        let mut reset = Editor::with_document(before);
        reset.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: true }).unwrap();
        reset.undo();
        assert_eq!(reset.doc().occurrences(&[]).len(), 2);
    }
    #[test]
    fn unused_components_remain_in_browser_until_explicitly_deleted() {
        let (mut e, instance, definition) = component();
        let before = e.doc().clone();
        assert!(e.apply(Command::DeleteDefinition { definition }).is_err());
        assert_eq!(e.doc(), &before);
        e.apply(Command::Erase { elements: vec![Element::Instance(instance)] }).unwrap();
        assert!(e.doc().definitions.contains_key(&definition));
        e.apply(Command::PlaceInstance { definition, origin: DVec3::ZERO, rotation: 0., scale: 1. }).unwrap();
        let id = *e.doc().root.instances.keys().next().unwrap();
        e.apply(Command::Erase { elements: vec![Element::Instance(id)] }).unwrap();
        e.apply(Command::DeleteDefinition { definition }).unwrap();
        assert!(!e.doc().definitions.contains_key(&definition));
        e.undo();
        assert!(e.doc().definitions.contains_key(&definition));
    }
    #[test]
    fn invalid_properties_and_placements_are_atomic() {
        let (mut e, _, definition) = component();
        let before = e.doc().clone();
        let mut settings = ComponentSettings::default();
        settings.insertion_axes.x = DVec3::ZERO;
        assert!(e.apply(Command::SetComponentProperties { definition, name: "Panel".into(), settings }).is_err());
        assert!(
            e.apply(Command::PlaceInstance { definition, origin: DVec3::splat(f64::NAN), rotation: 0., scale: 1. })
                .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
}

#[cfg(test)]
mod context_tests {
    use crate::{Command, Editor, Element};
    use rysmo_math::DVec3;
    #[test]
    fn hiding_the_active_context_tag_closes_it_and_invisible_contexts_cannot_be_entered() {
        let mut e = Editor::new();
        let elements = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = e.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let tag = e.apply(Command::AddTag { name: "Layer".into() }).unwrap().tag.unwrap();
        e.apply(Command::AssignTag { instances: vec![id], tag: Some(tag) }).unwrap();
        e.enter(id).unwrap();
        e.apply(Command::SetTag { tag, name: None, visible: Some(false) }).unwrap();
        assert!(e.context().is_empty());
        assert!(e.enter(id).is_err());
        e.undo();
        e.enter(id).unwrap();
        assert_eq!(e.context(), &[id]);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlueMode {
    #[default]
    None,
    Any,
    Horizontal,
    Vertical,
    Sloped,
}
impl GlueMode {
    pub fn accepts(self, normal: rysmo_math::DVec3, up: rysmo_math::DVec3) -> bool {
        let d = normal.normalize().dot(up.normalize()).abs();
        match self {
            Self::None => false,
            Self::Any => true,
            Self::Horizontal => d > 1. - 1e-6,
            Self::Vertical => d < 1e-6,
            Self::Sloped => (1e-6..=1. - 1e-6).contains(&d),
        }
    }
}

use crate::{CommandError, DefinitionId, Document, FaceId, InstanceId};
use rysmo_math::{DVec3, Plane};
use std::{borrow::Cow, collections::BTreeMap};

fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}

/// The closed outer boundary in the component's insertion plane.
pub fn opening_outline(definition: &crate::Definition) -> Result<Vec<DVec3>, CommandError> {
    let axes = definition.component.insertion_axes;
    let plane = Plane::new(axes.z(), axes.origin);
    let mesh = &definition.mesh;
    if let Some((id, _)) = mesh
        .faces()
        .filter(|(_, f)| f.outer.iter().all(|v| plane.distance(mesh.pos(*v)).abs() < 1e-7))
        .max_by(|(a, _), (b, _)| mesh.face_area(*a).total_cmp(&mesh.face_area(*b)))
    {
        return Ok(mesh.face(id).unwrap().outer.iter().map(|v| mesh.pos(*v)).collect());
    }
    let mut adjacent: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (_, edge) in mesh.edges().filter(|(_, e)| e.v.iter().all(|v| plane.distance(mesh.pos(*v)).abs() < 1e-7)) {
        adjacent.entry(edge.v[0]).or_default().push(edge.v[1]);
        adjacent.entry(edge.v[1]).or_default().push(edge.v[0]);
    }
    let Some(start) = adjacent.keys().next().copied() else {
        return Err(invalid("Cut Opening needs a closed outline in the insertion plane."));
    };
    let mut previous = None;
    let mut current = start;
    let mut ring = Vec::new();
    loop {
        let neighbours = &adjacent[&current];
        if neighbours.len() != 2 {
            return Err(invalid("The insertion outline must be a simple closed loop."));
        }
        ring.push(mesh.pos(current));
        let next = neighbours.iter().copied().find(|v| Some(*v) != previous).unwrap();
        previous = Some(current);
        current = next;
        if current == start {
            break;
        }
        if ring.len() > adjacent.len() {
            return Err(invalid("The insertion outline must be closed."));
        }
    }
    if ring.len() < 3 {
        return Err(invalid("The insertion outline needs at least three vertices."));
    }
    Ok(ring)
}

pub fn glued_transform(
    doc: &Document,
    owner: Option<DefinitionId>,
    definition: DefinitionId,
    face: FaceId,
    origin: DVec3,
    rotation: f64,
    scale: f64,
) -> Result<DMat4, CommandError> {
    let host = doc.def(owner).ok_or_else(|| invalid("The editing context is missing."))?;
    let component = doc.def(Some(definition)).ok_or_else(|| invalid("The component is missing."))?;
    let plane = host.mesh.face_plane(face).ok_or_else(|| invalid("Pick a face in the current editing context."))?;
    if !origin.is_finite() || !rotation.is_finite() || !scale.is_finite() || scale.abs() < 1e-9 {
        return Err(invalid("Placement values must be finite and scale must not be zero."));
    }
    if !component.component.glue.accepts(plane.normal, host.axes.z()) {
        return Err(invalid("This face does not match the component's glue orientation."));
    }
    if plane.distance(origin).abs() > 1e-7 {
        return Err(invalid("The insertion point must lie on the host plane."));
    }
    let origin = plane.project(origin);
    if !host.mesh.face_contains(face, origin) {
        return Err(invalid("The insertion point must lie on the host face."));
    }
    let (x, y) = plane.basis();
    let frame = DMat4::from_cols(x.extend(0.), y.extend(0.), plane.normal.extend(0.), origin.extend(1.));
    let matrix = frame
        * DMat4::from_rotation_z(rotation.to_radians())
        * DMat4::from_scale(DVec3::splat(scale))
        * component.component.frame().inverse();
    if component.component.cut_opening {
        let outline: Vec<_> = opening_outline(component)?.into_iter().map(|p| matrix.transform_point3(p)).collect();
        doc.effective_mesh(owner).with_surface_opening(face, &outline).map_err(|e| invalid(&e.to_string()))?;
    }
    Ok(matrix)
}

impl Document {
    /// Procedural single-face component cuts, shared by display, picking, navigation
    /// and export. The editable host stays intact, so removing a component heals it.
    pub fn effective_mesh(&self, owner: Option<DefinitionId>) -> Cow<'_, rysmo_mesh::Mesh> {
        let definition = self.def(owner).expect("known definition");
        let mut result = Cow::Borrowed(&definition.mesh);
        for instance in definition.instances.values() {
            let Some(face) = instance.glued_to else { continue };
            if instance.hidden || !self.tag_visible(instance.tag) {
                continue;
            }
            let Some(component) = self.def(Some(instance.definition)).filter(|d| d.component.cut_opening) else {
                continue;
            };
            let Ok(outline) = opening_outline(component) else { continue };
            let points: Vec<_> = outline.into_iter().map(|p| instance.transform.transform_point3(p)).collect();
            if let Ok(mesh) = result.with_surface_opening(face, &points) {
                result = Cow::Owned(mesh);
            }
        }
        result
    }
}

pub(crate) struct GlueBefore {
    face: FaceId,
    centre: DVec3,
    normal: DVec3,
    transform: DMat4,
}
pub(crate) fn capture_glue(doc: &Document) -> BTreeMap<InstanceId, GlueBefore> {
    let mut result = BTreeMap::new();
    for definition in std::iter::once(&doc.root).chain(doc.definitions.values()) {
        for (id, instance) in &definition.instances {
            let Some(face) = instance.glued_to else { continue };
            let Some(plane) = definition.mesh.face_plane(face) else { continue };
            let f = definition.mesh.face(face).unwrap();
            let centre = f.outer.iter().map(|v| definition.mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64;
            result.insert(*id, GlueBefore { face, centre, normal: plane.normal, transform: instance.transform });
        }
    }
    result
}
pub(crate) fn refresh_glue(doc: &mut Document, before: &BTreeMap<InstanceId, GlueBefore>) {
    let mut updates = Vec::new();
    for (owner, definition) in
        std::iter::once((None, &doc.root)).chain(doc.definitions.iter().map(|(id, d)| (Some(*id), d)))
    {
        for (id, instance) in &definition.instances {
            let Some(host) = instance.glued_to else { continue };
            let Some(component) = doc.def(Some(instance.definition)) else { continue };
            let mut transform = instance.transform;
            if let Some(old) = before.get(id).filter(|old| old.face == host && old.transform == transform)
                && let Some(plane) = definition.mesh.face_plane(host)
            {
                let face = definition.mesh.face(host).unwrap();
                let centre =
                    face.outer.iter().map(|v| definition.mesh.pos(*v)).sum::<DVec3>() / face.outer.len() as f64;
                let rotation = rysmo_math::DQuat::from_rotation_arc(old.normal, plane.normal);
                if centre.distance(old.centre) > 1e-9 || plane.normal.dot(old.normal) < 1. - 1e-12 {
                    transform = DMat4::from_translation(centre)
                        * DMat4::from_quat(rotation)
                        * DMat4::from_translation(-old.centre)
                        * transform;
                }
            }
            let origin = transform.transform_point3(component.component.insertion_axes.origin);
            let normal = transform
                .inverse()
                .transpose()
                .transform_vector3(component.component.insertion_axes.z())
                .normalize_or_zero();
            let fits = |face: FaceId| {
                definition.mesh.face_plane(face).is_some_and(|plane| {
                    plane.distance(origin).abs() < 1e-7
                        && plane.normal.dot(normal).abs() > 1. - 1e-6
                        && definition.mesh.face_contains(face, origin)
                })
            };
            let target = if component.component.glue == GlueMode::None {
                None
            } else if fits(host) {
                Some(host)
            } else {
                definition.mesh.faces().map(|(id, _)| id).find(|id| fits(*id)).or_else(|| {
                    definition
                        .mesh
                        .face_plane(host)
                        .filter(|p| p.distance(origin).abs() < 1e-7 && p.normal.dot(normal).abs() > 1. - 1e-6)
                        .map(|_| host)
                })
            };
            updates.push((owner, *id, transform, target));
        }
    }
    for (owner, id, transform, target) in updates {
        let instance = doc.def_mut(owner).unwrap().instances.get_mut(&id).unwrap();
        instance.transform = transform;
        instance.glued_to = target;
    }
}

pub fn validate_openings(doc: &Document) -> Result<(), CommandError> {
    for definition in std::iter::once(&doc.root).chain(doc.definitions.values()) {
        let mut mesh = Cow::Borrowed(&definition.mesh);
        for instance in definition.instances.values().filter(|i| !i.hidden && doc.tag_visible(i.tag)) {
            let Some(face) = instance.glued_to else { continue };
            let Some(component) = doc.def(Some(instance.definition)).filter(|d| d.component.cut_opening) else {
                continue;
            };
            let points: Vec<_> =
                opening_outline(component)?.into_iter().map(|p| instance.transform.transform_point3(p)).collect();
            mesh = Cow::Owned(mesh.with_surface_opening(face, &points).map_err(|e| invalid(&e.to_string()))?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod opening_tests {
    use super::*;
    use crate::{Command, Editor, Element};
    use rysmo_math::Ray;
    pub(super) fn fixture() -> (Editor, DefinitionId, FaceId) {
        let mut e = Editor::new();
        let geometry = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap()
            .created;
        let made = e.apply(Command::MakeComponent { elements: geometry, name: Some("Window".into()) }).unwrap();
        let Element::Instance(original) = made.created[0] else { panic!() };
        let definition = e.doc().root.instances[&original].definition;
        e.enter(original).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::Erase { elements: vec![Element::Face(face)] }).unwrap();
        e.exit();
        e.apply(Command::Erase { elements: vec![Element::Instance(original)] }).unwrap();
        let settings = ComponentSettings { glue: GlueMode::Any, cut_opening: true, ..Default::default() };
        e.apply(Command::SetComponentProperties { definition, name: "Window".into(), settings }).unwrap();
        let made = e
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 0.),
                u: DVec3::X * 10.,
                v: DVec3::Y * 10.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = made.created.iter().find_map(|e| if let Element::Face(id) = e { Some(*id) } else { None }).unwrap();
        (e, definition, face)
    }
    pub(super) fn place(e: &mut Editor, definition: DefinitionId, face: FaceId) -> InstanceId {
        let made = e
            .apply(Command::PlaceGluedInstance {
                definition,
                face,
                origin: DVec3::new(5., 5., 0.),
                rotation: 0.,
                scale: 1.,
            })
            .unwrap();
        let Element::Instance(id) = made.created[0] else { panic!() };
        id
    }
    #[test]
    fn opening_is_shared_by_surface_picking_and_heals_after_move_delete_and_undo() {
        let (mut e, definition, face) = fixture();
        let instance = place(&mut e, definition, face);
        let centre = e.doc().root.instances[&instance].transform.transform_point3(DVec3::new(0.5, 0.5, 0.));
        let surface = e.doc().effective_mesh(None);
        surface.validate().unwrap();
        assert!(!surface.face_contains(face, centre));
        assert!((surface.face_area(face) - 99.).abs() < 1e-8);
        assert!(e.doc().root.mesh.face_contains(face, centre));
        assert!(e.doc().ray_surface(Ray::new(centre + DVec3::Z, -DVec3::Z)).is_none());
        e.apply(Command::Move { elements: vec![Element::Instance(instance)], offset: DVec3::X * 2. }).unwrap();
        assert!(e.doc().effective_mesh(None).face_contains(face, centre));
        assert!(!e.doc().effective_mesh(None).face_contains(face, centre + DVec3::X * 2.));
        e.apply(Command::Erase { elements: vec![Element::Instance(instance)] }).unwrap();
        assert!((e.doc().effective_mesh(None).face_area(face) - 100.).abs() < 1e-8);
        e.undo();
        assert!((e.doc().effective_mesh(None).face_area(face) - 99.).abs() < 1e-8);
        e.redo();
        assert!((e.doc().effective_mesh(None).face_area(face) - 100.).abs() < 1e-8);
    }
    #[test]
    fn host_motion_moves_glued_component_and_grouping_remaps_host() {
        let (mut e, definition, face) = fixture();
        let instance = place(&mut e, definition, face);
        let original = e.doc().root.instances[&instance].transform;
        e.apply(Command::Move { elements: vec![Element::Face(face)], offset: DVec3::Z * 2. }).unwrap();
        assert!(
            e.doc().root.instances[&instance]
                .transform
                .transform_point3(DVec3::ZERO)
                .distance(original.transform_point3(DVec3::ZERO) + DVec3::Z * 2.)
                < 1e-8
        );
        let grouped =
            e.apply(Command::MakeGroup { elements: vec![Element::Face(face), Element::Instance(instance)] }).unwrap();
        let Element::Instance(group) = grouped.created[0] else { panic!() };
        let group_def = e.doc().root.instances[&group].definition;
        let owner = &e.doc().definitions[&group_def];
        let window = owner.instances.values().next().unwrap();
        assert!(owner.mesh.face(window.glued_to.unwrap()).is_some());
        assert!(
            (e.doc()
                .effective_mesh(Some(group_def))
                .faces()
                .map(|(id, _)| e.doc().effective_mesh(Some(group_def)).face_area(id))
                .sum::<f64>()
                - 99.)
                .abs()
                < 1e-8
        );
        e.apply(Command::Explode { instance: group }).unwrap();
        let window = e.doc().root.instances.values().next().unwrap();
        assert!(e.mesh().face(window.glued_to.unwrap()).is_some());
    }
    #[test]
    fn invalid_overlapping_openings_roll_back_and_unglue_restores_surface() {
        let (mut e, definition, face) = fixture();
        let instance = place(&mut e, definition, face);
        let before = e.doc().clone();
        let depth = e.undo_depth();
        assert!(
            e.apply(Command::PlaceGluedInstance {
                definition,
                face,
                origin: DVec3::new(5., 5., 0.),
                rotation: 0.,
                scale: 1.
            })
            .is_err()
        );
        assert!(
            e.apply(Command::Move { elements: vec![Element::Instance(instance)], offset: DVec3::X * 20. }).is_err()
        );
        assert_eq!(e.doc(), &before);
        assert_eq!(e.undo_depth(), depth);
        e.apply(Command::UnglueInstances { instances: vec![instance] }).unwrap();
        assert_eq!(e.doc().root.instances[&instance].glued_to, None);
        assert!((e.doc().effective_mesh(None).face_area(face) - 100.).abs() < 1e-8);
        e.undo();
        assert!((e.doc().effective_mesh(None).face_area(face) - 99.).abs() < 1e-8);
    }
    #[test]
    fn hidden_component_restores_opening_and_orientation_restrictions_are_enforced() {
        let (mut e, definition, face) = fixture();
        let instance = place(&mut e, definition, face);
        e.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: true }).unwrap();
        assert!((e.doc().effective_mesh(None).face_area(face) - 100.).abs() < 1e-8);
        e.undo();
        let mut settings = e.doc().definitions[&definition].component.clone();
        settings.glue = GlueMode::Vertical;
        e.apply(Command::SetComponentProperties { definition, name: "Window".into(), settings }).unwrap();
        assert!(
            e.apply(Command::PlaceGluedInstance {
                definition,
                face,
                origin: DVec3::new(8., 8., 0.),
                rotation: 0.,
                scale: 1.
            })
            .is_err()
        );
    }
    #[test]
    fn vertical_wall_cut_survives_transformed_group_navigation() {
        let (mut e, definition, face) = fixture();
        e.apply(Command::Rotate {
            elements: vec![Element::Face(face)],
            centre: DVec3::ZERO,
            axis: DVec3::X,
            angle: 90.,
            copies: 0,
        })
        .unwrap();
        let made = e
            .apply(Command::PlaceGluedInstance {
                definition,
                face,
                origin: DVec3::new(5., 0., 5.),
                rotation: 15.,
                scale: 1.,
            })
            .unwrap();
        let Element::Instance(window) = made.created[0] else { panic!() };
        let plane = e.mesh().face_plane(face).unwrap();
        let transform = e.doc().root.instances[&window].transform;
        assert!(transform.transform_vector3(DVec3::Z).dot(plane.normal).abs() > 1. - 1e-8);
        let centre = transform.transform_point3(DVec3::new(0.5, 0.5, 0.));
        assert!(!e.doc().effective_mesh(None).face_contains(face, centre));
        let grouped =
            e.apply(Command::MakeGroup { elements: vec![Element::Face(face), Element::Instance(window)] }).unwrap();
        let Element::Instance(group) = grouped.created[0] else { panic!() };
        e.apply(Command::Rotate {
            elements: vec![Element::Instance(group)],
            centre: DVec3::ZERO,
            axis: DVec3::Z,
            angle: 37.,
            copies: 0,
        })
        .unwrap();
        e.apply(Command::Scale {
            elements: vec![Element::Instance(group)],
            origin: DVec3::ZERO,
            factors: DVec3::new(2., 3., 1.),
        })
        .unwrap();
        let world = e.doc().root.instances[&group].transform;
        let definition = e.doc().root.instances[&group].definition;
        let child = e.doc().definitions[&definition].instances.values().next().unwrap();
        let point = (world * child.transform).transform_point3(DVec3::new(0.5, 0.5, 0.));
        let normal = world.inverse().transpose().transform_vector3(plane.normal).normalize();
        assert!(e.doc().ray_surface(rysmo_math::Ray::new(point + normal * 2., -normal)).is_none());
    }
}

pub(crate) fn bake_opening(
    doc: &mut Document,
    owner: Option<DefinitionId>,
    instance: InstanceId,
) -> Result<(), CommandError> {
    let item =
        doc.def(owner).and_then(|d| d.instances.get(&instance)).ok_or_else(|| invalid("The instance is missing."))?;
    let Some(face) = item.glued_to else { return Ok(()) };
    if item.hidden || !doc.tag_visible(item.tag) {
        return Ok(());
    }
    let source = doc.def(Some(item.definition)).ok_or_else(|| invalid("The component is missing."))?;
    if !source.component.cut_opening {
        return Ok(());
    }
    let points: Vec<_> = opening_outline(source)?.into_iter().map(|p| item.transform.transform_point3(p)).collect();
    let mut mesh = doc.def(owner).unwrap().mesh.clone();
    mesh.ensure_next_id(doc.next_id);
    let mut mesh = mesh.with_surface_opening(face, &points).map_err(|e| invalid(&e.to_string()))?;
    mesh.materialize_opening_edges();
    doc.next_id = mesh.next_id();
    doc.def_mut(owner).unwrap().mesh = mesh;
    Ok(())
}

#[cfg(test)]
mod explode_opening_tests {
    use crate::Command;
    #[test]
    fn exploding_a_cutting_wire_component_bakes_its_opening_and_undo_restores_the_link() {
        let (mut e, definition, face) = super::opening_tests::fixture();
        let instance = super::opening_tests::place(&mut e, definition, face);
        let before = e.doc().clone();
        e.apply(Command::Explode { instance }).unwrap();
        assert!(e.doc().root.instances.is_empty());
        assert!((e.mesh().face_area(face) - 99.).abs() < 1e-8);
        assert!(e.mesh().edges().all(|(_, edge)| !edge.procedural));
        e.mesh().validate().unwrap();
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert!((e.mesh().face_area(face) - 99.).abs() < 1e-8);
    }
}
