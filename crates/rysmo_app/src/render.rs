//! The style's light and render settings: the background and ground, the sun and its
//! shadows, the sky's ambient light, ambient occlusion, tone mapping and anti-aliasing,
//! all with Bevy's own features.
//!
//! The sky is a gradient cubemap made from the sky colour, or Bevy's physically based
//! atmosphere when the style asks for it on the desktop renderer.
//!
//! In the browser (WebGL2) Bevy has no ambient occlusion, no temporal anti-aliasing and
//! no atmosphere: those settings fall back to none, to FXAA and to the gradient sky.
//! Shadows and fog work there.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight, Skybox};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::pbr::{AtmosphereSettings, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use rysmo_model::{AntiAliasing, Lighting, Style, ToneMapping};

use crate::state::{App3d, to_bevy};
use crate::viewport::MainCamera;

#[derive(Component)]
struct SunLight;

#[derive(Component)]
struct Ground;

/// The gradient sky cubemap, repainted when the sky colour changes.
#[derive(Resource)]
struct SkyCube(Handle<Image>);

const SKY_SIZE: u32 = 64;

/// Exposure for sunlit scenes: an overcast day, two stops under the sun's own EV 15.
const SUN_EV100: f32 = 12.0;

/// Whether this build can do ambient occlusion and temporal anti-aliasing.
pub const ADVANCED: bool = !cfg!(target_arch = "wasm32");

/// Whether the 3D camera renders in HDR: the atmosphere, or bloom around glowing materials.
pub fn needs_hdr(app: &App3d) -> bool {
    let style = &app.editor.doc().style;
    let sun = style.lighting == Lighting::Sun;
    let atmosphere = style.atmosphere && sun && ADVANCED && !app.features.photo.active;
    let bloom = style.lighting != Lighting::Flat
        && cfg!(not(target_arch = "wasm32"))
        && app.editor.doc().materials.values().any(|m| m.glow > 0.0);
    atmosphere || bloom
}

/// Whether the 3D view renders offscreen and egui composites it under the interface. An HDR
/// camera cannot share the window with the egui camera: Bevy would give the overlay its own
/// never-cleared intermediate texture, freezing the 3D view and leaving old tooltips behind.
pub fn offscreen(app: &App3d) -> bool {
    (app.editor.doc().style.render_scale - 1.).abs() > 0.001 || app.features.photo.active || needs_hdr(app)
}

/// Under the sun, the luminance in cd/m² of a panorama value of one as light. A normalised sky
/// then lights a level surface with roughly a fifth of the sun's 10 000 lx, so shadows stay
/// legible whatever the exposure.
const SKY_FILL_NITS: f32 = 1200.;

/// Luminance, in cd/m², of a studio environment map value of one.
const STUDIO_NITS: f32 = 2000.;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, (setup, crate::studio::setup))
        .add_systems(PostUpdate, (apply_style.in_set(ApplyStyle), follow_camera).before(TransformSystems::Propagate));
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ApplyStyle;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    let mut cube = Image::new_fill(
        Extent3d { width: SKY_SIZE, height: SKY_SIZE, depth_or_array_layers: 6 },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    cube.texture_view_descriptor =
        Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    commands.insert_resource(SkyCube(images.add(cube)));
    if ADVANCED {
        // Only cameras with AtmosphereSettings render it.
        commands.spawn(bevy::light::Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256))));
    }
    commands.spawn((
        DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() },
        Transform::default(),
        SunLight,
    ));
    // A ground plane just below Z = 0, so it never fights with faces on the ground. It
    // reaches far enough to meet the horizon, where the atmosphere's haze hides its edge.
    let plane = meshes.add(Plane3d::default().mesh().size(400_000.0, 400_000.0));
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 1.0, ..default() });
    commands.spawn((
        Mesh3d(plane),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, -0.003, 0.0),
        Visibility::Hidden,
        Ground,
    ));
}

fn srgb(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

/// The sky colour as a pale horizon and a deeper zenith, in sRGB.
fn sky_gradient(sky: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let horizon = sky.map(|c| c + (1. - c) * 0.45);
    let zenith = [sky[0] * 0.52, sky[1] * 0.66, sky[2] * 0.92];
    (horizon, zenith)
}

/// The sky colour seen in a direction with this elevation (sine of the angle above the horizon).
fn sky_at(horizon: [f32; 3], zenith: [f32; 3], elevation: f32) -> [f32; 3] {
    let t = elevation.max(0.).sqrt();
    let haze = 1. - (-elevation * 4.).clamp(0., 1.) * 0.12;
    std::array::from_fn(|i| (horizon[i] + (zenith[i] - horizon[i]) * t) * haze)
}

/// Paints the cubemap faces (+X, -X, +Y, -Y, +Z, -Z, Bevy's Y up) with the gradient.
fn paint_sky(image: &mut Image, sky: [f32; 3]) {
    let (horizon, zenith) = sky_gradient(sky);
    let n = SKY_SIZE as usize;
    let mut data = Vec::with_capacity(n * n * 6 * 4);
    for face in 0..6 {
        for j in 0..n {
            for i in 0..n {
                let u = 2. * (i as f32 + 0.5) / n as f32 - 1.;
                let v = 2. * (j as f32 + 0.5) / n as f32 - 1.;
                let dir = match face {
                    0 => Vec3::new(1., -v, -u),
                    1 => Vec3::new(-1., -v, u),
                    2 => Vec3::new(u, 1., v),
                    3 => Vec3::new(u, -1., -v),
                    4 => Vec3::new(u, -v, 1.),
                    _ => Vec3::new(-u, -v, -1.),
                };
                let c = sky_at(horizon, zenith, dir.normalize().y);
                data.extend(c.map(|c| (c.clamp(0., 1.) * 255.).round() as u8));
                data.push(255);
            }
        }
    }
    image.data = Some(data);
}

#[allow(clippy::too_many_arguments)]
fn apply_style(
    mut commands: Commands,
    app: Res<App3d>,
    mut last: Local<Option<(Style, bool, Option<usize>)>>,
    mut panoramas: Local<crate::environment::Cache>,
    studio: Option<Res<crate::studio::StudioMaps>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    cameras: Query<Entity, With<MainCamera>>,
    mut suns: Query<&mut DirectionalLight, With<SunLight>>,
    mut grounds: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), With<Ground>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sky_cube: Option<Res<SkyCube>>,
    mut images: Option<ResMut<Assets<Image>>>,
) {
    let mut style = app.editor.doc().style;
    if app.features.photo.active {
        style.sky = false;
        style.ground = false;
        style.atmosphere = false;
        style.fog = false;
    }
    let glowing = app.editor.doc().materials.values().any(|m| m.glow > 0.0);
    let sun_lit = style.lighting == Lighting::Sun;
    let studio_lit = style.lighting == Lighting::Studio;
    let panorama = style
        .environment_image
        .filter(|_| studio_lit || sun_lit)
        .and_then(|id| app.editor.doc().images.get(&id))
        .map(|image| image.bytes.clone());
    let panorama_key = panorama.as_ref().map(|bytes| std::sync::Arc::as_ptr(bytes) as usize);
    if last.as_ref() == Some(&(style, glowing, panorama_key)) {
        return;
    }
    let repaint = last.is_none_or(|(l, _, _)| l.sky_color != style.sky_color);
    *last = Some((style, glowing, panorama_key));
    let panorama = match (&panorama, images.as_mut()) {
        (Some(bytes), Some(images)) => panoramas.get(bytes, images),
        _ => None,
    };
    let studio_rotation = Quat::from_rotation_y(style.studio_rotation.to_radians());
    // A panorama replaces the painted or computed sky.
    let atmosphere = style.atmosphere && sun_lit && ADVANCED && panorama.is_none();
    let (horizon, _) = sky_gradient(style.sky_color);
    // On the desktop renderer the sky itself lights a sunlit model: soft light in the
    // shade and reflections in glass and water. Elsewhere a flat ambient light stands in.
    let sky_light = sun_lit && (panorama.is_some() || ADVANCED && (atmosphere || style.sky));
    let backdrop = panorama.filter(|_| style.environment_background).map(|maps| maps.background.clone());
    clear.0 = if atmosphere || backdrop.is_some() {
        Color::BLACK
    } else if style.sky {
        srgb(horizon)
    } else {
        srgb(style.background)
    };
    if let Ok(mut sun) = suns.single_mut() {
        sun.illuminance = if sun_lit { 10_000.0 * style.sun_intensity } else { 0.0 };
        sun.shadow_maps_enabled = sun_lit && style.shadows;
    }
    // The sky's light fills the shade so shadows are not black.
    ambient.color = srgb(style.sky_color);
    ambient.brightness = (if studio_lit {
        0.
    } else if sky_light {
        300.0
    } else if sun_lit {
        1500.0
    } else {
        80.0
    }) * style.ambient_intensity;
    if shadow_map.size != style.shadow_quality.map_size() {
        shadow_map.size = style.shadow_quality.map_size();
    }
    if let Ok((mut vis, mat)) = grounds.single_mut() {
        *vis = if style.ground { Visibility::Visible } else { Visibility::Hidden };
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = srgb(style.ground_color);
            // Without the sun the ground is flat colour like everything else.
            m.unlit = style.lighting == Lighting::Flat;
        }
    }
    let Ok(cam) = cameras.single() else { return };
    let mut e = commands.entity(cam);
    // Bevy's default exposure suits indoor light; 10 000 lx of sun plus the sky would
    // wash materials out to white, so sunlit scenes start from an overcast-day exposure.
    let base = if sun_lit { SUN_EV100 } else { bevy::camera::Exposure::default().ev100 };
    let ev100 = base - if style.lighting == Lighting::Flat { 0. } else { style.exposure };
    e.insert(bevy::camera::Exposure { ev100 });
    // Panorama luminance per unit of its normalised radiance. In the studio it is a light
    // level. Under the sun it stands in for the painted sky: like that sky it shows at its own
    // brightness whatever the exposure; the shade it fills is set against the sun.
    let panorama_nits = if sun_lit { 3. * 2f32.powf(ev100) } else { STUDIO_NITS } * style.studio_intensity;
    let panorama_light =
        if sun_lit { SKY_FILL_NITS * style.ambient_intensity } else { STUDIO_NITS } * style.studio_intensity;
    match &sky_cube {
        _ if backdrop.is_some() => {
            // The panorama in the same units as the light it gives.
            e.insert(Skybox { image: backdrop.clone(), brightness: panorama_nits, rotation: studio_rotation });
        }
        Some(cube) if style.sky && !atmosphere => {
            if repaint && let Some(mut image) = images.as_mut().and_then(|i| i.get_mut(&cube.0)) {
                paint_sky(&mut image, style.sky_color);
            }
            // Undo the exposure, so the sky shows its own colours.
            let brightness = 1.2 * 2f32.powf(ev100);
            e.insert(Skybox { image: Some(cube.0.clone()), brightness, ..default() });
        }
        _ => {
            e.remove::<Skybox>();
        }
    }
    if atmosphere {
        e.insert(AtmosphereSettings::default());
    } else {
        e.remove::<(AtmosphereSettings, bevy::camera::Hdr)>();
    }
    // Glowing materials spill a soft glare around them. Not in WebGL2: with bloom the
    // browser view stops presenting new frames (verified in Chromium and Firefox on macOS).
    if glowing && style.lighting != Lighting::Flat && cfg!(not(target_arch = "wasm32")) {
        e.insert(bevy::post_process::bloom::Bloom::NATURAL);
    } else {
        e.remove::<bevy::post_process::bloom::Bloom>();
    }
    if style.fog {
        e.insert(DistanceFog {
            color: srgb(if style.sky { horizon } else { style.background }),
            falloff: FogFalloff::from_visibility_squared(style.fog_distance),
            ..default()
        });
    } else {
        e.remove::<DistanceFog>();
    }
    match &sky_cube {
        _ if atmosphere => {
            e.remove::<GeneratedEnvironmentMapLight>();
            e.insert(AtmosphereEnvironmentMapLight { intensity: style.ambient_intensity, ..default() });
        }
        _ if panorama.is_some() => {
            e.remove::<(AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight)>();
        }
        Some(cube) if sky_light => {
            e.remove::<AtmosphereEnvironmentMapLight>();
            e.insert(GeneratedEnvironmentMapLight {
                environment_map: cube.0.clone(),
                intensity: 0.6 * 2f32.powf(ev100) * style.ambient_intensity,
                ..default()
            });
        }
        _ => {
            e.remove::<(AtmosphereEnvironmentMapLight, GeneratedEnvironmentMapLight)>();
        }
    }
    if let Some(maps) = panorama {
        e.insert(bevy::light::EnvironmentMapLight {
            diffuse_map: maps.diffuse.clone(),
            specular_map: maps.specular.clone(),
            intensity: panorama_light,
            rotation: studio_rotation,
            ..default()
        });
    } else if studio_lit && let Some(studio) = studio {
        let maps = if style.studio_environment == rysmo_model::StudioEnvironment::Warm {
            studio.warm.clone()
        } else {
            studio.neutral.clone()
        };
        let [diffuse_map, specular_map] = maps;
        e.insert(bevy::light::EnvironmentMapLight {
            diffuse_map,
            specular_map,
            intensity: STUDIO_NITS * style.studio_intensity,
            rotation: studio_rotation,
            ..default()
        });
    } else {
        e.remove::<bevy::light::EnvironmentMapLight>();
    }
    e.insert(match style.tone_mapping {
        ToneMapping::None => Tonemapping::None,
        ToneMapping::AgX => Tonemapping::AgX,
        ToneMapping::TonyMcMapface => Tonemapping::TonyMcMapface,
        ToneMapping::AcesFitted => Tonemapping::AcesFitted,
    });
    // Ambient occlusion and TAA want no multisampling; FXAA smooths after.
    let ao = style.ambient_occlusion && (sun_lit || studio_lit) && ADVANCED;
    let aa = match style.anti_aliasing {
        AntiAliasing::Taa if !ADVANCED => AntiAliasing::Fxaa,
        AntiAliasing::Msaa4 if ao => AntiAliasing::Fxaa,
        other => other,
    };
    e.insert(if aa == AntiAliasing::Msaa4 { Msaa::Sample4 } else { Msaa::Off });
    if aa == AntiAliasing::Fxaa {
        e.insert(Fxaa::default());
    } else {
        e.remove::<Fxaa>();
    }
    if aa == AntiAliasing::Taa {
        e.insert(TemporalAntiAliasing::default());
    } else {
        e.remove::<TemporalAntiAliasing>();
    }
    if ao {
        e.insert(ScreenSpaceAmbientOcclusion::default());
    } else {
        e.remove::<ScreenSpaceAmbientOcclusion>();
    }
}

/// Points the sun and sizes its shadow cascades to the view.
fn follow_camera(
    app: Res<App3d>,
    mut suns: Query<(&mut Transform, &mut bevy::light::CascadeShadowConfig), With<SunLight>>,
) {
    let Ok((mut t, mut cascades)) = suns.single_mut() else { return };
    let dir = to_bevy(app.editor.doc().style.sun.direction());
    *t = Transform::from_translation(Vec3::ZERO).looking_to(-dir, Vec3::Y);
    let reach = (app.camera.distance as f32 * 4.0).max(10.0);
    *cascades = CascadeShadowConfigBuilder {
        num_cascades: app.editor.doc().style.shadow_quality.cascades(),
        minimum_distance: 0.1,
        first_cascade_far_bound: reach * 0.25,
        maximum_distance: reach,
        ..default()
    }
    .build();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_settings_update_lights_quality_and_camera_effects_without_gpu() {
        let mut app = App::new();
        app.insert_resource(App3d::new(rysmo_model::Editor::new()))
            .init_resource::<ClearColor>()
            .init_resource::<GlobalAmbientLight>()
            .init_resource::<DirectionalLightShadowMap>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, apply_style);
        let camera = app.world_mut().spawn(MainCamera).id();
        let sun = app.world_mut().spawn((SunLight, DirectionalLight::default())).id();
        let mut style = Style {
            lighting: Lighting::Sun,
            shadows: true,
            sun_intensity: 0.5,
            ambient_intensity: 0.,
            shadow_quality: rysmo_model::ShadowQuality::Low,
            anti_aliasing: AntiAliasing::Fxaa,
            ..Default::default()
        };
        app.world_mut().resource_mut::<App3d>().editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        app.update();
        assert_eq!(app.world().get::<DirectionalLight>(sun).unwrap().illuminance, 5000.);
        assert!(app.world().get::<DirectionalLight>(sun).unwrap().shadow_maps_enabled);
        assert_eq!(app.world().resource::<GlobalAmbientLight>().brightness, 0.);
        assert_eq!(app.world().resource::<DirectionalLightShadowMap>().size, 512);
        assert!(app.world().get::<Fxaa>(camera).is_some());
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        style.lighting = Lighting::Flat;
        style.anti_aliasing = AntiAliasing::Off;
        app.world_mut().resource_mut::<App3d>().editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        app.update();
        assert!(!app.world().get::<DirectionalLight>(sun).unwrap().shadow_maps_enabled);
        assert_eq!(app.world().get::<DirectionalLight>(sun).unwrap().illuminance, 0.);
        assert!(app.world().get::<Fxaa>(camera).is_none());
        assert!(app.world().get::<ScreenSpaceAmbientOcclusion>(camera).is_none());
    }
}

#[cfg(test)]
mod studio_tests {
    use super::*;
    #[test]
    fn studio_and_exposure_are_removed_when_returning_to_flat() {
        let mut app = App::new();
        app.insert_resource(App3d::new(rysmo_model::Editor::new()))
            .init_resource::<ClearColor>()
            .init_resource::<GlobalAmbientLight>()
            .init_resource::<DirectionalLightShadowMap>()
            .init_resource::<Assets<StandardMaterial>>()
            .insert_resource(crate::studio::StudioMaps { neutral: Default::default(), warm: Default::default() })
            .add_systems(Update, apply_style);
        let camera = app.world_mut().spawn(MainCamera).id();
        let style = Style {
            lighting: Lighting::Studio,
            studio_intensity: 2.,
            studio_rotation: 90.,
            exposure: 1.,
            ..Default::default()
        };
        app.world_mut().resource_mut::<App3d>().editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        app.update();
        assert_eq!(app.world().get::<bevy::light::EnvironmentMapLight>(camera).unwrap().intensity, 4000.);
        assert_eq!(
            app.world().get::<bevy::camera::Exposure>(camera).unwrap().ev100,
            bevy::camera::Exposure::default().ev100 - 1.
        );
        let style = Style { lighting: Lighting::Flat, ..style };
        app.world_mut().resource_mut::<App3d>().editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        app.update();
        assert!(app.world().get::<bevy::light::EnvironmentMapLight>(camera).is_none());
        assert_eq!(
            app.world().get::<bevy::camera::Exposure>(camera).unwrap().ev100,
            bevy::camera::Exposure::default().ev100
        );
    }
}
