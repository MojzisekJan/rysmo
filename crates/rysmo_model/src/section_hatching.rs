//! Bounded section hatch geometry shared by viewport and vector output.
use crate::{CommandError, Document, MaterialId};
use rysmo_math::{DMat4, DVec3, EPS};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hatch {
    pub angle: f64,
    pub spacing: f64,
    pub cross: bool,
}
impl Default for Hatch {
    fn default() -> Self {
        Self { angle: 45., spacing: 0.1, cross: false }
    }
}
impl Hatch {
    pub fn valid(self) -> bool {
        self.angle.is_finite() && self.spacing.is_finite() && (0.0001..=1000.).contains(&self.spacing)
    }
}
fn invalid(text: &str) -> CommandError {
    CommandError::Invalid(text.into())
}
/// Pair scanline crossings using even/odd fill, retaining holes and nested islands.
pub fn lines(
    segments: &[[DVec3; 2]],
    plane: &crate::sections::SectionPlane,
    hatch: Hatch,
    work: &mut usize,
) -> Result<Vec<[DVec3; 2]>, CommandError> {
    if !hatch.valid() {
        return Err(invalid("Hatch spacing must be 0.1 mm to 1000 m, with a finite angle"));
    }
    if segments.is_empty() {
        return Ok(Vec::new());
    }
    let (u, v) = rysmo_math::Plane::new(plane.normal, plane.origin).basis();
    let mut result = Vec::new();
    for angle in std::iter::once(hatch.angle).chain(hatch.cross.then_some(hatch.angle + 90.)) {
        let (sin, cos) = angle.rem_euclid(360.).to_radians().sin_cos();
        // Exact quarter turns must use the same half-open boundary convention.
        let snap = |n: f64| {
            if n.abs() < 1e-12 {
                0.
            } else if (n.abs() - 1.).abs() < 1e-12 {
                n.signum()
            } else {
                n
            }
        };
        let (sin, cos) = (snap(sin), snap(cos));
        let x = u * cos + v * sin;
        let y = v * cos - u * sin;
        let (lo, hi) = segments.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            let n = (*p - plane.origin).dot(y);
            (lo.min(n), hi.max(n))
        });
        let start = (lo / hatch.spacing).ceil();
        let end = (hi / hatch.spacing).floor();
        let count = (end - start + 1.).max(0.);
        if !count.is_finite() || count > 4096. {
            return Err(invalid("Hatch exceeds 4096 scanlines; increase spacing"));
        }
        for index in 0..count as usize {
            *work = work
                .checked_sub(segments.len())
                .ok_or_else(|| invalid("Section hatching exceeds its intersection budget"))?;
            let level = (start + index as f64) * hatch.spacing;
            let mut crossings = Vec::new();
            for [a, b] in segments {
                let a = *a - plane.origin;
                let b = *b - plane.origin;
                let ya = a.dot(y);
                let yb = b.dot(y);
                if (ya <= level && yb > level) || (yb <= level && ya > level) {
                    crossings.push(a.lerp(b, (level - ya) / (yb - ya)).dot(x));
                }
            }
            crossings.sort_by(f64::total_cmp);
            let mut parity: Vec<(f64, usize)> = Vec::new();
            for crossing in crossings {
                if let Some((value, count)) = parity.last_mut()
                    && (*value - crossing).abs() < EPS
                {
                    *count += 1;
                } else {
                    parity.push((crossing, 1));
                }
            }
            let crossings: Vec<_> =
                parity.into_iter().filter_map(|(value, count)| (count % 2 == 1).then_some(value)).collect();
            if crossings.len() % 2 != 0 {
                return Err(invalid("Open or ambiguous section contour cannot be hatched"));
            }
            for pair in crossings.as_chunks::<2>().0 {
                if pair[1] - pair[0] > EPS {
                    result.push([plane.origin + x * pair[0] + y * level, plane.origin + x * pair[1] + y * level]);
                }
            }
            if result.len() > 20_000 {
                return Err(invalid("Section hatching exceeds 20000 lines"));
            }
        }
    }
    Ok(result)
}
pub fn build(doc: &Document) -> Result<Vec<[DVec3; 2]>, CommandError> {
    if !doc.style.section_cuts
        || (doc.root.section_hatch.is_none()
            && doc.definitions.values().all(|d| d.section_hatch.is_none())
            && doc.materials.values().all(|m| m.section_hatch.is_none()))
    {
        return Ok(Vec::new());
    }
    let mut stack = vec![(None, DMat4::IDENTITY, Vec::new(), None::<MaterialId>, None::<Hatch>)];
    let mut occurrences = 4096usize;
    let mut work = 2_000_000usize;
    let mut result = Vec::new();
    while let Some((owner, transform, path, material, parent_hatch)) = stack.pop() {
        occurrences = occurrences.checked_sub(1).ok_or_else(|| invalid("Section hatching exceeds 4096 occurrences"))?;
        if path.len() > 32 || !transform.is_finite() || !transform.inverse().is_finite() {
            return Err(invalid("Invalid or deep hatch hierarchy"));
        }
        let def = doc.def(owner).ok_or_else(|| invalid("Missing hatch definition"))?;
        let cuts = doc.section_planes(&path);
        let object_hatch = if owner.is_some() { def.section_hatch.or(parent_hatch) } else { None };
        let hatch = object_hatch
            .or_else(|| material.and_then(|id| doc.materials.get(&id)).and_then(|m| m.section_hatch))
            .or(doc.root.section_hatch);
        if let Some(hatch) = hatch
            && !cuts.is_empty()
        {
            let mesh = doc.effective_mesh(owner);
            let mut segments = vec![Vec::new(); cuts.len()];
            for (id, face) in mesh.faces() {
                if face.hidden && !doc.style.show_hidden_geometry {
                    continue;
                }
                let corners = face.loops().map(|r| r.len()).sum::<usize>();
                if corners > 16384 {
                    return Err(invalid("A hatch face exceeds 16384 corners"));
                }
                work = work.checked_sub(corners).ok_or_else(|| invalid("Hatch geometry budget exceeded"))?;
                for triangle in rysmo_mesh::triangulate_face(&mesh, id) {
                    let points = triangle.map(|id| transform.transform_point3(mesh.pos(id)));
                    for (index, plane) in cuts.iter().enumerate() {
                        if let Some(line) = crate::sections::triangle_slice(points, plane) {
                            segments[index].push(line);
                        }
                    }
                }
            }
            for (plane, segments) in cuts.iter().zip(segments) {
                for line in lines(&segments, plane, hatch, &mut work)? {
                    if let Some(line) = crate::sections::clip_segment(line, &cuts) {
                        result.push(line);
                    }
                }
            }
            if result.len() > 20_000 {
                return Err(invalid("Section hatching exceeds 20000 lines"));
            }
        }
        for (id, instance) in &def.instances {
            if !instance.hidden && doc.tag_visible(instance.tag) {
                let mut child = path.clone();
                child.push(*id);
                stack.push((
                    Some(instance.definition),
                    transform * instance.transform,
                    child,
                    instance.material.or(material),
                    object_hatch,
                ));
            }
        }
        if stack.len() > 4096 {
            return Err(invalid("Section hatching exceeds hierarchy budget"));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ring(lo: f64, hi: f64) -> Vec<[DVec3; 2]> {
        let p = [DVec3::new(lo, lo, 0.), DVec3::new(hi, lo, 0.), DVec3::new(hi, hi, 0.), DVec3::new(lo, hi, 0.)];
        (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect()
    }
    fn plane() -> crate::sections::SectionPlane {
        crate::sections::SectionPlane { name: "Cut".into(), origin: DVec3::ZERO, normal: DVec3::Z }
    }
    #[test]
    fn hatch_preserves_holes_and_cross_pattern_in_world_units() {
        let segments: Vec<_> = ring(-2., 2.).into_iter().chain(ring(-1., 1.)).collect();
        let hatch = Hatch { angle: 0., spacing: 0.25, cross: false };
        let simple = lines(&segments, &plane(), hatch, &mut 100_000).unwrap();
        assert!(!simple.is_empty());
        for [a, b] in &simple {
            let mid = (*a + *b) * 0.5;
            assert!(mid.x.abs() >= 1. - EPS || mid.y.abs() >= 1. - EPS);
            assert!(a.z.abs() < EPS && b.z.abs() < EPS);
        }
        let cross = lines(&segments, &plane(), Hatch { cross: true, ..hatch }, &mut 100_000).unwrap();
        assert_eq!(cross.len(), simple.len() * 2);
    }
    #[test]
    fn open_contours_and_excessive_density_are_rejected() {
        let mut segments = ring(-1., 1.);
        segments.pop();
        assert!(lines(&segments, &plane(), Hatch::default(), &mut 100_000).is_err());
        assert!(
            lines(&ring(-100., 100.), &plane(), Hatch { spacing: 0.0001, ..Default::default() }, &mut 100_000).is_err()
        );
        assert!(lines(&ring(-1., 1.), &plane(), Hatch::default(), &mut 0).is_err());
    }
    #[test]
    fn object_hatch_overrides_propagate_through_nested_geometry() {
        use crate::{Command, Editor, Element, parametric::Spec};
        let mut editor =
            Editor::with_document(Spec::Stairs { width: 1., rise: 1., tread: 1., steps: 1 }.document().unwrap());
        let source = *editor.doc().root.instances.keys().next().unwrap();
        let out =
            editor.apply(Command::MakeComponent { elements: vec![Element::Instance(source)], name: None }).unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        let mut doc = editor.doc().clone();
        let definition = doc.root.instances[&instance].definition;
        doc.def_mut(Some(definition)).unwrap().section_hatch = Some(Hatch::default());
        let id = doc.reserve_id();
        doc.root_mut()
            .sections
            .planes
            .insert(id, crate::sections::SectionPlane { name: "Cut".into(), origin: DVec3::Z * 0.5, normal: DVec3::Z });
        doc.root_mut().sections.active = Some(id);
        assert!(!build(&doc).unwrap().is_empty());
        doc.def_mut(Some(definition)).unwrap().section_hatch = None;
        assert!(build(&doc).unwrap().is_empty());
    }
}
