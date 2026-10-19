//! The style saved with the model controls how faces and edges are
//! drawn, the background, and the light (sun, shadows) and render quality.

use rysmo_math::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceStyle {
    /// Material colours and textures.
    #[default]
    Textured,
    /// Material colours, no textures.
    Shaded,
    /// The default front and back colours everywhere.
    Monochrome,
    /// Faces in the background colour: only edges show, hidden ones hidden.
    HiddenLine,
    /// No faces at all.
    Wireframe,
    /// Faces see-through.
    XRay,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeColor {
    #[default]
    Black,
    /// The colour of a face beside the edge.
    ByMaterial,
    /// Red, green or blue for edges along an axis, black otherwise.
    ByAxis,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lighting {
    /// Default look: faces shaded by a fixed light, no light sources.
    #[default]
    Flat,
    /// Lit by the sun (with shadows if they are on) and the sky.
    Sun,
    Studio,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioEnvironment {
    #[default]
    Neutral,
    Warm,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToneMapping {
    /// Colours as they are; right for the flat look.
    #[default]
    None,
    AgX,
    TonyMcMapface,
    AcesFitted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiAliasing {
    Off,
    #[default]
    Msaa4,
    Fxaa,
    Taa,
}

/// Where the sun is: from a date, time and place (the Shadows panel), or set
/// directly.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sun {
    /// Use the date, time and latitude; otherwise `azimuth` and `elevation`.
    pub from_time: bool,
    /// Day of the year, 1 to 365.
    pub day: u16,
    /// Local solar time, hours (13.5 is half past one).
    pub hour: f64,
    /// Degrees, north positive.
    pub latitude: f64,
    /// Degrees the model's north (+Y, green) is turned from true north, clockwise.
    pub north: f64,
    /// Degrees clockwise from north, when set directly.
    pub azimuth: f64,
    /// Degrees above the horizon, when set directly.
    pub elevation: f64,
}

impl Default for Sun {
    fn default() -> Self {
        // Prague, early afternoon in late spring.
        Sun { from_time: true, day: 150, hour: 13.5, latitude: 50.1, north: 0.0, azimuth: 200.0, elevation: 45.0 }
    }
}

impl Sun {
    /// Azimuth (degrees clockwise from true north) and elevation (degrees) of the sun.
    pub fn position(&self) -> (f64, f64) {
        if !self.from_time {
            return (self.azimuth, self.elevation);
        }
        let decl = 23.44f64.to_radians() * ((360.0 / 365.0) * (284.0 + self.day as f64)).to_radians().sin();
        let hour_angle = (15.0 * (self.hour - 12.0)).to_radians();
        let lat = self.latitude.to_radians();
        let elevation = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * hour_angle.cos()).asin();
        let azimuth =
            hour_angle.sin().atan2(hour_angle.cos() * lat.sin() - decl.tan() * lat.cos()).to_degrees() + 180.0;
        (azimuth.rem_euclid(360.0), elevation.to_degrees())
    }

    /// The direction towards the sun in model coordinates (Z up, green is north turned by
    /// `north`).
    pub fn direction(&self) -> DVec3 {
        let (az, el) = self.position();
        let az = (az - self.north).to_radians();
        let el = el.to_radians();
        DVec3::new(az.sin() * el.cos(), az.cos() * el.cos(), el.sin())
    }
}

/// Shadow-map resolution and the number of distance cascades.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowQuality {
    Low,
    Medium,
    #[default]
    High,
}
impl ShadowQuality {
    pub fn map_size(self) -> usize {
        match self {
            Self::Low => 512,
            Self::Medium => 1024,
            Self::High => 2048,
        }
    }
    pub fn cascades(self) -> usize {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
        }
    }
}

/// Everything about how the model is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub show_hidden_geometry: bool,
    pub show_guides: bool,
    pub section_cuts: bool,
    pub section_planes: bool,
    pub section_lines: bool,
    pub section_color: [f32; 3],
    pub section_active_color: [f32; 3],
    pub section_inactive_color: [f32; 3],
    pub section_width: f32,
    pub faces: FaceStyle,
    pub edges: bool,
    /// Thicker outlines where a surface turns away from the view.
    pub profiles: bool,
    pub edge_color: EdgeColor,
    /// sRGB 0..1.
    pub background: [f32; 3],
    pub sky: bool,
    pub sky_color: [f32; 3],
    pub ground: bool,
    pub ground_color: [f32; 3],
    /// Haze that fades distant geometry into the horizon colour.
    pub fog: bool,
    /// Distance, in metres, at which the fog hides geometry almost completely.
    pub fog_distance: f32,
    /// A physically based sky and aerial haze lit by the sun (desktop renderer, Sun lighting).
    pub atmosphere: bool,
    /// How see-through faces are in X-ray, 0..1.
    pub xray_opacity: f32,
    pub lighting: Lighting,
    pub shadows: bool,
    pub shadow_quality: ShadowQuality,
    /// Multipliers of the default sun and ambient illumination, 0..4.
    pub sun_intensity: f32,
    pub ambient_intensity: f32,
    pub studio_environment: StudioEnvironment,
    pub studio_intensity: f32,
    pub studio_rotation: f32,
    /// Exposure compensation in stops; positive brightens lit views.
    pub exposure: f32,
    /// Zero retains the original texture resolution.
    pub texture_limit: u32,
    pub texture_anisotropy: u16,
    pub render_scale: f32,
    pub sun: Sun,
    pub ambient_occlusion: bool,
    pub tone_mapping: ToneMapping,
    pub anti_aliasing: AntiAliasing,
    pub dimensions: crate::dimensions::DimensionVisibility,
    pub text_defaults: crate::text::TextDefaults,
    pub dimension_defaults: crate::dimensions::DimensionDefaults,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            show_hidden_geometry: false,
            show_guides: true,
            section_cuts: true,
            section_planes: true,
            section_lines: true,
            section_color: [0.65, 0.15, 0.05],
            section_active_color: [0.9, 0.45, 0.05],
            section_inactive_color: [0.6, 0.6, 0.6],
            section_width: 3.4,
            faces: FaceStyle::Textured,
            edges: true,
            profiles: false,
            edge_color: EdgeColor::Black,
            background: [1.0, 1.0, 1.0],
            sky: true,
            sky_color: [0.84, 0.89, 0.94],
            ground: false,
            ground_color: [0.82, 0.80, 0.74],
            fog: false,
            fog_distance: 400.,
            atmosphere: false,
            xray_opacity: 0.5,
            lighting: Lighting::Flat,
            shadows: false,
            shadow_quality: ShadowQuality::High,
            sun_intensity: 1.,
            ambient_intensity: 1.,
            studio_environment: StudioEnvironment::Neutral,
            studio_intensity: 1.,
            studio_rotation: 0.,
            exposure: 0.,
            texture_limit: 4096,
            texture_anisotropy: 4,
            render_scale: 1.,
            sun: Sun::default(),
            ambient_occlusion: false,
            tone_mapping: ToneMapping::None,
            anti_aliasing: AntiAliasing::Msaa4,
            dimensions: Default::default(),
            dimension_defaults: Default::default(),
            text_defaults: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_light_strength_is_rejected_without_changing_history() {
        let mut editor = crate::Editor::new();
        let before = editor.doc().clone();
        for intensity in [-1., 4.1, f32::NAN, f32::INFINITY] {
            let style = Style { sun_intensity: intensity, ..Default::default() };
            assert!(editor.apply(crate::Command::SetStyle { style }).is_err());
            assert_eq!(editor.doc(), &before);
            assert!(editor.undo_label().is_none());
        }
        let style = Style { sun_intensity: 0., ambient_intensity: 4., ..Default::default() };
        editor.apply(crate::Command::SetStyle { style }).unwrap();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc().style, style);
    }
    #[test]
    fn equinox_noon_sun_is_due_south_at_ninety_minus_latitude() {
        let sun = Sun { from_time: true, day: 80, hour: 12.0, latitude: 50.0, ..Default::default() };
        let (az, el) = sun.position();
        assert!((az - 180.0).abs() < 1.0, "azimuth {az}");
        assert!((el - 40.0).abs() < 1.5, "elevation {el}");
        let d = sun.direction();
        assert!(d.y < -0.5 && d.z > 0.5, "towards the south (-Y) and up: {d:?}");
    }

    #[test]
    fn morning_sun_is_in_the_east_and_night_is_below() {
        let morning = Sun { from_time: true, day: 172, hour: 8.0, latitude: 50.0, ..Default::default() };
        let (az, el) = morning.position();
        assert!(az > 60.0 && az < 120.0 && el > 10.0, "{az} {el}");
        assert!(morning.direction().x > 0.5);
        let night = Sun { hour: 0.0, ..morning };
        assert!(night.position().1 < 0.0);
    }

    #[test]
    fn north_angle_turns_the_sun() {
        let s = Sun { from_time: false, azimuth: 90.0, elevation: 0.0, north: 90.0, ..Default::default() };
        let d = s.direction();
        assert!((d - DVec3::new(0.0, 1.0, 0.0)).length() < 1e-9, "{d:?}");
    }

    #[test]
    fn style_json_fills_missing_fields() {
        let s: Style = serde_json::from_str(r#"{"faces":"x_ray","shadows":true}"#).unwrap();
        assert_eq!(s.faces, FaceStyle::XRay);
        assert!(s.shadows && s.edges);
    }
}

impl Style {
    pub fn copy_lighting_from(&mut self, other: &Self) {
        self.shadows = other.shadows;
        self.sun = other.sun;
        self.lighting = other.lighting;
        self.shadow_quality = other.shadow_quality;
        self.sun_intensity = other.sun_intensity;
        self.ambient_intensity = other.ambient_intensity;
        self.studio_environment = other.studio_environment;
        self.studio_intensity = other.studio_intensity;
        self.studio_rotation = other.studio_rotation;
        self.exposure = other.exposure;
    }
    pub fn valid_render(&self) -> bool {
        [self.sun_intensity, self.ambient_intensity, self.studio_intensity]
            .into_iter()
            .all(|v| v.is_finite() && (0. ..=4.).contains(&v))
            && self.studio_rotation.is_finite()
            && (-180. ..=180.).contains(&self.studio_rotation)
            && self.exposure.is_finite()
            && (-4. ..=4.).contains(&self.exposure)
            && self.fog_distance.is_finite()
            && (10. ..=100_000.).contains(&self.fog_distance)
            && self.render_scale.is_finite()
            && (0.5..=1.5).contains(&self.render_scale)
            && [0, 512, 1024, 2048, 4096, 8192].contains(&self.texture_limit)
            && [1, 2, 4, 8, 16].contains(&self.texture_anisotropy)
    }
    pub fn valid_sections(&self) -> bool {
        self.section_width.is_finite()
            && self.section_width > 0.
            && self.section_width <= 100.
            && [self.section_color, self.section_active_color, self.section_inactive_color]
                .into_iter()
                .flatten()
                .all(|c| c.is_finite() && (0. ..=1.).contains(&c))
    }
}

#[cfg(test)]
mod render_quality_tests {
    use super::*;
    #[test]
    fn extended_render_controls_validate_and_undo_atomically() {
        let mut editor = crate::Editor::new();
        let before = editor.doc().clone();
        let valid = Style {
            lighting: Lighting::Studio,
            studio_environment: StudioEnvironment::Warm,
            studio_rotation: 123.,
            studio_intensity: 2.,
            exposure: 1.5,
            render_scale: 0.75,
            texture_limit: 1024,
            texture_anisotropy: 8,
            ..Default::default()
        };
        editor.apply(crate::Command::SetStyle { style: valid }).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        assert_eq!(editor.doc().style, valid);
        let depth = editor.undo_depth();
        for invalid in [
            Style { exposure: f32::NAN, ..valid },
            Style { render_scale: 0.1, ..valid },
            Style { render_scale: f32::INFINITY, ..valid },
            Style { texture_limit: 333, ..valid },
            Style { texture_anisotropy: 3, ..valid },
            Style { studio_rotation: 181., ..valid },
            Style { studio_intensity: -1., ..valid },
        ] {
            assert!(editor.apply(crate::Command::SetStyle { style: invalid }).is_err());
            assert_eq!(editor.doc().style, valid);
            assert_eq!(editor.undo_depth(), depth);
        }
    }
}
