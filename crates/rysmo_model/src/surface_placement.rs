//! Rigid world-space alignment of clipboard geometry to a picked surface.
use rysmo_math::{DMat4, DQuat, DVec3, DVec4};

pub struct SurfacePlacement {
    pub point: DVec3,
    pub normal: DVec3,
    pub angle_degrees: f64,
    pub offset: f64,
    pub flip: bool,
}
impl SurfacePlacement {
    /// `anchor` is the insertion frame inside clipboard coordinates.
    pub fn transform(&self, source: DMat4, context: DMat4, anchor: DMat4) -> Result<DMat4, String> {
        if !self.point.is_finite()
            || !self.normal.is_finite()
            || self.normal.length() < 1e-12
            || !self.angle_degrees.is_finite()
            || !self.offset.is_finite()
            || !source.is_finite()
            || !context.is_finite()
            || !anchor.is_finite()
            || !source.inverse().is_finite()
            || !context.inverse().is_finite()
            || !anchor.inverse().is_finite()
        {
            return Err("Surface placement needs finite, invertible frames and a nonzero normal".into());
        }
        let mut source = source;
        source.w_axis = DVec4::W;
        let normal = self.normal.normalize() * if self.flip { -1. } else { 1. };
        let reference = (source * anchor).inverse().transpose().transform_vector3(DVec3::Z).normalize();
        let rotation = DQuat::from_axis_angle(normal, self.angle_degrees.to_radians())
            * DQuat::from_rotation_arc(reference, normal);
        let aligned = DMat4::from_quat(rotation) * source;
        let origin = self.point + normal * self.offset - aligned.transform_vector3(anchor.w_axis.truncate());
        let result = context.inverse() * DMat4::from_translation(origin) * aligned;
        if !result.is_finite() || !result.inverse().is_finite() {
            return Err("Surface placement produced an invalid transform".into());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_faces_preserve_world_shape_and_anchor_in_scaled_context() {
        let source = DMat4::from_scale_rotation_translation(
            DVec3::new(2., 0.5, 1.3),
            DQuat::from_rotation_z(0.4),
            DVec3::splat(100.),
        );
        let anchor = DMat4::from_scale_rotation_translation(
            DVec3::new(1., 2., 1.),
            DQuat::from_rotation_x(0.3),
            DVec3::new(3., 2., 1.),
        );
        let context = DMat4::from_scale_rotation_translation(
            DVec3::new(-2., 3., 0.7),
            DQuat::from_rotation_y(0.5),
            DVec3::splat(7.),
        );
        for normal in [DVec3::X, DVec3::Y, DVec3::Z, -DVec3::Z, DVec3::new(1., 2., 3.).normalize()] {
            for flip in [false, true] {
                let placement =
                    SurfacePlacement { point: DVec3::new(4., 5., 6.), normal, angle_degrees: 37., offset: 0.25, flip };
                let transform = placement.transform(source, context, anchor).unwrap();
                let world = context * transform * anchor;
                let target = normal * if flip { -1. } else { 1. };
                assert!(world.transform_point3(DVec3::ZERO).distance(placement.point + target * 0.25) < 1e-10);
                assert!(world.inverse().transpose().transform_vector3(DVec3::Z).normalize().dot(target) > 1. - 1e-10);
                for vector in [DVec3::X, DVec3::Y, DVec3::Z, DVec3::new(1., 2., 3.)] {
                    assert!(
                        (world.transform_vector3(vector).length()
                            - (source * anchor).transform_vector3(vector).length())
                        .abs()
                            < 1e-10
                    );
                }
            }
        }
    }
    #[test]
    fn twist_and_invalid_inputs_are_deterministic() {
        let mut placement =
            SurfacePlacement { point: DVec3::ZERO, normal: DVec3::Z, angle_degrees: 90., offset: 0., flip: false };
        let transform = placement.transform(DMat4::IDENTITY, DMat4::IDENTITY, DMat4::IDENTITY).unwrap();
        assert!(transform.transform_vector3(DVec3::X).distance(DVec3::Y) < 1e-10);
        placement.normal = DVec3::ZERO;
        assert!(placement.transform(DMat4::IDENTITY, DMat4::IDENTITY, DMat4::IDENTITY).is_err());
        placement.normal = DVec3::Z;
        assert!(placement.transform(DMat4::IDENTITY, DMat4::ZERO, DMat4::IDENTITY).is_err());
        placement.offset = f64::NAN;
        assert!(placement.transform(DMat4::IDENTITY, DMat4::IDENTITY, DMat4::IDENTITY).is_err());
    }
}
