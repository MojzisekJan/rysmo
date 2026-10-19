//! Textured materials: images, their real-world size, and where they sit on faces.

use std::sync::Arc;

use rysmo_math::{DVec2, DVec3, model_axes};
use rysmo_mesh::{ImageId, TexturePlacement};
use serde::{Deserialize, Serialize};

/// An image a textured material uses, kept in the document (and in the file) as its
/// encoded bytes (PNG or JPEG).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub name: String,
    pub mime: String,
    #[serde(serialize_with = "serialize_image_bytes", deserialize_with = "deserialize_image_bytes")]
    pub bytes: Arc<Vec<u8>>,
}

fn serialize_image_bytes<S: serde::Serializer>(bytes: &Arc<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
    bytes.as_ref().serialize(serializer)
}
fn deserialize_image_bytes<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Arc<Vec<u8>>, D::Error> {
    Vec::<u8>::deserialize(deserializer).map(Arc::new)
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.mime == other.mime
            && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }
}

/// A material's texture: which image, and how many metres one copy of it covers.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Texture {
    pub image: ImageId,
    /// Width and height of one copy of the image, in metres.
    pub size: [f64; 2],
}

/// The normal turned so that its largest component is positive, so that both sides of
/// a face, and parallel faces facing either way, use the same texture axes.
fn canonical(n: DVec3) -> DVec3 {
    let a = n.abs();
    let maximum = a.max_element();
    let tolerance = maximum * 1e-12;
    let big = if a.x >= maximum - tolerance {
        n.x
    } else if a.y >= maximum - tolerance {
        n.y
    } else {
        n.z
    };
    if big < 0.0 { -n } else { n }
}

/// The texture axes of a face with normal `n`: the plane's axes lined up with the model
/// axes.
pub fn texture_axes(n: DVec3) -> (DVec3, DVec3) {
    model_axes(canonical(n))
}

/// Texture coordinates of a point on a face with normal `n`: 0..1 across one copy of
/// the image, image rows going down as `v` grows (so the texture reads upright).
pub fn uv_at(p: DVec3, n: DVec3, size: [f64; 2], placement: &TexturePlacement) -> [f64; 2] {
    let (u, v) = texture_axes(placement.projected_normal.map(DVec3::from_array).unwrap_or(n));
    let q = DVec2::new(u.dot(p), v.dot(p)) - DVec2::from_array(placement.offset);
    let (s, c) = (-placement.rotation.to_radians()).sin_cos();
    let r = DVec2::new(q.x * c - q.y * s, q.x * s + q.y * c);
    let w = size[0] * placement.scale[0];
    let h = size[1] * placement.scale[1];
    let w = if w.abs() < 1e-9 { 1e-9 } else { w };
    let h = if h.abs() < 1e-9 { 1e-9 } else { h };
    let uv = [r.x / w, -r.y / h];
    placement
        .pins
        .and_then(pin_mapping)
        .map(|m| {
            let q = m * DVec3::new(uv[0], uv[1], 1.0);
            let z = if q.z.abs() < 1e-9 { if q.z < 0. { -1e-9 } else { 1e-9 } } else { q.z };
            [q.x / z, q.y / z]
        })
        .unwrap_or(uv)
}

/// The placement after the face moved by `d`: the texture moves with it.
pub fn moved(placement: TexturePlacement, n: DVec3, d: DVec3) -> TexturePlacement {
    let (u, v) = texture_axes(placement.projected_normal.map(DVec3::from_array).unwrap_or(n));
    TexturePlacement { offset: [placement.offset[0] + u.dot(d), placement.offset[1] + v.dot(d)], ..placement }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_texture_reads_from_above_whichever_way_the_face_points() {
        let pl = TexturePlacement::default();
        for n in [DVec3::Z, -DVec3::Z] {
            let a = uv_at(DVec3::new(0.0, 0.0, 0.0), n, [2.0, 1.0], &pl);
            let b = uv_at(DVec3::new(2.0, 0.0, 0.0), n, [2.0, 1.0], &pl);
            let c = uv_at(DVec3::new(0.0, 1.0, 0.0), n, [2.0, 1.0], &pl);
            assert_eq!(a, [0.0, 0.0]);
            assert!((b[0] - 1.0).abs() < 1e-12 && b[1].abs() < 1e-12);
            assert!(c[0].abs() < 1e-12 && (c[1] + 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn placement_offsets_rotates_and_scales() {
        let pl =
            TexturePlacement { offset: [0.5, 0.0], rotation: 90.0, scale: [2.0, 2.0], ..TexturePlacement::default() };
        // The point at the offset is the texture's corner.
        let a = uv_at(DVec3::new(0.5, 0.0, 0.0), DVec3::Z, [1.0, 1.0], &pl);
        assert!(a[0].abs() < 1e-12 && a[1].abs() < 1e-12);
        // Turned a quarter: +Y on the face runs along the texture's u.
        let b = uv_at(DVec3::new(0.5, 2.0, 0.0), DVec3::Z, [1.0, 1.0], &pl);
        assert!((b[0] - 1.0).abs() < 1e-9 && b[1].abs() < 1e-9);
    }

    #[test]
    fn a_moved_face_keeps_its_texture() {
        let n = DVec3::new(0.0, -1.0, 0.0);
        let pl =
            TexturePlacement { offset: [0.1, 0.2], rotation: 30.0, scale: [1.0, 1.5], ..TexturePlacement::default() };
        let d = DVec3::new(0.7, 0.0, 0.4);
        let after = moved(pl, n, d);
        let p = DVec3::new(0.3, 0.0, 0.9);
        let before = uv_at(p, n, [2.0, 1.0], &pl);
        let now = uv_at(p + d, n, [2.0, 1.0], &after);
        assert!((before[0] - now[0]).abs() < 1e-12 && (before[1] - now[1]).abs() < 1e-12);
    }
}

/// Inverse projective mapping from four displaced tile corners to image UVs.
/// Reject folded, collapsed and non-finite tiles rather than emitting invalid UVs.
pub fn pin_mapping(pins: [[f64; 2]; 4]) -> Option<rysmo_math::DMat3> {
    use rysmo_math::DMat3;
    let p = pins.map(DVec2::from_array);
    if p.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut sign = 0.0;
    for i in 0..4 {
        let cross = (p[(i + 1) % 4] - p[i]).perp_dot(p[(i + 2) % 4] - p[(i + 1) % 4]);
        if cross.abs() < 1e-9 || (sign != 0.0 && cross * sign <= 0.0) {
            return None;
        }
        sign = cross;
    }
    let d = p[0] - p[1] + p[2] - p[3];
    let a = p[1] - p[2];
    let b = p[3] - p[2];
    let det = a.perp_dot(b);
    if det.abs() < 1e-12 {
        return None;
    }
    let g = d.perp_dot(b) / det;
    let h = a.perp_dot(d) / det;
    let x = p[1] - p[0] + g * p[1];
    let y = p[3] - p[0] + h * p[3];
    let m = DMat3::from_cols(DVec3::new(x.x, x.y, g), DVec3::new(y.x, y.y, h), DVec3::new(p[0].x, p[0].y, 1.0));
    if !m.is_finite() || m.determinant().abs() < 1e-12 {
        return None;
    }
    let inverse = m.inverse();
    inverse.is_finite().then_some(inverse)
}

/// Check both numeric positioning controls and the optional four-pin deformation.
pub fn valid_placement(p: &TexturePlacement) -> bool {
    p.offset.iter().all(|v| v.is_finite())
        && p.rotation.is_finite()
        && p.scale.iter().all(|v| v.is_finite() && v.abs() >= 1e-9)
        && p.projected_normal.is_none_or(|n| {
            let n = DVec3::from_array(n);
            n.is_finite() && n.length_squared().is_finite() && n.length_squared() >= 1e-12
        })
        && p.pins.is_none_or(|pins| pin_mapping(pins).is_some())
}

#[cfg(test)]
mod pin_tests {
    use super::*;
    #[test]
    fn perspective_maps_all_four_corners_and_interior() {
        let pins = [[0.2, 0.3], [2., 0.1], [1.5, 1.8], [-0.3, 1.2]];
        let placement = TexturePlacement { pins: Some(pins), ..Default::default() };
        for (p, expected) in pins.into_iter().zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]]) {
            let actual = uv_at(DVec3::new(p[0], -p[1], 0.), DVec3::Z, [1., 1.], &placement);
            assert!((actual[0] - expected[0]).abs() < 1e-9 && (actual[1] - expected[1]).abs() < 1e-9);
        }
        let forward = pin_mapping(pins).unwrap().inverse();
        let p = forward * DVec3::new(0.35, 0.7, 1.);
        let actual = uv_at(DVec3::new(p.x / p.z, -p.y / p.z, 0.), DVec3::Z, [1., 1.], &placement);
        assert!((actual[0] - 0.35).abs() < 1e-9 && (actual[1] - 0.7).abs() < 1e-9);
        let d = DVec3::new(4., -2., 0.);
        assert_eq!(
            uv_at(d, DVec3::Z, [1., 1.], &moved(placement, DVec3::Z, d)),
            uv_at(DVec3::ZERO, DVec3::Z, [1., 1.], &placement)
        );
    }
    #[test]
    fn collapsed_crossed_and_nonfinite_pins_are_invalid() {
        assert!(pin_mapping([[0., 0.], [1., 1.], [1., 0.], [0., 1.]]).is_none());
        assert!(pin_mapping([[0., 0.]; 4]).is_none());
        assert!(pin_mapping([[f64::NAN, 0.], [1., 0.], [1., 1.], [0., 1.]]).is_none());
    }
}

#[cfg(test)]
mod command_pin_tests {
    use super::*;
    use crate::{Command, Editor};
    #[test]
    fn pin_edits_amend_atomically_and_undo_to_original() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let command = |pins| Command::SetTexturePlacement {
            faces: vec![face],
            placement: Some(TexturePlacement { pins: Some(pins), ..Default::default() }),
        };
        editor.apply(command([[0., 0.], [1., 0.], [1.2, 1.], [0., 1.]])).unwrap();
        let before = editor.doc().clone();
        let revision = editor.revision();
        assert!(editor.amend_last(command([[0., 0.]; 4])).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        editor.amend_last(command([[0., 0.], [1., 0.], [1.4, 1.], [0., 1.]])).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.mesh().face(face).unwrap().uv, None);
        assert!(editor.redo());
        assert_eq!(editor.mesh().face(face).unwrap().uv.unwrap().pins.unwrap()[2], [1.4, 1.]);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    #[test]
    fn projected_coordinates_are_shared_across_different_face_normals() {
        let placement = TexturePlacement { projected_normal: Some([0., 0., 1.]), ..Default::default() };
        let point = DVec3::new(0.2, 0.3, 4.);
        for normal in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::new(1., 2., 3.).normalize()] {
            assert_eq!(uv_at(point, normal, [1., 1.], &placement), [0.2, -0.3]);
        }
        let invalid = TexturePlacement { projected_normal: Some([0.; 3]), ..Default::default() };
        assert!(!valid_placement(&invalid));
    }
}

/// The point on a plane corresponding to an image UV coordinate.
pub fn point_at_uv(
    uv: [f64; 2],
    plane: rysmo_math::Plane,
    size: [f64; 2],
    placement: &TexturePlacement,
) -> Option<DVec3> {
    let uv = if let Some(m) = placement.pins.and_then(pin_mapping) {
        let q = m.inverse() * DVec3::new(uv[0], uv[1], 1.);
        if q.z.abs() < 1e-12 {
            return None;
        }
        [q.x / q.z, q.y / q.z]
    } else {
        uv
    };
    let (u, v) = texture_axes(placement.projected_normal.map(DVec3::from_array).unwrap_or(plane.normal));
    let q = DVec2::new(uv[0] * size[0] * placement.scale[0], -uv[1] * size[1] * placement.scale[1]);
    let (s, c) = placement.rotation.to_radians().sin_cos();
    let q = DVec2::new(q.x * c - q.y * s, q.x * s + q.y * c) + DVec2::from_array(placement.offset);
    let origin = u * q.x + v * q.y;
    let direction = u.cross(v);
    let denominator = plane.normal.dot(direction);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let point = origin + direction * ((plane.d - plane.normal.dot(origin)) / denominator);
    point.is_finite().then_some(point)
}

/// Carry texture coordinates with geometry through a context transform or unfolding.
pub fn transformed(
    plane: rysmo_math::Plane,
    size: [f64; 2],
    placement: TexturePlacement,
    transform: rysmo_math::DMat4,
) -> Option<TexturePlacement> {
    if [DVec3::X, DVec3::Y, DVec3::Z].iter().all(|axis| transform.transform_vector3(*axis).distance(*axis) < 1e-12) {
        return Some(moved(placement, plane.normal, transform.w_axis.truncate()));
    }
    let normal = transform.inverse().transpose().transform_vector3(plane.normal).try_normalize()?;
    let mut target = TexturePlacement {
        projected_normal: placement
            .projected_normal
            .map(|n| transform.transform_vector3(DVec3::from_array(n)).normalize().to_array()),
        ..Default::default()
    };
    let corners = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut pins = [[0.; 2]; 4];
    for i in 0..4 {
        let point = transform.transform_point3(point_at_uv(corners[i], plane, size, &placement)?);
        pins[i] = uv_at(point, normal, size, &target);
    }
    target.pins = Some(pins);
    valid_placement(&target).then_some(target)
}

/// Unfold selected adjacent faces around shared edges. Closed surfaces acquire a
/// deterministic seam where traversal paths meet; projected textures retain one frame.
pub fn wrapped_placements(
    mesh: &crate::Mesh,
    source: crate::FaceId,
    faces: &[crate::FaceId],
    size: [f64; 2],
    placement: TexturePlacement,
) -> Result<std::collections::BTreeMap<crate::FaceId, TexturePlacement>, String> {
    use rysmo_math::{DMat4, DQuat};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    if !valid_placement(&placement) || size.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("Invalid texture placement or image size.".into());
    }
    let mut allowed = faces.iter().copied().collect::<BTreeSet<_>>();
    allowed.insert(source);
    let mut adjacency: BTreeMap<crate::EdgeId, Vec<crate::FaceId>> = BTreeMap::new();
    for face in &allowed {
        if mesh.face(*face).is_none() {
            return Err("The source or target texture face is missing.".into());
        }
        for edge in mesh.face_edges(*face) {
            adjacency.entry(edge).or_default().push(*face);
        }
    }
    if placement.projected_normal.is_some() {
        return Ok(faces.iter().map(|f| (*f, placement)).collect());
    }
    fn inward(mesh: &crate::Mesh, face: crate::FaceId, edge: crate::EdgeId, axis: DVec3) -> DVec3 {
        let e = mesh.edge(edge).unwrap();
        let face_data = mesh.face(face).unwrap();
        let sign = if face_data.loop_edges().contains(&(e.v[0], e.v[1])) { 1. } else { -1. };
        mesh.face_normal(face).unwrap().cross(axis * sign).normalize()
    }
    let mut result = BTreeMap::from([(source, placement)]);
    let mut pending = VecDeque::from([source]);
    while let Some(face) = pending.pop_front() {
        let plane = mesh.face_plane(face).ok_or("A texture face is degenerate.")?;
        for edge in mesh.face_edges(face) {
            let (a, b) = mesh.edge_points(edge);
            let axis = (b - a).normalize();
            for target in &adjacency[&edge] {
                if result.contains_key(target) {
                    continue;
                }
                let from = inward(mesh, face, edge, axis);
                let to = -inward(mesh, *target, edge, axis);
                let angle = axis.dot(from.cross(to)).atan2(from.dot(to));
                let transform = DMat4::from_translation(a)
                    * DMat4::from_quat(DQuat::from_axis_angle(axis, angle))
                    * DMat4::from_translation(-a);
                let placement = transformed(plane, size, result[&face], transform)
                    .ok_or("Cannot wrap this texture through a degenerate projection.")?;
                result.insert(*target, placement);
                pending.push_back(*target);
            }
        }
    }
    Ok(faces.iter().map(|face| (*face, result.get(face).copied().unwrap_or(placement))).collect())
}

#[cfg(test)]
mod wrapping_tests {
    use super::*;
    use crate::{Command, Editor, Element};
    #[test]
    fn perpendicular_faces_wrap_continuously_regardless_of_face_winding() {
        for reverse in [false, true] {
            let mut e = Editor::new();
            e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
            let source = e.mesh().faces().next().unwrap().0;
            let out =
                e.apply(Command::DrawRectangle { corner: DVec3::X, u: DVec3::Y, v: DVec3::Z, facing: None }).unwrap();
            let target =
                out.created.iter().find_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).unwrap();
            if reverse {
                e.apply(Command::ReverseFaces { faces: vec![target] }).unwrap();
            }
            let wrapped =
                wrapped_placements(e.mesh(), source, &[source, target], [1., 1.], TexturePlacement::default()).unwrap();
            let uv = uv_at(DVec3::new(1., 0.3, 0.), e.mesh().face_normal(target).unwrap(), [1., 1.], &wrapped[&target]);
            assert!((uv[0] - 1.).abs() < 1e-8 && (uv[1] + 0.3).abs() < 1e-8);
            let uv =
                uv_at(DVec3::new(1., 0.3, 0.4), e.mesh().face_normal(target).unwrap(), [1., 1.], &wrapped[&target]);
            assert!((uv[0] - 1.4).abs() < 1e-8 && (uv[1] + 0.3).abs() < 1e-8);
        }
    }
    #[test]
    fn cylindrical_wrap_has_one_seam_and_preserves_other_shared_edges() {
        let mut e = Editor::new();
        e.apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 12, start: None })
            .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face, distance: -1., keep_base: false }).unwrap();
        let faces = e
            .mesh()
            .faces()
            .filter(|(f, _)| e.mesh().face_normal(*f).unwrap().z.abs() < 0.1)
            .map(|(f, _)| f)
            .collect::<Vec<_>>();
        assert_eq!(faces.len(), 12);
        let wrapped = wrapped_placements(e.mesh(), faces[0], &faces, [4., 1.], TexturePlacement::default()).unwrap();
        let mut joins = 0;
        let mut seams = 0;
        for (edge, _) in e.mesh().edges() {
            let adjacent =
                e.mesh().faces_of_edge(edge).into_iter().filter(|f| wrapped.contains_key(f)).collect::<Vec<_>>();
            if adjacent.len() != 2 {
                continue;
            }
            joins += 1;
            let (a, b) = e.mesh().edge_points(edge);
            let p = (a + b) * 0.5;
            let uv = adjacent
                .iter()
                .map(|f| uv_at(p, e.mesh().face_normal(*f).unwrap(), [4., 1.], &wrapped[f]))
                .collect::<Vec<_>>();
            if (uv[0][0] - uv[1][0]).abs() + (uv[0][1] - uv[1][1]).abs() > 1e-7 {
                seams += 1;
            }
        }
        assert_eq!(joins, 12);
        assert_eq!(seams, 1);
        let projected = TexturePlacement { projected_normal: Some([0., 1., 0.]), ..Default::default() };
        let result = wrapped_placements(e.mesh(), faces[0], &faces, [4., 1.], projected).unwrap();
        assert!(result.values().all(|p| *p == projected));
    }
    #[test]
    fn context_transform_preserves_projected_texture_coordinates() {
        let plane = rysmo_math::Plane::new(DVec3::Z, DVec3::new(0., 0., 2.));
        let placement = TexturePlacement {
            offset: [0.2, 0.5],
            rotation: 25.,
            projected_normal: Some([0., 0., 1.]),
            pins: Some([[0., 0.], [1., 0.], [1.2, 1.], [0., 1.]]),
            ..Default::default()
        };
        let transform = rysmo_math::DMat4::from_translation(DVec3::new(4., 3., 2.))
            * rysmo_math::DMat4::from_rotation_x(0.5)
            * rysmo_math::DMat4::from_scale(DVec3::new(2., 0.7, 1.3));
        let after = transformed(plane, [2., 1.], placement, transform).unwrap();
        let p = point_at_uv([0.3, 0.6], plane, [2., 1.], &placement).unwrap();
        let normal = transform.inverse().transpose().transform_vector3(plane.normal).normalize();
        let uv = uv_at(transform.transform_point3(p), normal, [2., 1.], &after);
        assert!((uv[0] - 0.3).abs() < 1e-8 && (uv[1] - 0.6).abs() < 1e-8);
    }
    #[test]
    fn wrapped_material_and_placement_are_one_atomic_history_step() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let source = e.mesh().faces().next().unwrap().0;
        let made =
            e.apply(Command::DrawRectangle { corner: DVec3::X, u: DVec3::Y, v: DVec3::Z, facing: None }).unwrap();
        let target = made.created.iter().find_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).unwrap();
        let image = e
            .apply(Command::AddImage { name: "Tile.png".into(), mime: "image/png".into(), data: vec![1] })
            .unwrap()
            .image
            .unwrap();
        let material = e
            .apply(Command::AddMaterial {
                name: "Tile".into(),
                color: [1.; 4],
                texture: Some(Texture { image, size: [1., 1.] }),
            })
            .unwrap()
            .material;
        let before = e.doc().clone();
        let depth = e.undo_depth();
        let command = |source| Command::PaintWrappedElements {
            elements: vec![Element::Face(source), Element::Face(target)],
            material,
            back: false,
            source,
            placement: TexturePlacement::default(),
        };
        e.apply(command(source)).unwrap();
        assert_eq!(e.undo_depth(), depth + 1);
        assert!(e.mesh().face(target).unwrap().uv.is_some());
        e.undo();
        assert_eq!(e.doc(), &before);
        let revision = e.revision();
        assert!(e.apply(command(crate::FaceId(u64::MAX))).is_err());
        assert_eq!(e.doc(), &before);
        assert_eq!(e.revision(), revision);
    }
}
