//! Analytic three-point arcs with an exact through vertex in their faceted mesh.
use rysmo_math::{DVec3, EPS};

#[derive(Clone, Copy, Debug)]
pub struct ArcParameters {
    pub centre: DVec3,
    pub normal: DVec3,
    pub start: DVec3,
    pub radius: f64,
    pub sweep: f64,
    pub through_fraction: f64,
}

/// The signed sweep follows `normal` and passes `through` strictly between the ends.
/// Translating before the circumcentre calculation avoids cancellation far from zero.
pub fn three_point_parameters(start: DVec3, through: DVec3, end: DVec3, normal: DVec3) -> Option<ArcParameters> {
    if !start.is_finite() || !through.is_finite() || !end.is_finite() {
        return None;
    }
    let n = normal.try_normalize()?;
    let a = through - start;
    let b = end - start;
    let span = a.length().max(b.length());
    if !span.is_finite() || a.length() <= EPS || b.length() <= EPS || through.distance(end) <= EPS {
        return None;
    }
    if a.dot(n).abs() > EPS * span.max(1.) || b.dot(n).abs() > EPS * span.max(1.) {
        return None;
    }
    let u = a.try_normalize()?;
    let v = n.cross(u).try_normalize()?;
    let x = b.dot(u);
    let y = b.dot(v);
    if y.abs() <= EPS * span.max(1.) {
        return None;
    }
    let offset = u * (a.length() * 0.5) + v * ((b.length_squared() - a.length() * x) / (2. * y));
    let centre = start + offset;
    let radius = offset.length();
    if !centre.is_finite() || !radius.is_finite() || radius <= EPS {
        return None;
    }
    let direction = (-offset).try_normalize()?;
    let side = n.cross(direction);
    let angle = |p: DVec3| {
        let d = (p - start) - offset;
        d.dot(side).atan2(d.dot(direction)).to_degrees().rem_euclid(360.)
    };
    let end_angle = angle(end);
    let middle_angle = angle(through);
    let sweep = if middle_angle < end_angle { end_angle } else { end_angle - 360. };
    let middle = if sweep > 0. { middle_angle } else { middle_angle - 360. };
    let through_fraction = middle / sweep;
    if !sweep.is_finite()
        || sweep.abs() < 1e-6
        || sweep.abs() >= 360.
        || through_fraction <= 0.
        || through_fraction >= 1.
    {
        return None;
    }
    Some(ArcParameters { centre, normal: n, start: direction, radius, sweep, through_fraction })
}

/// Keep the requested total segment count while sampling each side of the through
/// point separately. Editing the segment count later may redistribute free vertices.
pub fn three_point_points(
    start: DVec3,
    through: DVec3,
    end: DVec3,
    normal: DVec3,
    segments: u32,
) -> Option<(ArcParameters, Vec<DVec3>)> {
    if !(2..=4096).contains(&segments) {
        return None;
    }
    let arc = three_point_parameters(start, through, end, normal)?;
    let left = ((segments as f64 * arc.through_fraction).round() as u32).clamp(1, segments - 1);
    let right = segments - left;
    let side = arc.normal.cross(arc.start);
    let point = |t: f64| {
        let a = (arc.sweep * t).to_radians();
        arc.centre + (arc.start * a.cos() + side * a.sin()) * arc.radius
    };
    let mut points: Vec<_> = (0..=left).map(|i| point(arc.through_fraction * i as f64 / left as f64)).collect();
    points[0] = start;
    points[left as usize] = through;
    points.extend(
        (1..=right).map(|i| point(arc.through_fraction + (1. - arc.through_fraction) * i as f64 / right as f64)),
    );
    *points.last_mut()? = end;
    Some((arc, points))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor};
    fn draw(start: DVec3, through: DVec3, end: DVec3, normal: DVec3, segments: u32) -> Command {
        Command::DrawThreePointArc { start, through, end, normal, segments }
    }
    #[test]
    fn nonuniform_through_point_is_an_exact_mesh_vertex_without_changing_segment_count() {
        let b = DVec3::new(0.37_f64.cos(), 0.37_f64.sin(), 0.);
        let mut e = Editor::new();
        e.apply(draw(DVec3::X, b, -DVec3::X, DVec3::Z, 12)).unwrap();
        let v = e.mesh().find_vertex(b).unwrap();
        let (id, curve) = e.mesh().curves().next().unwrap();
        assert!((curve.radius - 1.).abs() < 1e-9);
        assert!((curve.sweep - 180.).abs() < 1e-8);
        assert_eq!(e.mesh().curve_edges(id).len(), 12);
        assert!(e.mesh().curve_edges(id).iter().any(|id| e.mesh().edge(*id).unwrap().v.contains(&v)));
        let before = e.doc().clone();
        e.undo();
        e.redo();
        assert_eq!(e.doc(), &before);
        let json = serde_json::to_string(&draw(DVec3::X, b, -DVec3::X, DVec3::Z, 12)).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), draw(DVec3::X, b, -DVec3::X, DVec3::Z, 12));
    }
    #[test]
    fn attached_through_vertex_survives_radius_edit_and_blocks_destructive_resampling() {
        let b = DVec3::new(0.37_f64.cos(), 0.37_f64.sin(), 0.);
        let mut e = Editor::new();
        e.apply(Command::DrawLines { lines: vec![[b, b + DVec3::Z]], facing: None }).unwrap();
        e.apply(draw(DVec3::X, b, -DVec3::X, DVec3::Z, 12)).unwrap();
        let id = e.mesh().curves().next().unwrap().0;
        let before = e.doc().clone();
        assert!(e.apply(Command::SetCurveSegments { curve: id, segments: 24 }).is_err());
        assert_eq!(e.doc(), &before);
        e.apply(Command::SetCurveRadius { curve: id, radius: 2. }).unwrap();
        assert!(e.mesh().find_vertex(b * 2.).is_some());
    }
    #[test]
    fn clockwise_major_and_vertical_arcs_fit_the_requested_points() {
        let a = DVec3::ZERO;
        let b = DVec3::new(1., 1., 0.);
        let c = DVec3::new(1., -1., 0.);
        let arc = three_point_parameters(a, b, c, DVec3::Z).unwrap();
        assert!((arc.sweep + 270.).abs() < 1e-8);
        for offset in [DVec3::ZERO, DVec3::splat(1e6)] {
            let arc = three_point_parameters(offset, offset + DVec3::new(1., 0., 1.), offset + DVec3::X * 2., DVec3::Y)
                .unwrap();
            assert!((arc.radius - 1.).abs() < 1e-8);
            assert!(arc.centre.distance(offset + DVec3::X) < 1e-8);
        }
    }
    #[test]
    fn invalid_three_point_arcs_leave_the_document_and_history_unchanged() {
        let mut e = Editor::new();
        let before = e.doc().clone();
        for cmd in [
            draw(DVec3::ZERO, DVec3::X, DVec3::X * 2., DVec3::Z, 12),
            draw(DVec3::ZERO, DVec3::ZERO, DVec3::X, DVec3::Z, 12),
            draw(DVec3::ZERO, DVec3::new(1., 1., 1.), DVec3::X * 2., DVec3::Z, 12),
            draw(DVec3::ZERO, DVec3::Y, DVec3::X, DVec3::Z, 1),
        ] {
            assert!(e.apply(cmd).is_err());
            assert_eq!(e.doc(), &before);
            assert_eq!(e.undo_depth(), 0);
        }
    }
}
