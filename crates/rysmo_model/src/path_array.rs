//! Component occurrences distributed at measured distances along a bounded path.
use crate::{CommandError, DefinitionId, Document, EdgeId, InstanceId};
use rysmo_math::{DMat4, DQuat, DVec3, EPS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum Distribution {
    Count(u32),
    Spacing(f64),
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub distribution: Distribution,
    pub align_tangent: bool,
    pub reverse: bool,
    pub start_offset: f64,
}
impl Default for Options {
    fn default() -> Self {
        Self { distribution: Distribution::Count(10), align_tangent: true, reverse: false, start_offset: 0. }
    }
}
pub struct Plan {
    pub transforms: Vec<DMat4>,
    pub length: f64,
    pub closed: bool,
}
fn invalid(text: &str) -> CommandError {
    CommandError::Invalid(text.into())
}
#[derive(Clone, Copy)]
struct Work {
    occurrences: usize,
    vertices: usize,
    faces: usize,
    depth: usize,
}
fn workload(
    doc: &Document,
    id: DefinitionId,
    path: &mut BTreeSet<DefinitionId>,
    cache: &mut BTreeMap<DefinitionId, Work>,
) -> Result<Work, CommandError> {
    if path.len() >= 32 || !path.insert(id) {
        return Err(invalid("Source component hierarchy is cyclic or deeper than 32 levels"));
    }
    if let Some(work) = cache.get(&id).copied() {
        path.remove(&id);
        if path.len() + work.depth > 32 {
            return Err(invalid("Source component hierarchy exceeds 32 levels"));
        }
        return Ok(work);
    }
    let def = doc.definitions.get(&id).ok_or_else(|| invalid("Missing source definition"))?;
    let mut work = Work { occurrences: 1, vertices: def.mesh.vertex_count(), faces: def.mesh.face_count(), depth: 1 };
    for instance in def.instances.values() {
        let child = workload(doc, instance.definition, path, cache)?;
        work.occurrences = work.occurrences.saturating_add(child.occurrences);
        work.vertices = work.vertices.saturating_add(child.vertices);
        work.faces = work.faces.saturating_add(child.faces);
        work.depth = work.depth.max(child.depth + 1);
        if work.occurrences > 100_000 || work.vertices > 1_000_000 || work.faces > 500_000 {
            return Err(invalid("Source component exceeds the array expansion budget"));
        }
    }
    path.remove(&id);
    cache.insert(id, work);
    Ok(work)
}
pub fn plan(
    doc: &Document,
    owner: Option<DefinitionId>,
    context: DMat4,
    source: InstanceId,
    path: &[EdgeId],
    options: Options,
) -> Result<Plan, CommandError> {
    let active = doc.def(owner).ok_or_else(|| invalid("Missing array context"))?;
    let instance = active.instances.get(&source).ok_or_else(|| invalid("Choose a component in the current context"))?;
    let def = doc.definitions.get(&instance.definition).ok_or_else(|| invalid("Missing component definition"))?;
    if def.group || instance.hidden || !doc.tag_visible(instance.tag) {
        return Err(invalid("Choose a visible component, not a group"));
    }
    if path.is_empty()
        || path.len() > 2048
        || path.iter().any(|id| active.mesh.edge(*id).is_none_or(|edge| edge.procedural))
    {
        return Err(invalid("Choose up to 2048 editable connected path edges"));
    }
    if !context.is_finite() || !context.inverse().is_finite() {
        return Err(invalid("Invalid array context transform"));
    }
    let (vertices, closed) =
        active.mesh.edge_chain(path).ok_or_else(|| invalid("Path must be one unbranched connected chain or loop"))?;
    let mut points: Vec<_> = vertices.into_iter().map(|id| context.transform_point3(active.mesh.pos(id))).collect();
    if options.reverse {
        points.reverse();
        if closed {
            points.rotate_right(1);
        }
    }
    if closed {
        points.push(points[0]);
    }
    let mut cumulative = vec![0.];
    for pair in points.windows(2) {
        let length = pair[0].distance(pair[1]);
        if !length.is_finite() || length <= EPS {
            return Err(invalid("Path contains a degenerate segment"));
        }
        cumulative.push(cumulative.last().copied().unwrap_or(0.) + length);
    }
    let length = *cumulative.last().ok_or_else(|| invalid("Empty path"))?;
    if !length.is_finite()
        || !options.start_offset.is_finite()
        || options.start_offset < 0.
        || options.start_offset > length
    {
        return Err(invalid("Start offset must lie within the measured path"));
    }
    let extent = if closed { length } else { length - options.start_offset };
    let (count, step) = match options.distribution {
        Distribution::Count(count) => {
            if !(1..=1024).contains(&count) || (!closed && count > 1 && extent <= EPS) {
                return Err(invalid("Choose 1–1024 distinct placements"));
            }
            (
                count as usize,
                if closed {
                    length / count as f64
                } else if count > 1 {
                    extent / (count - 1) as f64
                } else {
                    0.
                },
            )
        }
        Distribution::Spacing(spacing) => {
            if !spacing.is_finite() || spacing <= EPS {
                return Err(invalid("Spacing must exceed model tolerance"));
            }
            let count =
                if closed { (length / spacing).ceil().max(1.) } else { (extent / spacing + 1e-10).floor() + 1. };
            if count > 1024. {
                return Err(invalid("Spacing would create more than 1024 placements"));
            }
            (count as usize, spacing)
        }
    };
    if count > 1 && (step <= EPS || (closed && length - (count - 1) as f64 * step <= EPS)) {
        return Err(invalid("Array placements would overlap within model tolerance"));
    }
    let work = workload(doc, instance.definition, &mut BTreeSet::new(), &mut BTreeMap::new())?;
    if work.occurrences.saturating_mul(count) > 100_000
        || work.vertices.saturating_mul(count) > 1_000_000
        || work.faces.saturating_mul(count) > 500_000
    {
        return Err(invalid("Array exceeds 100000 occurrences, 1000000 vertices or 500000 faces"));
    }
    let mut base = context * instance.transform;
    base.w_axis = rysmo_math::DVec4::W;
    let x = base.transform_vector3(DVec3::X).try_normalize().ok_or_else(|| invalid("Invalid source orientation"))?;
    if !base.is_finite() || !base.inverse().is_finite() {
        return Err(invalid("Invalid component transform"));
    }
    let mut transforms = Vec::with_capacity(count);
    for i in 0..count {
        let distance = options.start_offset + i as f64 * step;
        let distance = if closed { distance.rem_euclid(length) } else { distance.min(length) };
        let segment = cumulative.partition_point(|value| *value <= distance).saturating_sub(1).min(points.len() - 2);
        let tangent = (points[segment + 1] - points[segment]).normalize();
        let t = (distance - cumulative[segment]) / (cumulative[segment + 1] - cumulative[segment]);
        let point = points[segment].lerp(points[segment + 1], t);
        let rotation = if options.align_tangent {
            DMat4::from_quat(DQuat::from_rotation_arc(x, tangent))
        } else {
            DMat4::IDENTITY
        };
        let transform = context.inverse() * DMat4::from_translation(point) * rotation * base;
        if !transform.is_finite() || !transform.inverse().is_finite() {
            return Err(invalid("Invalid array placement"));
        }
        transforms.push(transform);
    }
    Ok(Plan { transforms, length, closed })
}
pub(crate) fn apply(
    doc: &mut Document,
    owner: Option<DefinitionId>,
    context: DMat4,
    source: InstanceId,
    path: &[EdgeId],
    options: Options,
) -> Result<Vec<crate::Element>, CommandError> {
    let plan = plan(doc, owner, context, source, path, options)?;
    let instance = doc
        .def(owner)
        .and_then(|def| def.instances.get(&source))
        .ok_or_else(|| invalid("Missing array source"))?
        .clone();
    let mut created = Vec::new();
    for transform in plan.transforms {
        let mut copy = instance.clone();
        copy.transform = transform;
        copy.glued_to = None;
        let id = InstanceId(doc.reserve_id());
        doc.def_mut(owner).expect("validated context").instances.insert(id, copy);
        created.push(crate::Element::Instance(id));
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element};
    fn fixture(closed: bool) -> (Editor, InstanceId, Vec<EdgeId>) {
        let mut editor = Editor::new();
        let elements = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = editor.apply(Command::MakeComponent { elements, name: Some("Panel".into()) }).unwrap();
        let source =
            out.created.iter().find_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }).unwrap();
        let a = DVec3::new(10., 0., 0.);
        let b = a + DVec3::X * 6.;
        let c = b + DVec3::Y * 4.;
        let mut lines = vec![[a, b], [b, c]];
        if closed {
            lines.extend([[c, a + DVec3::Y * 4.], [a + DVec3::Y * 4., a]]);
        }
        let out = editor.apply(Command::DrawLines { lines, facing: None }).unwrap();
        let path =
            out.created.into_iter().filter_map(|e| if let Element::Edge(id) = e { Some(id) } else { None }).collect();
        (editor, source, path)
    }
    fn options(distribution: Distribution) -> Options {
        Options { distribution, ..Default::default() }
    }
    #[test]
    fn count_includes_open_endpoints_and_spacing_keeps_measured_distances() {
        let (e, source, path) = fixture(false);
        let p = plan(e.doc(), None, DMat4::IDENTITY, source, &path, options(Distribution::Count(3))).unwrap();
        assert_eq!(p.length, 10.);
        assert!(!p.closed);
        let positions: Vec<_> = p.transforms.iter().map(|t| t.transform_point3(DVec3::ZERO)).collect();
        assert_eq!(positions, vec![DVec3::new(10., 0., 0.), DVec3::new(15., 0., 0.), DVec3::new(16., 4., 0.)]);
        let p = plan(e.doc(), None, DMat4::IDENTITY, source, &path, options(Distribution::Spacing(3.))).unwrap();
        assert_eq!(p.transforms.len(), 4);
        assert!(p.transforms[3].transform_point3(DVec3::ZERO).distance(DVec3::new(16., 3., 0.)) < 1e-9);
    }
    #[test]
    fn closed_loop_has_no_duplicate_seam_and_reverse_retains_start() {
        let (e, source, path) = fixture(true);
        let opts = options(Distribution::Count(4));
        let p = plan(e.doc(), None, DMat4::IDENTITY, source, &path, opts).unwrap();
        assert!(p.closed);
        assert_eq!(p.length, 20.);
        for (i, a) in p.transforms.iter().enumerate() {
            for b in &p.transforms[i + 1..] {
                assert!(a.w_axis.distance(b.w_axis) > 1.);
            }
        }
        let reverse = plan(e.doc(), None, DMat4::IDENTITY, source, &path, Options { reverse: true, ..opts }).unwrap();
        assert_eq!(p.transforms[0].w_axis, reverse.transforms[0].w_axis);
        assert!(reverse.transforms[0].transform_vector3(DVec3::X).normalize().distance(DVec3::Y) < 1e-9);
    }
    #[test]
    fn world_spacing_respects_mirrored_nonuniform_context_and_preserves_shape() {
        let (e, source, path) = fixture(false);
        let context = DMat4::from_scale(DVec3::new(-2., 3., 4.));
        let p = plan(e.doc(), None, context, source, &path, options(Distribution::Count(3))).unwrap();
        assert_eq!(p.length, 24.);
        for t in p.transforms {
            let world = context * t;
            for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
                assert!(
                    (world.transform_vector3(axis).length() - context.transform_vector3(axis).length()).abs() < 1e-9
                );
            }
        }
    }
    #[test]
    fn command_retains_source_definition_appearance_and_is_one_undo_step() {
        let (mut e, source, path) = fixture(false);
        let before = e.doc().clone();
        let original = before.root.instances[&source].clone();
        let out = e
            .apply(Command::ArrayAlongPath {
                source,
                path,
                options: options(Distribution::Count(5)),
                context: DMat4::IDENTITY,
            })
            .unwrap();
        assert_eq!(out.created.len(), 5);
        assert_eq!(e.doc().root.instances.len(), 6);
        assert_eq!(e.doc().root.instances[&source], original);
        for element in out.created {
            let Element::Instance(id) = element else { panic!() };
            let copy = &e.doc().root.instances[&id];
            assert_eq!(copy.definition, original.definition);
            assert_eq!(copy.material, original.material);
            assert_eq!(copy.tag, original.tag);
        }
        let after = e.doc().clone();
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert_eq!(e.doc(), &after);
    }
    #[test]
    fn invalid_options_and_paths_reject_without_partial_changes() {
        let (mut e, source, path) = fixture(false);
        let before = e.doc().clone();
        for distribution in [
            Distribution::Count(0),
            Distribution::Count(1025),
            Distribution::Spacing(f64::NAN),
            Distribution::Spacing(1e-9),
        ] {
            assert!(
                e.apply(Command::ArrayAlongPath {
                    source,
                    path: path.clone(),
                    options: options(distribution),
                    context: DMat4::IDENTITY
                })
                .is_err()
            );
            assert_eq!(e.doc(), &before);
        }
        assert!(plan(e.doc(), None, DMat4::IDENTITY, source, &[EdgeId(u64::MAX)], Options::default()).is_err());
    }
    #[test]
    fn branches_and_disconnected_paths_are_rejected() {
        for line in
            [[DVec3::new(16., 0., 0.), DVec3::new(16., 0., 5.)], [DVec3::new(30., 0., 0.), DVec3::new(31., 0., 0.)]]
        {
            let (mut e, source, mut path) = fixture(false);
            let out = e.apply(Command::DrawLines { lines: vec![line], facing: None }).unwrap();
            path.extend(
                out.created.into_iter().filter_map(|e| if let Element::Edge(id) = e { Some(id) } else { None }),
            );
            let before = e.doc().clone();
            assert!(
                e.apply(Command::ArrayAlongPath {
                    source,
                    path,
                    options: Options::default(),
                    context: DMat4::IDENTITY
                })
                .is_err()
            );
            assert_eq!(e.doc(), &before);
        }
    }
    #[test]
    fn cyclic_sources_and_near_duplicate_closed_seams_are_refused() {
        let (e, source, path) = fixture(true);
        assert!(
            plan(e.doc(), None, DMat4::IDENTITY, source, &path, options(Distribution::Spacing(20. - EPS * 0.5)))
                .is_err()
        );
        let mut doc = e.doc().clone();
        let instance = doc.root.instances[&source].clone();
        let id = InstanceId(doc.reserve_id());
        std::sync::Arc::make_mut(doc.definitions.get_mut(&instance.definition).unwrap()).instances.insert(id, instance);
        assert!(plan(&doc, None, DMat4::IDENTITY, source, &path, Options::default()).is_err());
    }
}
