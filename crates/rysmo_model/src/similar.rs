//! Read-only selection queries in the current editing context.
use crate::{CommandError, Editor, Element};
use rysmo_math::EPS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Criterion {
    Material,
    EdgeLength { relative_tolerance: f64, absolute_tolerance: f64 },
    Definition,
}
pub fn find(
    editor: &Editor,
    seed: Element,
    criterion: Criterion,
    include_hidden: bool,
) -> Result<Vec<Element>, CommandError> {
    let invalid = |text: &str| CommandError::Invalid(text.into());
    let mesh = editor.mesh();
    let active = editor.active();
    if mesh.edge_count().saturating_add(mesh.face_count()).saturating_add(active.instances.len()) > 200_000 {
        return Err(invalid("Selection query exceeds 200000 entities in the current context"));
    }
    let transform = editor.context_transform();
    let length = |id| {
        let (a, b) = mesh.edge_points(id);
        transform.transform_vector3(b - a).length()
    };
    let (seed_material, definition, seed_length) = match seed {
        Element::Edge(id) => {
            let edge = mesh.edge(id).ok_or_else(|| invalid("Reference edge no longer exists"))?;
            (edge.material, None, Some(length(id)))
        }
        Element::Face(id) => {
            (mesh.face(id).ok_or_else(|| invalid("Reference face no longer exists"))?.material, None, None)
        }
        Element::Instance(id) => {
            let instance = active.instances.get(&id).ok_or_else(|| invalid("Reference instance no longer exists"))?;
            (instance.material, Some(instance.definition), None)
        }
        _ => return Err(invalid("Choose one edge, face, group or component as reference")),
    };
    let tolerance = match criterion {
        Criterion::EdgeLength { relative_tolerance, absolute_tolerance } => {
            let reference = seed_length.ok_or_else(|| invalid("Length matching requires an edge"))?;
            if !reference.is_finite()
                || !relative_tolerance.is_finite()
                || !(0. ..=1.).contains(&relative_tolerance)
                || !absolute_tolerance.is_finite()
                || !(0. ..=1000.).contains(&absolute_tolerance)
            {
                return Err(invalid("Invalid edge-length tolerance"));
            }
            (reference * relative_tolerance).max(absolute_tolerance).max(EPS)
        }
        Criterion::Definition if definition.is_none() => {
            return Err(invalid("Definition matching requires a group or component"));
        }
        _ => 0.,
    };
    let found = match seed {
        Element::Edge(_) => mesh
            .edges()
            .filter(|(_, edge)| !edge.procedural && (include_hidden || edge.visible()))
            .filter(|(id, edge)| match criterion {
                Criterion::Material => edge.material == seed_material,
                Criterion::EdgeLength { .. } => (length(*id) - seed_length.expect("edge reference")).abs() <= tolerance,
                Criterion::Definition => false,
            })
            .map(|(id, _)| Element::Edge(id))
            .collect(),
        Element::Face(_) => mesh
            .faces()
            .filter(|(_, face)| include_hidden || !face.hidden)
            .filter(|(_, face)| face.material == seed_material)
            .map(|(id, _)| Element::Face(id))
            .collect(),
        Element::Instance(_) => active
            .instances
            .iter()
            .filter(|(_, instance)| include_hidden || (!instance.hidden && editor.doc().tag_visible(instance.tag)))
            .filter(|(_, instance)| match criterion {
                Criterion::Definition => Some(instance.definition) == definition,
                Criterion::Material => instance.material == seed_material,
                _ => false,
            })
            .map(|(id, _)| Element::Instance(*id))
            .collect(),
        _ => unreachable!("reference checked"),
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use rysmo_math::DVec3;
    #[test]
    fn material_matching_is_typed_visibility_aware_and_read_only() {
        let mut editor = Editor::new();
        for x in [0., 3., 6.] {
            editor
                .apply(Command::DrawRectangle { corner: DVec3::X * x, u: DVec3::X, v: DVec3::Y, facing: None })
                .unwrap();
        }
        let faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
        let material = editor
            .apply(Command::AddMaterial { name: "Reference".into(), color: [1., 0., 0., 1.], texture: None })
            .unwrap()
            .material
            .unwrap();
        editor.apply(Command::Paint { faces: faces[..2].to_vec(), material: Some(material), back: false }).unwrap();
        editor.apply(Command::SetGeometryHidden { elements: vec![Element::Face(faces[1])], hidden: true }).unwrap();
        let before = editor.doc().clone();
        let revision = editor.revision();
        let depth = editor.undo_depth();
        assert_eq!(
            find(&editor, Element::Face(faces[0]), Criterion::Material, false).unwrap(),
            vec![Element::Face(faces[0])]
        );
        assert_eq!(find(&editor, Element::Face(faces[0]), Criterion::Material, true).unwrap().len(), 2);
        assert_eq!(
            find(&editor, Element::Face(faces[2]), Criterion::Material, true).unwrap(),
            vec![Element::Face(faces[2])]
        );
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_depth(), depth);
    }
    #[test]
    fn length_matches_world_dimensions_inside_nonuniformly_scaled_group() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::Y * 3., DVec3::Y * 5.], [DVec3::Y * 7., DVec3::Y * 8.]],
                facing: None,
            })
            .unwrap();
        let group = editor.apply(Command::MakeGroup { elements: out.created }).unwrap().created[0];
        editor
            .apply(Command::Scale { elements: vec![group], origin: DVec3::ZERO, factors: DVec3::new(2., 1., 1.) })
            .unwrap();
        let Element::Instance(id) = group else { panic!() };
        editor.enter(id).unwrap();
        let edge = editor
            .mesh()
            .edges()
            .find(|(id, _)| {
                let (a, b) = editor.mesh().edge_points(*id);
                (b - a).x.abs() > 0.5
            })
            .unwrap()
            .0;
        let found = find(
            &editor,
            Element::Edge(edge),
            Criterion::EdgeLength { relative_tolerance: 0., absolute_tolerance: 0. },
            false,
        )
        .unwrap();
        assert_eq!(found.len(), 2);
        assert!(
            find(
                &editor,
                Element::Edge(edge),
                Criterion::EdgeLength { relative_tolerance: f64::NAN, absolute_tolerance: 0. },
                false
            )
            .is_err()
        );
        assert!(find(&editor, Element::Edge(edge), Criterion::Definition, false).is_err());
    }
    #[test]
    fn definition_matching_respects_hidden_instances_tags_and_context() {
        let mut editor = Editor::new();
        let out = editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let component =
            editor.apply(Command::MakeComponent { elements: out.created, name: Some("Part".into()) }).unwrap().created
                [0];
        editor.apply(Command::Copy { elements: vec![component], offset: DVec3::Y * 2., copies: 2 }).unwrap();
        let ids: Vec<_> = editor.active().instances.keys().copied().collect();
        editor.apply(Command::SetInstancesHidden { instances: vec![ids[1]], hidden: true }).unwrap();
        let tag = editor.apply(Command::AddTag { name: "Hidden".into() }).unwrap().tag.unwrap();
        editor.apply(Command::AssignTag { instances: vec![ids[2]], tag: Some(tag) }).unwrap();
        editor.apply(Command::SetTag { tag, name: None, visible: Some(false) }).unwrap();
        assert_eq!(find(&editor, component, Criterion::Definition, false).unwrap(), vec![component]);
        assert_eq!(find(&editor, component, Criterion::Definition, true).unwrap().len(), 3);
        editor.enter(ids[0]).unwrap();
        assert!(find(&editor, component, Criterion::Definition, true).is_err());
    }
    #[test]
    fn tolerances_and_soft_edges_are_explicit() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawLines {
                lines: vec![
                    [DVec3::ZERO, DVec3::X],
                    [DVec3::Y, DVec3::Y + DVec3::X * 1.0005],
                    [DVec3::Y * 2., DVec3::Y * 2. + DVec3::X * 1.01],
                ],
                facing: None,
            })
            .unwrap();
        let edges: Vec<_> = editor.mesh().edges().map(|(id, _)| id).collect();
        let criterion = Criterion::EdgeLength { relative_tolerance: 0.001, absolute_tolerance: 0. };
        assert_eq!(find(&editor, Element::Edge(edges[0]), criterion, false).unwrap().len(), 2);
        editor
            .apply(Command::SetEdges { edges: vec![edges[1]], soft: Some(true), smooth: None, hidden: None })
            .unwrap();
        assert_eq!(find(&editor, Element::Edge(edges[0]), criterion, false).unwrap().len(), 1);
        assert_eq!(find(&editor, Element::Edge(edges[0]), criterion, true).unwrap().len(), 2);
    }
}
