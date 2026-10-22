//! Arc sampling and its geometric validation.

use rysmo_math::{DVec3, EPS};

/// Points of a 2-point arc, start to end.
pub fn arc_points(start: DVec3, end: DVec3, normal: DVec3, bulge: f64, segments: u32) -> Option<Vec<DVec3>> {
    let chord = end - start;
    let len = chord.length();
    if !start.is_finite()
        || !end.is_finite()
        || !normal.is_finite()
        || !bulge.is_finite()
        || len < EPS
        || bulge.abs() < EPS
        || segments < 1
        || normal.length() < EPS
    {
        return None;
    }
    let n = normal.normalize();
    let chord_dir = chord / len;
    let left = n.cross(chord_dir).normalize();
    let mid = (start + end) * 0.5;
    let r = (len * len / 4.0 + bulge * bulge) / (2.0 * bulge.abs());
    let centre = mid + left * (bulge - bulge.signum() * r);
    let theta = 4.0 * (2.0 * bulge.abs() / len).atan();
    let e1 = (start - centre) / r;
    let bulge_point = mid + left * bulge;
    let sweep = |axis: DVec3| -> Vec<DVec3> {
        let e2 = axis.cross(e1);
        (0..=segments)
            .map(|k| {
                let phi = theta * k as f64 / segments as f64;
                centre + (e1 * phi.cos() + e2 * phi.sin()) * r
            })
            .collect()
    };
    // Sweep round the normal or against it, whichever passes the bulge point.
    let halfway = centre + (e1 * (theta * 0.5).cos() + n.cross(e1) * (theta * 0.5).sin()) * r;
    let mut pts = if halfway.distance(bulge_point) < 1e-6 * (1.0 + r) { sweep(n) } else { sweep(-n) };
    // Pin the ends exactly.
    pts[0] = start;
    let last = pts.len() - 1;
    pts[last] = end;
    Some(pts)
}

/// Samples a signed arc strictly smaller than a full turn. Distances are metres.
pub fn centre_arc_points(
    centre: DVec3,
    normal: DVec3,
    start: DVec3,
    radius: f64,
    sweep: f64,
    segments: u32,
) -> Option<Vec<DVec3>> {
    if !centre.is_finite()
        || !normal.is_finite()
        || !start.is_finite()
        || !radius.is_finite()
        || !(radius * radius).is_finite()
        || radius <= EPS
        || !sweep.is_finite()
        || sweep.abs() < 1e-6
        || sweep.abs() >= 360.
        || !(2..=4096).contains(&segments)
    {
        return None;
    }
    let n = normal.try_normalize()?;
    let u = start.try_normalize()?;
    if n.dot(u).abs() > 1e-6 {
        return None;
    }
    let v = n.cross(u);
    Some(
        (0..=segments)
            .map(|i| {
                let t = (sweep * i as f64 / segments as f64).to_radians();
                centre + (u * t.cos() + v * t.sin()) * radius
            })
            .collect(),
    )
}
#[cfg(test)]
mod centre_arc_tests {
    use super::*;
    use crate::{Command, Editor};
    fn command(pie: bool, sweep: f64) -> Command {
        Command::DrawCentreArc {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 2.,
            sweep,
            segments: 24,
            pie,
        }
    }
    #[test]
    fn major_arc_keeps_analytic_identity_and_supports_radius_and_segment_edits() {
        let mut e = Editor::new();
        e.apply(command(false, 270.)).unwrap();
        assert_eq!(e.mesh().face_count(), 0);
        let (id, curve) = e.mesh().curves().next().unwrap();
        assert_eq!(curve.sweep, 270.);
        assert_eq!(curve.radius, 2.);
        assert_eq!(curve.kind, rysmo_mesh::CurveKind::Arc);
        assert_eq!(e.mesh().curve_edges(id).len(), 24);
        assert!(e.mesh().find_vertex(DVec3::new(0., -2., 0.)).is_some());
        e.apply(Command::SetCurveSegments { curve: id, segments: 32 }).unwrap();
        e.apply(Command::SetCurveRadius { curve: id, radius: 3. }).unwrap();
        assert_eq!(e.mesh().curve(id).unwrap().radius, 3.);
        let doc = e.doc().clone();
        e.undo();
        e.redo();
        assert_eq!(e.doc(), &doc);
    }
    #[test]
    fn pie_radii_are_not_arc_members_and_face_remains_editable() {
        let mut e = Editor::new();
        e.apply(command(true, 90.)).unwrap();
        let (id, _) = e.mesh().curves().next().unwrap();
        assert_eq!(e.mesh().curve_edges(id).len(), 24);
        assert_eq!(e.mesh().edges().filter(|(_, e)| e.curve.is_none()).count(), 2);
        assert_eq!(e.mesh().face_count(), 1);
        let face = e.mesh().faces().next().unwrap().0;
        let expected = 24. * 2. * (90_f64.to_radians() / 24.).sin();
        assert!((e.mesh().face_area(face) - expected).abs() < 1e-8);
        e.apply(Command::SetCurveSegments { curve: id, segments: 12 }).unwrap();
        e.apply(Command::SetCurveRadius { curve: id, radius: 3. }).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let expected = 12. * 4.5 * (90_f64.to_radians() / 12.).sin();
        assert!((e.mesh().face_area(face) - expected).abs() < 1e-8);
        e.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
        assert!(e.mesh().edges().all(|(id, _)| e.mesh().faces_of_edge(id).len() == 2));
    }
    #[test]
    fn clockwise_major_pie_is_one_face_and_keeps_signed_sweep() {
        let mut e = Editor::new();
        e.apply(command(true, -270.)).unwrap();
        assert_eq!(e.mesh().face_count(), 1);
        assert_eq!(e.mesh().curves().next().unwrap().1.sweep, -270.);
        let face = e.mesh().faces().next().unwrap().0;
        assert!((e.mesh().face_area(face) - 48. * (270_f64.to_radians() / 24.).sin()).abs() < 1e-8);
        let json = serde_json::to_string(&command(true, -270.)).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command(true, -270.));
    }
    #[test]
    fn invalid_centre_arcs_are_transactional() {
        let mut e = Editor::new();
        let doc = e.doc().clone();
        for cmd in [
            command(false, 0.),
            command(true, 360.),
            command(false, f64::NAN),
            Command::DrawCentreArc {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::Z,
                radius: 2.,
                sweep: 90.,
                segments: 24,
                pie: false,
            },
            Command::DrawCentreArc {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 0.,
                sweep: 90.,
                segments: 24,
                pie: true,
            },
        ] {
            assert!(e.apply(cmd).is_err());
            assert_eq!(e.doc(), &doc);
            assert_eq!(e.undo_depth(), 0);
        }
    }
}
