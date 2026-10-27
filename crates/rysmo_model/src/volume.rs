//! Volume reporting: a topology-only fast path for single shells, and full solid
//! validation with nesting-aware volume for compound shells and cavities.
use crate::{Document, Instance};
use rysmo_math::DMat4;
use rysmo_mesh::{FaceId, Mesh, VertexId, triangulate_face};
use std::collections::{BTreeMap, BTreeSet};

impl Document {
    /// Volume in world cubic metres, including enclosing-instance scale and mirrors.
    /// Nested instances are rejected. Multiple shells additionally undergo geometric
    /// validation and nesting checks so cavity volume is subtracted correctly.
    pub fn instance_volume(&self, instance: &Instance, parent: DMat4) -> Result<f64, &'static str> {
        let def = self.definitions.get(&instance.definition).ok_or("Missing definition")?;
        if !def.instances.is_empty() {
            return Err("Contains nested groups or components");
        }
        let scale = (parent * instance.transform).determinant().abs();
        if !scale.is_finite() || scale == 0. {
            return Err("Degenerate instance transform");
        }
        let local = mesh_volume(&def.mesh)?;
        let volume = local * scale;
        if !volume.is_finite() || volume <= 0. {
            return Err("Degenerate volume");
        }
        Ok(volume)
    }
}

/// Local cubic metres from a closed mesh; compound shells use nesting validation.
/// Single-shell reporting is topology-based, as in the Properties inspector.
pub fn mesh_volume(mesh: &Mesh) -> Result<f64, &'static str> {
    match shell_volume(mesh) {
        Ok(volume) => Ok(volume),
        Err("Multiple shells: volume is not summed") => crate::solid_validation::validate(mesh, false)
            .map(f64::abs)
            .map_err(|_| "Invalid or over-complex compound solid"),
        Err(error) => Err(error),
    }
}

fn shell_volume(mesh: &Mesh) -> Result<f64, &'static str> {
    type Step = (FaceId, VertexId, VertexId);
    let mut edges: BTreeMap<(VertexId, VertexId), Vec<Step>> = BTreeMap::new();
    let mut vertex_faces: BTreeMap<VertexId, BTreeSet<FaceId>> = BTreeMap::new();
    for (id, face) in mesh.faces() {
        for (a, b) in face.loop_edges() {
            edges.entry((a.min(b), a.max(b))).or_default().push((id, a, b));
            vertex_faces.entry(a).or_default().insert(id);
        }
    }
    if edges.is_empty() {
        return Err("No closed surface");
    }
    for (_, edge) in mesh.edges() {
        let [a, b] = edge.v;
        if edges.get(&(a.min(b), a.max(b))).is_none_or(|f| f.len() != 2) {
            return Err("Open edges, stray edges or non-manifold edges");
        }
    }
    let mut adjacent: BTreeMap<FaceId, BTreeSet<FaceId>> = BTreeMap::new();
    let mut around: BTreeMap<VertexId, BTreeMap<FaceId, BTreeSet<FaceId>>> = BTreeMap::new();
    for steps in edges.values() {
        if steps.len() != 2 {
            return Err("Open or non-manifold surface");
        }
        let [(f, a, b), (g, c, d)] = [steps[0], steps[1]];
        if f == g || a != d || b != c {
            return Err("Inconsistent face orientation");
        }
        adjacent.entry(f).or_default().insert(g);
        adjacent.entry(g).or_default().insert(f);
        for v in [a, b] {
            around.entry(v).or_default().entry(f).or_default().insert(g);
            around.entry(v).or_default().entry(g).or_default().insert(f);
        }
    }
    let faces: BTreeSet<_> = mesh.faces().map(|(id, _)| id).collect();
    if connected(&adjacent, *faces.first().unwrap()).len() != faces.len() {
        return Err("Multiple shells: volume is not summed");
    }
    for (v, fan) in &around {
        if connected(fan, *vertex_faces[v].first().unwrap()).len() != vertex_faces[v].len() {
            return Err("Non-manifold vertex");
        }
    }
    // Translate before integrating to avoid cancellation far from the origin.
    let origin = mesh.pos(edges.first_key_value().unwrap().0.0);
    let mut volume = 0.;
    let mut extent: f64 = 0.;
    for f in faces {
        let triangles = triangulate_face(mesh, f);
        if triangles.is_empty() {
            return Err("Cannot triangulate surface");
        }
        for t in triangles {
            let [a, b, c] = t.map(|v| mesh.pos(v) - origin);
            extent = extent.max(a.length()).max(b.length()).max(c.length());
            volume += a.dot(b.cross(c)) / 6.;
        }
    }
    let volume = volume.abs();
    if !volume.is_finite() || volume <= extent.powi(3) * 1e-12 {
        return Err("Degenerate volume");
    }
    Ok(volume)
}

fn connected(graph: &BTreeMap<FaceId, BTreeSet<FaceId>>, seed: FaceId) -> BTreeSet<FaceId> {
    let mut seen = BTreeSet::from([seed]);
    let mut stack = vec![seed];
    while let Some(f) = stack.pop() {
        for g in graph.get(&f).into_iter().flatten() {
            if seen.insert(*g) {
                stack.push(*g);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Definition, DefinitionId, Editor};
    use rysmo_math::DVec3;
    fn box_editor(origin: DVec3) -> Editor {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: origin, u: DVec3::X * 2., v: DVec3::Y * 3., facing: None }).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face, distance: -4., keep_base: false }).unwrap();
        e
    }
    #[test]
    fn box_volume_is_stable_far_from_origin_and_after_reversing_every_face() {
        let mut e = box_editor(DVec3::splat(1e6));
        assert!((shell_volume(e.mesh()).unwrap() - 24.).abs() < 1e-8);
        let faces = e.mesh().faces().map(|(id, _)| id).collect();
        e.apply(Command::ReverseFaces { faces }).unwrap();
        assert!((shell_volume(e.mesh()).unwrap() - 24.).abs() < 1e-8);
    }
    #[test]
    fn open_faces_loose_edges_and_inconsistent_facing_have_no_volume() {
        let mut e = box_editor(DVec3::ZERO);
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::ReverseFaces { faces: vec![face] }).unwrap();
        assert_eq!(shell_volume(e.mesh()), Err("Inconsistent face orientation"));
        e.undo();
        e.apply(Command::DrawLines { lines: vec![[DVec3::splat(10.), DVec3::splat(11.)]], facing: None }).unwrap();
        assert!(shell_volume(e.mesh()).is_err());
        let mut flat = Editor::new();
        flat.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        assert!(shell_volume(flat.mesh()).is_err());
    }
    #[test]
    fn instance_volume_includes_mirrors_nonuniform_scale_and_parent_context() {
        let e = box_editor(DVec3::ZERO);
        let mut doc = Document::new();
        let id = DefinitionId(1000);
        doc.definitions.insert(id, (Definition { mesh: e.mesh().clone(), ..Default::default() }).into());
        let mut inst = Instance {
            definition: id,
            transform: DMat4::from_scale(DVec3::new(-2., 3., 4.)),
            name: "Box".into(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
        };
        assert!((doc.instance_volume(&inst, DMat4::from_scale(DVec3::splat(2.))).unwrap() - 4608.).abs() < 1e-8);
        inst.transform = DMat4::from_scale(DVec3::new(1., 0., 1.));
        assert!(doc.instance_volume(&inst, DMat4::IDENTITY).is_err());
        doc.def_mut(Some(id)).unwrap().instances.insert(crate::InstanceId(1001), inst.clone());
        assert_eq!(doc.instance_volume(&inst, DMat4::IDENTITY), Err("Contains nested groups or components"));
    }
    #[test]
    fn independent_volume_units_precision_validation_and_undo() {
        let mut e = box_editor(DVec3::ZERO);
        let mesh = e.mesh().clone();
        assert_eq!(e.doc().format_volume(1.), "1.000 m³");
        let mut settings = e.doc().measurement;
        settings.volume_units = Some(crate::Units::Centimetres);
        settings.volume_precision = 1;
        e.apply(Command::SetMeasurementSettings { settings }).unwrap();
        assert_eq!(e.doc().format_volume(1.), "1000000.0 cm³");
        assert_eq!(e.doc().format_area(1.), "1.000 m²");
        assert_eq!(e.mesh(), &mesh);
        e.undo();
        assert_eq!(e.doc().format_volume(1.), "1.000 m³");
        settings.volume_units = Some(crate::Units::Architectural);
        assert!(e.apply(Command::SetMeasurementSettings { settings }).is_err());
        settings.volume_units = Some(crate::Units::Metres);
        settings.volume_precision = 7;
        assert!(e.apply(Command::SetMeasurementSettings { settings }).is_err());
    }
    #[test]
    fn disconnected_shells_are_not_incorrectly_summed() {
        let mut e = box_editor(DVec3::ZERO);
        e.apply(Command::DrawRectangle { corner: DVec3::X * 10., u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = e.mesh().faces().find(|(_, f)| e.mesh().pos(f.outer[0]).x >= 10.).unwrap().0;
        e.apply(Command::PushPull { face, distance: -1., keep_base: false }).unwrap();
        assert_eq!(shell_volume(e.mesh()), Err("Multiple shells: volume is not summed"));
    }
    #[test]
    fn through_hole_is_subtracted_from_the_enclosed_volume() {
        let mut e = Editor::new();
        for (corner, u, v) in
            [(DVec3::ZERO, DVec3::X * 3., DVec3::Y * 3.), (DVec3::new(1., 1., 0.), DVec3::X, DVec3::Y)]
        {
            e.apply(Command::DrawRectangle { corner, u, v, facing: None }).unwrap();
        }
        let inner = e.mesh().faces().find(|(_, f)| f.holes.is_empty()).unwrap().0;
        e.apply(Command::Erase { elements: vec![crate::Element::Face(inner)] }).unwrap();
        let ring = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face: ring, distance: -2., keep_base: false }).unwrap();
        assert!((shell_volume(e.mesh()).unwrap() - 16.).abs() < 1e-8);
    }
}
