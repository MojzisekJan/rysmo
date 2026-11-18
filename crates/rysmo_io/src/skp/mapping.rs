//! Coordinate, transform and projective texture bridges. Both model spaces are Z-up metres.
use rysmo_math::{DMat3, DMat4, DVec2, DVec3, DVec4};
use rysmo_mesh::TexturePlacement;

const CORNERS: [[f64; 2]; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];

pub(super) fn transform(t: [f64; 13], scale: f64) -> Option<DMat4> {
    if t.iter().any(|v| !v.is_finite()) || (t[12] - 1.).abs() > 1e-9 {
        return None;
    }
    let factor = scale / skpwright::INCH;
    let m = DMat4::from_cols(
        DVec4::new(t[0], t[3], t[6], 0.),
        DVec4::new(t[1], t[4], t[7], 0.),
        DVec4::new(t[2], t[5], t[8], 0.),
        DVec4::new(t[9] * factor, t[10] * factor, t[11] * factor, 1.),
    );
    (m.is_finite() && m.inverse().is_finite()).then_some(m)
}

pub(super) fn stored_transform(m: DMat4) -> Option<[f64; 13]> {
    if !m.is_finite() || !m.inverse().is_finite() || (m.row(3) - DVec4::W).abs().max_element() > 1e-9 {
        return None;
    }
    let t = m.w_axis.truncate() * skpwright::INCH;
    Some([
        m.x_axis.x, m.y_axis.x, m.z_axis.x, m.x_axis.y, m.y_axis.y, m.z_axis.y, m.x_axis.z, m.y_axis.z, m.z_axis.z,
        t.x, t.y, t.z, 1.,
    ])
}

/// Material UVs are bottom-up in SKP and top-down in the renderer.
pub(super) fn placement(
    source: &skpwright::MeshFace,
    point: DVec3,
    size: [f64; 2],
    applied_inches: (f64, f64),
    scale: f64,
    side: skpwright::Side,
) -> Option<TexturePlacement> {
    let normal = DVec3::from_array(source.normal);
    let (u, v) = rysmo_model::texture_axes(normal);
    let (file_u, file_v) = frame(normal);
    let origin = normal * normal.dot(point);
    let inches = skpwright::INCH / scale;
    let local = DMat3::from_cols(
        DVec3::new(file_u.dot(u) * size[0] * inches, file_v.dot(u) * size[0] * inches, 0.),
        DVec3::new(-file_u.dot(v) * size[1] * inches, -file_v.dot(v) * size[1] * inches, 0.),
        DVec3::new(file_u.dot(origin) * inches, file_v.dot(origin) * inches, 1.),
    );
    let stored = match (&source.texture, side) {
        (Some(texture), skpwright::Side::Front) => texture.front,
        (Some(texture), skpwright::Side::Back) => texture.back,
        _ => [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    };
    let normalize = DMat3::from_cols(
        DVec3::new(1. / applied_inches.0, 0., 0.),
        DVec3::new(0., -1. / applied_inches.1, 0.),
        DVec3::new(0., 1., 1.),
    );
    let mapping = normalize * inverse(DMat3::from_cols_array(&stored))? * local;
    let anchor = mapping * DVec3::new(u.dot(point) / size[0], -v.dot(point) / size[1], 1.);
    let anchor = DVec2::new(anchor.x / anchor.z, anchor.y / anchor.z);
    if !anchor.is_finite() {
        return None;
    }
    let base = -anchor.floor();
    // Repeating textures permit integer UV rebasing. A valid projective map may
    // have a horizon through tile (0,0) even though the face's own tile is valid.
    let shifts = std::iter::once(DVec2::ZERO)
        .chain([-1., 0., 1.].into_iter().flat_map(|x| [-1., 0., 1.].into_iter().map(move |y| base + DVec2::new(x, y))));
    for shift in shifts {
        let rebase = DMat3::from_cols(DVec3::X, DVec3::Y, DVec3::new(shift.x, shift.y, 1.));
        let Some(backward) = inverse(rebase * mapping) else { continue };
        let pins = CORNERS.map(|[x, y]| {
            let p = backward * DVec3::new(x, y, 1.);
            [p.x / p.z, p.y / p.z]
        });
        if rysmo_model::pin_mapping(pins).is_some() {
            return Some(TexturePlacement { pins: Some(pins), ..Default::default() });
        }
    }
    None
}

fn inverse(matrix: DMat3) -> Option<DMat3> {
    if !matrix.is_finite() || matrix.determinant() == 0. {
        return None;
    }
    let inverse = matrix.inverse();
    inverse.is_finite().then_some(inverse)
}

fn frame(n: DVec3) -> (DVec3, DVec3) {
    let v = if n.z.abs() > 1. - 1e-9 { DVec3::Y } else { (DVec3::Z - n * n.z).normalize() };
    (v.cross(n), v)
}

pub(super) fn texture_matrix(
    normal: DVec3,
    point: DVec3,
    size: [f64; 2],
    placement: &TexturePlacement,
) -> Option<[f64; 9]> {
    let (u, v) = frame(normal);
    let origin = normal * normal.dot(point);
    let base = TexturePlacement { pins: None, ..*placement };
    let at =
        |x, y| DVec2::from_array(rysmo_model::uv_at(origin + (u * x + v * y) / skpwright::INCH, normal, size, &base));
    let zero = at(0., 0.);
    let x = at(1., 0.) - zero;
    let y = at(0., 1.) - zero;
    let local = DMat3::from_cols(x.extend(0.), y.extend(0.), zero.extend(1.));
    let pins = match placement.pins {
        Some(pins) => rysmo_model::pin_mapping(pins)?,
        None => DMat3::IDENTITY,
    };
    let (w, h) = (size[0] * skpwright::INCH, size[1] * skpwright::INCH);
    let texture_inches = DMat3::from_cols(DVec3::new(w, 0., 0.), DVec3::new(0., -h, 0.), DVec3::new(0., h, 1.));
    // Column-major coefficients are the file's row-vector array.
    Some(inverse(texture_inches * pins * local)?.to_cols_array())
}
