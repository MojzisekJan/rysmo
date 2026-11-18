use rysmo_math::{DVec3, DrawingAxes, OrbitCamera};
use rysmo_mesh::GuideId;
use rysmo_model::{Definition, Document, Guide, Units};
use std::collections::BTreeSet;

pub(super) fn camera(value: &skpwright::Camera, scale: f64) -> Option<OrbitCamera> {
    let target = DVec3::from_array(value.target_m) * scale;
    let delta = (DVec3::from_array(value.eye_m) - DVec3::from_array(value.target_m)) * scale;
    let distance = delta.length();
    if !distance.is_finite() || distance < 1e-9 {
        return None;
    }
    let direction = delta / distance;
    let fov_y = value.fov_deg.to_radians();
    let mut result = OrbitCamera {
        target,
        distance,
        yaw: direction.y.atan2(direction.x),
        pitch: direction.z.clamp(-1., 1.).asin(),
        fov_y,
        parallel: !value.perspective,
        ..Default::default()
    };
    let up = DVec3::from_array(value.up).try_normalize()?;
    let base = result.up();
    result.roll = up.dot((-direction).cross(base)).atan2(up.dot(base));
    if result.parallel {
        result.distance = value.parallel_height_m * scale / (2. * (result.fov_y / 2.).tan());
    }
    result.valid().then_some(result)
}

pub(super) fn stored_camera(camera: OrbitCamera) -> skpwright::Camera {
    skpwright::Camera {
        eye_m: camera.eye().to_array(),
        target_m: camera.target.to_array(),
        up: camera.up().to_array(),
        perspective: !camera.parallel,
        fov_deg: camera.fov_y.to_degrees(),
        parallel_height_m: 2. * camera.distance * (camera.fov_y / 2.).tan(),
        two_point_perspective: false,
        description: String::new(),
        undecoded: [1., 1000.],
        aspect_ratio: 0.,
        scale_2d: 1.,
        offset_2d: [0.; 2],
        image_width: 0.,
    }
}

pub(super) fn read_run(
    doc: &mut Document,
    definition: &mut Definition,
    guides: &[&skpwright::Guide],
    hidden_layers: &BTreeSet<u16>,
    run: &skpwright::GeometryRun,
    scale: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<(), String> {
    for guide in guides {
        let value = Guide::Line {
            origin: DVec3::from_array(guide.point_m) * scale,
            dir: DVec3::from_array(guide.direction),
            hidden: guide.hidden || hidden_layers.contains(&guide.layer),
        };
        if !value.valid() {
            return Err("Invalid SKP construction guide.".into());
        }
        definition.guides.insert(GuideId(doc.reserve_id()), value);
    }
    for point in &run.guide_points {
        let value = Guide::Point {
            p: DVec3::from_array(point.point_m) * scale,
            origin: Some(DVec3::from_array(point.anchor_m) * scale),
            hidden: point.hidden || hidden_layers.contains(&point.layer),
        };
        if !value.valid() {
            return Err("Invalid SKP guide point.".into());
        }
        definition.guides.insert(GuideId(doc.reserve_id()), value);
    }
    for section in &run.sections {
        let normal = DVec3::new(section.plane[0], section.plane[1], section.plane[2]);
        let origin = normal * (-section.plane[3] / skpwright::INCH * scale);
        let value = rysmo_model::sections::SectionPlane { name: section.name.clone(), normal, origin };
        if !value.valid() {
            return Err("Invalid SKP section plane.".into());
        }
        let id = doc.reserve_id();
        definition.sections.planes.insert(id, value);
        if run.active_section == Some(section.pid) {
            definition.sections.active = Some(id);
        }
    }
    if !run.polylines.is_empty() || !run.images.is_empty() {
        warnings.insert("Placed images and decorative 3D polylines are not imported.".into());
    }
    Ok(())
}

fn units(unit: u32) -> Units {
    match unit {
        0 => Units::Inches,
        1 => Units::Feet,
        2 => Units::Millimetres,
        3 => Units::Centimetres,
        _ => Units::Metres,
    }
}
pub(super) fn unit_code(unit: Units) -> u32 {
    match unit {
        Units::Inches | Units::Architectural | Units::Fractional => 0,
        Units::Feet => 1,
        Units::Millimetres => 2,
        Units::Centimetres => 3,
        Units::Metres => 4,
    }
}

pub(super) fn read_document(
    doc: &mut Document,
    source: &skpwright::Model,
    scale: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<(), String> {
    let unit = &source.units;
    doc.units = match unit.length_format {
        1 => Units::Architectural,
        3 => Units::Fractional,
        _ => units(unit.length_unit),
    };
    doc.length_precision = Some(unit.length_precision.min(6) as u8);
    doc.measurement.show_unit_markers = !unit.suppress_units_display;
    doc.measurement.force_zero_inches = unit.force_inch_display;
    doc.measurement.area_units = unit.area_unit.filter(|&code| code <= 4).map(units);
    doc.measurement.volume_units = unit.volume_unit.filter(|&code| code <= 4).map(units);
    doc.measurement.area_precision = unit.area_precision.unwrap_or(3).min(6) as u8;
    doc.measurement.volume_precision = unit.volume_precision.unwrap_or(3).min(6) as u8;
    if unit.area_unit.is_some_and(|code| code > 4) || unit.volume_unit.is_some_and(|code| code > 4) {
        warnings.insert("Unsupported area or volume display units use the model's units.".into());
    }
    doc.measurement.angle_precision = unit.angle_precision.min(6) as u8;
    doc.measurement.angle_snap = unit.angle_snap.then_some(unit.snap_angle);
    doc.measurement.length_snap =
        unit.length_snap.then_some(unit.length_snap_length / units(unit.length_unit).per_metre() * scale);
    if !doc.measurement.valid() {
        return Err("Invalid SKP measurement settings.".into());
    }
    if let Some(rendering) = &source.rendering {
        read_style(&mut doc.style, rendering);
    }
    if let Some(value) = &source.camera {
        doc.camera = Some(camera(value, scale).ok_or("Invalid SKP camera.")?);
        if value.two_point_perspective {
            warnings.insert("Two-point perspective is imported as a regular camera.".into());
        }
    }
    if let Some(axes) = &source.axes {
        doc.root_mut().axes = DrawingAxes::new(
            DVec3::from_array(axes.origin_m) * scale,
            DVec3::from_array(axes.x),
            DVec3::from_array(axes.y),
        )
        .ok_or("Invalid SKP model axes.")?;
    }
    for value in &source.scenes {
        let view = value.camera.as_ref().and_then(|c| camera(c, scale)).or(doc.camera).unwrap_or_default();
        let mut scene = rysmo_model::scenes::Scene::capture(doc, view, value.name.clone());
        scene.id = doc.reserve_id();
        scene.description = value.description.clone();
        scene.include_in_animation = value.in_animation;
        scene.properties = rysmo_model::scenes::SceneProperties {
            camera: value.saved.has(skpwright::SceneProperties::CAMERA) && value.camera.is_some(),
            style: value.saved.has(skpwright::SceneProperties::STYLE_AND_FOG) && value.rendering.is_some(),
            hidden_geometry: false,
            tags: false,
            sections: false,
            shadows: false,
            axes: false,
        };
        if let Some(rendering) = &value.rendering {
            read_style(&mut scene.style, rendering);
        }
        doc.scenes.push(scene);
    }
    if !source.scenes.is_empty() {
        warnings.insert(
            "Scene cameras and basic appearance are imported; per-scene visibility and section overrides are not converted."
                .into(),
        );
    }
    if !source.texts.is_empty() || !source.dimensions.is_empty() || !source.radial_dimensions.is_empty() {
        warnings.insert("Text and dimension annotations are not yet converted.".into());
    }
    Ok(())
}

pub(super) fn read_style(style: &mut rysmo_model::Style, source: &skpwright::RenderingOptions) {
    use rysmo_model::{EdgeColor, FaceStyle};
    style.faces = if source.xray {
        FaceStyle::XRay
    } else {
        match source.face_style {
            0 => FaceStyle::Wireframe,
            1 => FaceStyle::HiddenLine,
            5 => FaceStyle::Monochrome,
            _ => {
                if source.textures {
                    FaceStyle::Textured
                } else {
                    FaceStyle::Shaded
                }
            }
        }
    };
    style.edges = source.edges;
    style.profiles = source.profiles;
    style.edge_color = match source.edge_color_mode {
        0 => EdgeColor::ByMaterial,
        2 => EdgeColor::ByAxis,
        _ => EdgeColor::Black,
    };
    let rgb = |rgba: [u8; 4]| [f32::from(rgba[0]) / 255., f32::from(rgba[1]) / 255., f32::from(rgba[2]) / 255.];
    style.background = rgb(source.background);
    style.sky = source.sky;
    style.sky_color = rgb(source.sky_color);
    style.ground = source.ground;
    style.ground_color = rgb(source.ground_color);
    style.show_hidden_geometry = source.hidden_geometry;
    style.show_guides = !source.guides_hidden;
    style.section_cuts = source.section_cuts();
    style.section_planes = source.section_planes();
}

pub(super) fn stored_style(style: rysmo_model::Style) -> skpwright::RenderingOptions {
    use rysmo_model::{EdgeColor, FaceStyle};
    let rgba = |rgb: [f32; 3]| {
        [
            (rgb[0].clamp(0., 1.) * 255.).round() as u8,
            (rgb[1].clamp(0., 1.) * 255.).round() as u8,
            (rgb[2].clamp(0., 1.) * 255.).round() as u8,
            255,
        ]
    };
    let mut result = skpwright::RenderingOptions::sketchup_default();
    result.face_style = match style.faces {
        FaceStyle::Wireframe => 0,
        FaceStyle::HiddenLine => 1,
        FaceStyle::Monochrome => 5,
        _ => 2,
    };
    result.xray = style.faces == FaceStyle::XRay;
    result.textures = style.faces == FaceStyle::Textured;
    result.edges = style.edges;
    result.profiles = style.profiles;
    result.edge_color_mode = match style.edge_color {
        EdgeColor::ByMaterial => 0,
        EdgeColor::Black => 1,
        EdgeColor::ByAxis => 2,
    };
    result.background = rgba(style.background);
    result.sky = style.sky;
    result.sky_color = rgba(style.sky_color);
    result.ground = style.ground;
    result.ground_color = rgba(style.ground_color);
    result.hidden_geometry = style.show_hidden_geometry;
    result.guides_hidden = !style.show_guides;
    result.section_display = u32::from(style.section_planes) | (u32::from(style.section_cuts) * 2);
    result
}
