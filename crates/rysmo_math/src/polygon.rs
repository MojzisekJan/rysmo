//! Polygon area, orientation and containment.

use glam::{DVec2, DVec3};

/// The polygon normal by Newell's method, unit length, or `None` for a degenerate
/// polygon. Works for concave polygons and is stable for nearly collinear vertices.
pub fn newell_normal(points: &[DVec3]) -> Option<DVec3> {
    if points.len() < 3 {
        return None;
    }
    let mut n = DVec3::ZERO;
    for (i, a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    let len = n.length();
    (len > 1e-12).then(|| n / len)
}

/// Signed area of a 2D polygon: positive when counter-clockwise.
pub fn polygon_area_2d(points: &[DVec2]) -> f64 {
    let mut a = 0.0;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

/// Where a point lies relative to a polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolygonSide {
    Inside,
    Boundary,
    Outside,
}

/// Classifies a 2D point against a polygon by ray casting along +X, treating points
/// within `tol` of an edge as on the boundary.
///
/// Adapted from `polygon_contains_point_on_axis` in Heron Brush's
/// `geometry_math::intersection` (after TrenchBroom's VmLib): the point is moved to the
/// origin and each edge is classified against the positive X axis.
pub fn point_in_polygon_2d(p: DVec2, polygon: &[DVec2], tol: f64) -> PolygonSide {
    if polygon.len() < 3 {
        return PolygonSide::Outside;
    }
    let mut crossings = 0usize;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if crate::distance_point_segment_2d(p, *a, b) <= tol {
            return PolygonSide::Boundary;
        }
        let v0 = *a - p;
        let v1 = b - p;
        // Half-open rule on Y so that a vertex on the axis counts once.
        if (v0.y > 0.0) != (v1.y > 0.0) {
            let x = v0.x + (0.0 - v0.y) * (v1.x - v0.x) / (v1.y - v0.y);
            if x > 0.0 {
                crossings += 1;
            }
        }
    }
    if crossings % 2 == 1 { PolygonSide::Inside } else { PolygonSide::Outside }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Vec<DVec2> {
        vec![DVec2::new(0.0, 0.0), DVec2::new(2.0, 0.0), DVec2::new(2.0, 2.0), DVec2::new(0.0, 2.0)]
    }

    #[test]
    fn area_sign_follows_winding() {
        let mut s = square();
        assert_eq!(polygon_area_2d(&s), 4.0);
        s.reverse();
        assert_eq!(polygon_area_2d(&s), -4.0);
    }

    #[test]
    fn containment() {
        let s = square();
        assert_eq!(point_in_polygon_2d(DVec2::new(1.0, 1.0), &s, 1e-9), PolygonSide::Inside);
        assert_eq!(point_in_polygon_2d(DVec2::new(3.0, 1.0), &s, 1e-9), PolygonSide::Outside);
        assert_eq!(point_in_polygon_2d(DVec2::new(2.0, 1.0), &s, 1e-9), PolygonSide::Boundary);
        // A ray through a vertex is not counted twice.
        assert_eq!(point_in_polygon_2d(DVec2::new(-1.0, 2.0), &s, 1e-9), PolygonSide::Outside);
        assert_eq!(point_in_polygon_2d(DVec2::new(1.0, 0.0 + 1e-3), &s, 1e-9), PolygonSide::Inside);
    }

    #[test]
    fn newell_of_concave_l() {
        let l = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ];
        assert!((newell_normal(&l).unwrap() - DVec3::Z).length() < 1e-12);
        assert!(newell_normal(&[DVec3::ZERO, DVec3::X, DVec3::X * 2.0]).is_none());
    }
}
