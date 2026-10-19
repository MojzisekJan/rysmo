//! Intersect faces without boolean deletion: draw cut edges in the current context.
use crate::{
    Document, Element, InstanceId,
    sections::{SectionPlane, triangle_slice},
};
use rysmo_math::{DVec3, EPS};

fn triangle_intersection(a: [DVec3; 3], b: [DVec3; 3]) -> Option<[DVec3; 2]> {
    let normal = |t: [DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).try_normalize();
    let (na, nb) = (normal(a)?, normal(b)?);
    let dir = na.cross(nb).try_normalize()?;
    if na.cross(nb).length_squared() < 1e-16 {
        return None;
    }
    let plane = |origin, normal| SectionPlane { name: String::new(), origin, normal };
    let sa = triangle_slice(a, &plane(b[0], nb))?;
    let sb = triangle_slice(b, &plane(a[0], na))?;
    let origin = sa[0];
    let interval = |s: [DVec3; 2]| {
        let x = (s[0] - origin).dot(dir);
        let y = (s[1] - origin).dot(dir);
        (x.min(y), x.max(y))
    };
    let (a0, a1) = interval(sa);
    let (b0, b1) = interval(sb);
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    (hi - lo > EPS).then(|| [origin + dir * lo, origin + dir * hi])
}
/// Clip a coplanar boundary against a triangle, excluding triangulation diagonals.
fn clip_boundary(mut line: [DVec3; 2], triangle: [DVec3; 3], normal: DVec3) -> Option<[DVec3; 2]> {
    for i in 0..3 {
        let origin = triangle[i];
        let inward = normal.cross(triangle[(i + 1) % 3] - origin).normalize();
        let [a, b] = line.map(|p| (p - origin).dot(inward));
        if a < -EPS && b < -EPS {
            return None;
        }
        if (a < 0.) != (b < 0.) {
            let point = line[0].lerp(line[1], a / (a - b));
            line[if a < 0. { 0 } else { 1 }] = point;
        }
    }
    (line[0].distance(line[1]) > EPS).then_some(line)
}
struct FaceGeometry {
    triangles: Vec<[DVec3; 3]>,
    boundaries: Vec<[DVec3; 2]>,
    selected: bool,
    lo: DVec3,
    hi: DVec3,
}
pub fn lines(
    doc: &Document,
    context: &[InstanceId],
    elements: &[Element],
    with_model: bool,
) -> Result<Vec<(DVec3, DVec3)>, String> {
    doc.resolve(context).map_err(|_| "Invalid intersection context.")?;
    let inverse = doc.path_transform(context).inverse();
    let mut faces = Vec::new();
    for occurrence in doc.occurrences(context) {
        let Some(definition) = doc.def(occurrence.definition) else { continue };
        for (id, face) in definition.mesh.faces() {
            let selected = occurrence.path == context && elements.contains(&Element::Face(id))
                || occurrence.in_context && occurrence.top.is_some_and(|id| elements.contains(&Element::Instance(id)));
            if !with_model && !selected {
                continue;
            }
            let world = |v| occurrence.transform.transform_point3(definition.mesh.pos(v));
            let triangles: Vec<_> =
                rysmo_mesh::triangulate_face(&definition.mesh, id).into_iter().map(|t| t.map(world)).collect();
            if triangles.is_empty() {
                continue;
            }
            let boundaries: Vec<_> = face.loop_edges().into_iter().map(|(a, b)| [world(a), world(b)]).collect();
            let (lo, hi) = boundaries
                .iter()
                .flatten()
                .fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| {
                    (lo.min(*p), hi.max(*p))
                });
            faces.push(FaceGeometry { triangles, boundaries, selected, lo, hi });
        }
    }
    if !faces.iter().any(|face| face.selected) {
        return Err("Select faces or groups to intersect.".into());
    }
    let mut result = Vec::new();
    let mut push = |[p, q]: [DVec3; 2]| result.push((inverse.transform_point3(p), inverse.transform_point3(q)));
    for i in 0..faces.len() {
        for j in i + 1..faces.len() {
            let (a, b) = (&faces[i], &faces[j]);
            if !a.selected && !b.selected
                || a.lo.cmpgt(b.hi + DVec3::splat(EPS)).any()
                || b.lo.cmpgt(a.hi + DVec3::splat(EPS)).any()
            {
                continue;
            }
            let normal = |t: [DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]).normalize();
            let (na, nb) = (normal(a.triangles[0]), normal(b.triangles[0]));
            if na.cross(nb).length_squared() < 1e-16 {
                if (a.triangles[0][0] - b.triangles[0][0]).dot(nb).abs() > EPS {
                    continue;
                }
                for (source, target, n) in [(a, b, nb), (b, a, na)] {
                    for edge in &source.boundaries {
                        for triangle in &target.triangles {
                            if let Some(line) = clip_boundary(*edge, *triangle, n) {
                                push(line);
                            }
                        }
                    }
                }
            } else {
                for ta in &a.triangles {
                    for tb in &b.triangles {
                        if let Some(line) = triangle_intersection(*ta, *tb) {
                            push(line);
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor};
    fn rectangle(e: &mut Editor, corner: DVec3, u: DVec3, v: DVec3) -> Vec<Element> {
        e.apply(Command::DrawRectangle { corner, u, v, facing: None }).unwrap().created
    }
    #[test]
    fn intersect_selection_splits_crossing_faces_and_preserves_undo() {
        let mut e = Editor::new();
        rectangle(&mut e, DVec3::ZERO, DVec3::X * 2., DVec3::Y * 2.);
        rectangle(&mut e, DVec3::new(1., -1., -1.), DVec3::Y * 4., DVec3::Z * 2.);
        let selected: Vec<_> = e.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let before = e.doc().clone();
        let out = e.apply(Command::IntersectFaces { elements: selected, with_model: false, context: vec![] }).unwrap();
        assert!(out.changed);
        assert_eq!(e.mesh().faces().count(), 3);
        e.mesh().validate().unwrap();
        assert!((e.mesh().faces().map(|(id, _)| e.mesh().face_area(id)).sum::<f64>() - 12.).abs() < EPS);
        assert!(e.mesh().edges().any(|(_, edge)| {
            let [a, b] = edge.v.map(|v| e.mesh().pos(v));
            (a.x - 1.).abs() < EPS
                && (b.x - 1.).abs() < EPS
                && a.z.abs() < EPS
                && b.z.abs() < EPS
                && a.distance(b) > 0.5
        }));
        let after = e.doc().clone();
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert_eq!(e.doc(), &after);
    }
    #[test]
    fn intersect_model_in_transformed_group_keeps_external_definition_unchanged() {
        let mut e = Editor::new();
        let items = rectangle(&mut e, DVec3::ZERO, DVec3::X * 2., DVec3::Y * 2.);
        let group = e
            .apply(Command::MakeGroup { elements: items })
            .unwrap()
            .created
            .into_iter()
            .find_map(|el| if let Element::Instance(id) = el { Some(id) } else { None })
            .unwrap();
        e.apply(Command::Move { elements: vec![Element::Instance(group)], offset: DVec3::X * 4. }).unwrap();
        rectangle(&mut e, DVec3::new(5., -1., -1.), DVec3::Y * 4., DVec3::Z * 2.);
        let root = e.doc().root.clone();
        e.enter(group).unwrap();
        let selected = e.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        e.apply(Command::IntersectFaces { elements: selected, with_model: true, context: vec![group] }).unwrap();
        assert_eq!(e.doc().root, root);
        assert_eq!(e.mesh().faces().count(), 2);
        assert!(e.mesh().vertices().any(|(_, v)| (v.pos.x - 1.).abs() < EPS));
    }
    #[test]
    fn coplanar_group_overlap_uses_boundaries_without_diagonals() {
        let mut e = Editor::new();
        let first = rectangle(&mut e, DVec3::ZERO, DVec3::X * 2., DVec3::Y * 2.);
        let a = e.apply(Command::MakeGroup { elements: first }).unwrap().created;
        let second = rectangle(&mut e, DVec3::new(1., 1., 0.), DVec3::X * 2., DVec3::Y * 2.);
        let b = e.apply(Command::MakeGroup { elements: second }).unwrap().created;
        let definitions = e.doc().definitions.clone();
        let mut selected = a;
        selected.extend(b);
        let lines = lines(e.doc(), &[], &selected, false).unwrap();
        assert!(!lines.is_empty());
        for (p, q) in &lines {
            assert!(p.x >= 1. - EPS && p.y >= 1. - EPS && p.x <= 2. + EPS && p.y <= 2. + EPS);
            assert!(q.x >= 1. - EPS && q.y >= 1. - EPS && q.x <= 2. + EPS && q.y <= 2. + EPS);
            assert!((p.x - q.x).abs() < EPS || (p.y - q.y).abs() < EPS);
        }
        e.apply(Command::IntersectFaces { elements: selected, with_model: false, context: vec![] }).unwrap();
        assert_eq!(e.doc().definitions, definitions);
        e.mesh().validate().unwrap();
        assert!((e.mesh().faces().map(|(id, _)| e.mesh().face_area(id)).sum::<f64>() - 1.).abs() < EPS);
    }
    #[test]
    fn selection_excludes_external_faces_but_model_includes_them() {
        let mut e = Editor::new();
        let first = rectangle(&mut e, DVec3::ZERO, DVec3::X * 2., DVec3::Y * 2.);
        let selected = e.apply(Command::MakeGroup { elements: first }).unwrap().created;
        rectangle(&mut e, DVec3::new(1., -1., -1.), DVec3::Y * 4., DVec3::Z * 2.);
        assert!(lines(e.doc(), &[], &selected, false).unwrap().is_empty());
        assert!(!lines(e.doc(), &[], &selected, true).unwrap().is_empty());
    }
    #[test]
    fn disjoint_faces_do_not_create_history_and_invalid_context_is_atomic() {
        let mut e = Editor::new();
        let items = rectangle(&mut e, DVec3::ZERO, DVec3::X, DVec3::Y);
        let before = e.doc().clone();
        let result =
            e.apply(Command::IntersectFaces { elements: items.clone(), with_model: true, context: vec![] }).unwrap();
        assert!(!result.changed);
        assert_eq!(e.doc(), &before);
        assert!(
            e.apply(Command::IntersectFaces { elements: items, with_model: true, context: vec![InstanceId(999)] })
                .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
}
