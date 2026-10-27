//! Regeneration with semantic-part paint retention and planar architectural fitting.
use crate::{CommandError, DefinitionId, Document, FaceId, InstanceId, Mesh, parametric::Spec};
use rysmo_math::{DMat4, DVec3, EPS};
use std::collections::{BTreeMap, BTreeSet};
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
fn key(mesh: &Mesh, face: FaceId) -> Vec<Vec<[i64; 3]>> {
    mesh.face(face)
        .unwrap()
        .loops()
        .map(|ring| {
            let mut points: Vec<_> =
                ring.iter().map(|id| mesh.pos(*id).to_array().map(|n| (n / (EPS * 0.1)).round() as i64)).collect();
            points.sort();
            points
        })
        .collect()
}
fn roles(spec: &Spec, mesh: &Mesh) -> BTreeMap<(String, usize), FaceId> {
    let mut remaining: BTreeSet<_> = mesh.faces().map(|(id, _)| id).collect();
    let mut index = 0;
    let mut result = BTreeMap::new();
    while let Some(seed) = remaining.pop_first() {
        let mut queue = vec![seed];
        let mut shell = BTreeSet::from([seed]);
        while let Some(face) = queue.pop() {
            for edge in mesh.face_edges(face) {
                for neighbor in mesh.faces_of_edge(edge) {
                    if remaining.remove(&neighbor) {
                        shell.insert(neighbor);
                        queue.push(neighbor);
                    }
                }
            }
        }
        let role = match spec {
            Spec::Window { columns, .. } if index == 0 => "frame".into(),
            Spec::Window { columns, .. } if index < *columns as usize => format!("mullion:{}", index - 1),
            Spec::Window { columns, .. } => format!("pane:{}", index - *columns as usize),
            Spec::Door { .. } => {
                if index == 0 {
                    "frame".into()
                } else {
                    "leaf".into()
                }
            }
            Spec::Stairs { .. } => "stairs".into(),
        };
        for (face_index, face) in shell.into_iter().enumerate() {
            result.insert((role.clone(), face_index), face);
        }
        index += 1;
    }
    result
}
/// Preserve paint by generated part/face rather than unrelated regenerated global IDs.
pub fn regenerate(previous: &Spec, painted: &Mesh, next: &Spec) -> Result<Mesh, CommandError> {
    if std::mem::discriminant(previous) != std::mem::discriminant(next) {
        return Err(invalid("Change component type by creating a new component"));
    }
    if !previous.matches_geometry(painted).map_err(CommandError::Invalid)? {
        return Err(invalid("Component geometry was edited manually; regeneration would replace those edits"));
    }
    let expected = previous.mesh().map_err(CommandError::Invalid)?;
    let mut result = next.mesh().map_err(CommandError::Invalid)?;
    let actual: BTreeMap<_, _> = painted.faces().map(|(id, _)| (key(painted, id), id)).collect();
    let old = roles(previous, &expected);
    let new = roles(next, &result);
    let uniform = painted
        .faces()
        .next()
        .map(|(_, f)| (f.material, f.back_material, f.uv, f.hidden))
        .filter(|paint| painted.faces().all(|(_, f)| (f.material, f.back_material, f.uv, f.hidden) == *paint));
    if matches!((previous,next),(Spec::Stairs {steps:a,..},Spec::Stairs {steps:b,..}) if a!=b) && uniform.is_none() {
        return Err(invalid(
            "Changing stair count with individual face paint is ambiguous; apply a uniform material first",
        ));
    }
    for (role, target) in new {
        let source = old.get(&role).and_then(|id| actual.get(&key(&expected, *id))).and_then(|id| painted.face(*id));
        let paint = source.map(|f| (f.material, f.back_material, f.uv, f.hidden)).or(uniform);
        if let Some((material, back_material, uv, hidden)) = paint {
            let face = result.face_mut(target).unwrap();
            face.material = material;
            face.back_material = back_material;
            face.uv = uv;
            face.hidden = hidden;
        }
    }
    Ok(result)
}
/// Fit a window/door's local X/Z rectangle to one editable planar rectangle.
pub fn fit(
    doc: &Document,
    owner: Option<DefinitionId>,
    instance: InstanceId,
    face: FaceId,
) -> Result<(Spec, DMat4), CommandError> {
    let active = doc.def(owner).ok_or_else(|| invalid("Missing fitting context"))?;
    let occurrence = active.instances.get(&instance).ok_or_else(|| invalid("Choose one architectural component"))?;
    let mut spec = doc
        .definitions
        .get(&occurrence.definition)
        .and_then(|d| d.parametric.clone())
        .ok_or_else(|| invalid("Choose a parametric window or door"))?;
    let target = active.mesh.face(face).ok_or_else(|| invalid("Choose one rectangular target face"))?;
    if target.hidden
        || !target.holes.is_empty()
        || target.outer.len() != 4
        || active.mesh.face_edges(face).iter().any(|id| active.mesh.edge(*id).is_some_and(|edge| edge.procedural))
    {
        return Err(invalid("Choose an editable rectangular face without holes"));
    }
    let normal = active.mesh.face_normal(face).ok_or_else(|| invalid("Invalid target plane"))?;
    let points = active.mesh.loop_points(&target.outer);
    let mut vertical = DVec3::Z - normal * normal.z;
    if vertical.length() < EPS {
        vertical = points[3] - points[0];
    }
    let v = vertical.normalize();
    let u = v.cross(normal).normalize();
    let ranges = |axis: DVec3| {
        points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            let n = p.dot(axis);
            (lo.min(n), hi.max(n))
        })
    };
    let (x0, x1) = ranges(u);
    let (z0, z1) = ranges(v);
    let width = x1 - x0;
    let height = z1 - z0;
    if points.iter().any(|p| {
        let x = p.dot(u);
        let z = p.dot(v);
        !((x - x0).abs() < EPS || (x - x1).abs() < EPS) || !((z - z0).abs() < EPS || (z - z1).abs() < EPS)
    }) {
        return Err(invalid("Target must be an orthogonal rectangle"));
    }
    match &mut spec {
        Spec::Window { width: w, height: h, .. } | Spec::Door { width: w, height: h, .. } => {
            *w = width;
            *h = height;
        }
        _ => return Err(invalid("Planar fitting is available for windows and doors")),
    }
    spec.validate().map_err(CommandError::Invalid)?;
    let origin = u * x0 + v * z0 + normal * points[0].dot(normal);
    Ok((spec, DMat4::from_cols(u.extend(0.), (-normal).extend(0.), v.extend(0.), origin.extend(1.))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element, MaterialId};
    #[test]
    fn resizing_and_changing_panes_retains_semantic_part_paint() {
        let previous = Spec::default();
        let mut mesh = previous.mesh().unwrap();
        for ((role, _), face) in roles(&previous, &mesh) {
            mesh.face_mut(face).unwrap().material = Some(MaterialId(if role == "frame" {
                11
            } else if role == "pane:0" {
                12
            } else {
                13
            }));
        }
        let next = Spec::Window { width: 2., height: 2.1, depth: 0.2, frame: 0.07, columns: 3 };
        let result = regenerate(&previous, &mesh, &next).unwrap();
        for ((role, _), face) in roles(&next, &result) {
            if role == "frame" {
                assert_eq!(result.face(face).unwrap().material, Some(MaterialId(11)));
            }
            if role == "pane:0" {
                assert_eq!(result.face(face).unwrap().material, Some(MaterialId(12)));
            }
            if role == "pane:2" {
                assert_eq!(result.face(face).unwrap().material, None);
            }
        }
    }
    #[test]
    fn architectural_fit_is_atomic_unique_and_undoable() {
        let spec = Spec::default();
        let mut e = Editor::with_document(spec.document().unwrap());
        let instance = *e.doc().root.instances.keys().next().unwrap();
        let original_definition = e.doc().root.instances[&instance].definition;
        let out = e
            .apply(Command::DrawRectangle {
                corner: DVec3::new(5., 0., 0.),
                u: DVec3::X * 3.,
                v: DVec3::Z * 2.,
                facing: None,
            })
            .unwrap();
        let face = out.created.iter().find_map(|e| if let Element::Face(id) = e { Some(*id) } else { None }).unwrap();
        let before = e.doc().clone();
        e.apply(Command::FitParametric { instance, face, make_unique: true }).unwrap();
        let occurrence = &e.doc().root.instances[&instance];
        assert_ne!(occurrence.definition, original_definition);
        let fitted = e.doc().definitions[&occurrence.definition].parametric.as_ref().unwrap();
        assert!(matches!(fitted,Spec::Window {width,height,..} if (*width-3.).abs()<1e-9 && (*height-2.).abs()<1e-9));
        assert!(occurrence.transform.transform_point3(DVec3::ZERO).distance(DVec3::new(5., 0., 0.)) < 1e-9);
        e.undo();
        assert_eq!(e.doc(), &before);
    }
}
