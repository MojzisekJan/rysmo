//! Geometry maths for Rysmo: tolerances, planes, rays, closest points, polygon tests
//! and the camera that both the core and the viewport use.
//!
//! Coordinates are `f64`, in metres, right-handed with Z up.
//!
//! Parts of this crate are adapted from the `geometry_math` crate of Heron Brush
//! (Jan Mojzisek), which in turn derives from TrenchBroom's VmLib (Kristian Duske, Eric
//! Wasylishen). Both are GPL-3.0-or-later; see `NOTICE.md` at the repository root. The
//! functions that were adapted say so in their comments.

#![forbid(unsafe_code)]

pub use glam::{DMat3, DMat4, DQuat, DVec2, DVec3, DVec4};

mod axes;
mod camera;
mod polygon;

pub use axes::DrawingAxes;
pub use camera::{OrbitCamera, StandardView, View};
pub use polygon::{PolygonSide, newell_normal, point_in_polygon_2d, polygon_area_2d};

/// Two points closer than this are the same point, in metres (one micrometre).
pub const EPS: f64 = 1e-6;

/// The three model axes, coloured red, green and blue in the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    pub fn dir(self) -> DVec3 {
        match self {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        }
    }
}

/// Two unit axes spanning the plane with normal `n`, lined up with the model axes where
/// possible: the first is the model axis least aligned with `n`, flattened into the
/// plane; the second completes a right-handed frame with `n`. Rectangles and textures
/// use these, so they line up with red, green and blue.
pub fn model_axes(n: DVec3) -> (DVec3, DVec3) {
    let n = n.normalize();
    // Equivalent normals produced by a transform and a polygon must choose the
    // same frame even when components differ only by floating-point roundoff.
    let least = n.abs().min_element();
    let axis =
        [DVec3::X, DVec3::Y, DVec3::Z].into_iter().find(|axis| axis.dot(n).abs() <= least + 1e-12).unwrap_or(DVec3::X);
    let u = (axis - n * axis.dot(n)).normalize();
    (u, n.cross(u))
}

/// Whether two unit directions are parallel or anti-parallel.
pub fn parallel(a: DVec3, b: DVec3) -> bool {
    a.cross(b).length_squared() < 1e-12
}

/// An infinite plane `normal . p = d` with a unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: DVec3,
    pub d: f64,
}

impl Plane {
    pub fn new(normal: DVec3, point: DVec3) -> Self {
        let normal = normal.normalize();
        Plane { normal, d: normal.dot(point) }
    }

    /// The plane through a polygon, oriented by its winding (counter-clockwise seen from
    /// the side the normal points to). `None` when the points are collinear.
    pub fn from_polygon(points: &[DVec3]) -> Option<Self> {
        let n = newell_normal(points)?;
        let centre = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        Some(Plane::new(n, centre))
    }

    pub fn distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.d
    }

    pub fn contains(&self, p: DVec3) -> bool {
        self.distance(p).abs() < EPS
    }

    pub fn project(&self, p: DVec3) -> DVec3 {
        p - self.normal * self.distance(p)
    }

    /// The same plane, whichever way it faces.
    pub fn same_surface(&self, other: &Plane) -> bool {
        if !parallel(self.normal, other.normal) {
            return false;
        }
        if self.normal.dot(other.normal) > 0.0 {
            (self.d - other.d).abs() < EPS
        } else {
            (self.d + other.d).abs() < EPS
        }
    }

    pub fn flipped(&self) -> Plane {
        Plane { normal: -self.normal, d: -self.d }
    }

    /// Two unit vectors spanning the plane, with `u x v = normal`. Points map to 2D
    /// coordinates in this basis so that counter-clockwise stays counter-clockwise.
    pub fn basis(&self) -> (DVec3, DVec3) {
        let n = self.normal;
        let helper = if n.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
        let u = helper.cross(n).normalize();
        let v = n.cross(u);
        (u, v)
    }

    pub fn to_2d(&self, p: DVec3) -> DVec2 {
        let (u, v) = self.basis();
        DVec2::new(u.dot(p), v.dot(p))
    }

    /// Distance along the ray to the plane, if the ray hits it going forward.
    pub fn intersect_ray(&self, ray: &Ray) -> Option<f64> {
        let denom = self.normal.dot(ray.dir);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (self.d - self.normal.dot(ray.origin)) / denom;
        (t >= 0.0).then_some(t)
    }
}

/// A half-line from `origin` along the unit vector `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

impl Ray {
    pub fn new(origin: DVec3, dir: DVec3) -> Self {
        Ray { origin, dir: dir.normalize() }
    }

    pub fn at(&self, t: f64) -> DVec3 {
        self.origin + self.dir * t
    }
}

/// The closest point to `p` on the segment `a..b`, and its parameter in `0..=1`.
pub fn closest_on_segment(a: DVec3, b: DVec3, p: DVec3) -> (DVec3, f64) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < EPS * EPS {
        return (a, 0.0);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (a + ab * t, t)
}

/// The closest points between the infinite line `origin + s * dir` and a ray.
///
/// Returns `(s, t)`: the parameter along the line and the distance along the ray, or
/// `None` when they are parallel. Adapted from `squared_distance_ray_segment` in Heron
/// Brush's `geometry_math::distance`, without the segment clamping.
pub fn closest_line_ray(origin: DVec3, dir: DVec3, ray: &Ray) -> Option<(f64, f64)> {
    let u = dir;
    let v = ray.dir;
    let w = origin - ray.origin;
    let a = u.dot(u);
    let b = u.dot(v);
    let c = v.dot(v);
    let d = u.dot(w);
    let e = v.dot(w);
    let denom = a * c - b * b;
    if denom.abs() < 1e-12 {
        return None;
    }
    let s = (b * e - c * d) / denom;
    let t = (a * e - b * d) / denom;
    Some((s, t.max(0.0)))
}

/// Where two segments cross: the parameters along `a0..a1` and `b0..b1` (each in
/// `0..=1`) if they pass within `EPS` of each other at a single point. Parallel segments
/// return `None`; the caller handles overlaps.
pub fn segment_intersection(a0: DVec3, a1: DVec3, b0: DVec3, b1: DVec3) -> Option<(f64, f64)> {
    let u = a1 - a0;
    let v = b1 - b0;
    let a = u.dot(u);
    let c = v.dot(v);
    if a < EPS * EPS || c < EPS * EPS || parallel(u / a.sqrt(), v / c.sqrt()) {
        return None;
    }
    let w = a0 - b0;
    let b = u.dot(v);
    let d = u.dot(w);
    let e = v.dot(w);
    let denom = a * c - b * b;
    let s = (b * e - c * d) / denom;
    let t = (a * e - b * d) / denom;
    let tol_s = EPS / a.sqrt();
    let tol_t = EPS / c.sqrt();
    if s < -tol_s || s > 1.0 + tol_s || t < -tol_t || t > 1.0 + tol_t {
        return None;
    }
    let s = s.clamp(0.0, 1.0);
    let t = t.clamp(0.0, 1.0);
    let pa = a0 + u * s;
    let pb = b0 + v * t;
    (pa.distance(pb) < EPS).then_some((s, t))
}

/// Distance from a 2D point to a 2D segment.
pub fn distance_point_segment_2d(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-18 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_from_square_faces_up() {
        let pts = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        let p = Plane::from_polygon(&pts).unwrap();
        assert!((p.normal - DVec3::Z).length() < 1e-12);
        assert!(p.contains(DVec3::new(5.0, 5.0, 0.0)));
        assert!(!p.contains(DVec3::new(0.0, 0.0, 0.01)));
    }

    #[test]
    fn basis_keeps_orientation() {
        let p = Plane::new(DVec3::new(0.3, -0.5, 0.8), DVec3::ZERO);
        let (u, v) = p.basis();
        assert!((u.cross(v) - p.normal).length() < 1e-12);
    }

    #[test]
    fn same_surface_ignores_facing() {
        let a = Plane::new(DVec3::Z, DVec3::new(0.0, 0.0, 2.0));
        assert!(a.same_surface(&a.flipped()));
        assert!(!a.same_surface(&Plane::new(DVec3::Z, DVec3::ZERO)));
    }

    #[test]
    fn crossing_segments_meet() {
        let (s, t) = segment_intersection(
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(1.0, -1.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
        )
        .unwrap();
        assert!((s - 0.5).abs() < 1e-12 && (t - 0.5).abs() < 1e-12);
        assert!(segment_intersection(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(1.0, 1.0, 0.0)).is_none());
        // Skew segments that miss each other.
        assert!(
            segment_intersection(DVec3::ZERO, DVec3::X, DVec3::new(0.5, -1.0, 0.1), DVec3::new(0.5, 1.0, 0.1))
                .is_none()
        );
    }

    #[test]
    fn line_ray_closest() {
        let ray = Ray::new(DVec3::new(0.0, -5.0, 1.0), DVec3::Y);
        let (s, t) = closest_line_ray(DVec3::ZERO, DVec3::X, &ray).unwrap();
        assert!(s.abs() < 1e-12 && (t - 5.0).abs() < 1e-12);
    }
}
