//! The editable drawing coordinate frame, independent of model geometry.
use crate::{Axis, DVec3, EPS, Plane};
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DrawingAxes {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
}
impl Default for DrawingAxes {
    fn default() -> Self {
        Self { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y }
    }
}
impl DrawingAxes {
    pub fn new(origin: DVec3, x: DVec3, y: DVec3) -> Option<Self> {
        if !origin.is_finite() || !x.is_finite() || !y.is_finite() || x.length() < EPS {
            return None;
        }
        let x = x.normalize();
        let y = y - x * y.dot(x);
        if y.length() < EPS {
            return None;
        }
        Some(Self { origin, x, y: y.normalize() })
    }
    pub fn basis(self) -> crate::DMat4 {
        crate::DMat4::from_cols(self.x.extend(0.0), self.y.extend(0.0), self.z().extend(0.0), crate::DVec4::W)
    }
    pub fn valid(self) -> bool {
        self.origin.is_finite()
            && self.x.is_finite()
            && self.y.is_finite()
            && (self.x.length() - 1.0).abs() < 1e-6
            && (self.y.length() - 1.0).abs() < 1e-6
            && self.x.dot(self.y).abs() < 1e-6
    }
    pub fn z(self) -> DVec3 {
        self.x.cross(self.y).normalize()
    }
    pub fn dir(self, axis: Axis) -> DVec3 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z(),
        }
    }
    pub fn plane(self) -> Plane {
        Plane::new(self.z(), self.origin)
    }
    pub fn rect_axes(self, plane: &Plane) -> (DVec3, DVec3) {
        let n = plane.normal;
        let axis =
            [self.x, self.y, self.z()].into_iter().min_by(|a, b| a.dot(n).abs().total_cmp(&b.dot(n).abs())).unwrap();
        let u = (axis - n * axis.dot(n)).normalize();
        (u, n.cross(u))
    }
}
