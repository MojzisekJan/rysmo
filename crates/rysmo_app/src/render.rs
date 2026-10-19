//! The style's light and render settings: the background and ground, the sun and its
//! shadows, the sky's ambient light, ambient occlusion, tone mapping and anti-aliasing,
//! all with Bevy's own features.
//!
//! In the browser (WebGL2) Bevy has no ambient occlusion and no temporal anti-aliasing:
//! those settings fall back to none and to FXAA. Shadows work there.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap};
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::prelude::*;
use rysmo_model::{AntiAliasing, Lighting, Style, ToneMapping};

use crate::state::{App3d, to_bevy};
use crate::viewport::MainCamera;

#[derive(Component)]
struct SunLight;

#[derive(Component)]
struct Ground;

/// Exposure for sunlit scenes: an overcast day, two stops under the sun's own EV 15.
const SUN_EV100: f32 = 12.0;

/// Whether this build can do ambient occlusion and temporal anti-aliasing.
pub const ADVANCED: bool = !cfg!(target_arch = "wasm32");

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, (setup, crate::studio::setup))
        .add_systems(PostUpdate, (apply_style.in_set(ApplyStyle), follow_camera).before(TransformSystems::Propagate));
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ApplyStyle;

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((
        DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() },
        Transform::default(),
        SunLight,
    ));
    // A large ground plane just below Z = 0, so it never fights with faces on the ground.
    let plane = meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0));
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

#[allow(clippy::too_many_arguments)]
fn apply_style(
    mut commands: Commands,
    app: Res<App3d>,
    mut last: Local<Option<Style>>,
    studio: Option<Res<crate::studio::StudioMaps>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    cameras: Query<Entity, With<MainCamera>>,
    mut suns: Query<&mut DirectionalLight, With<SunLight>>,
    mut grounds: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), With<Ground>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let style = app.editor.doc().style;
    if last.as_ref() == Some(&style) {
        return;
    }
    *last = Some(style);
    clear.0 = srgb(if style.sky { style.sky_color } else { style.background });
    let sun_lit = style.lighting == Lighting::Sun;
    let studio_lit = style.lighting == Lighting::Studio;
    if let Ok(mut sun) = suns.single_mut() {
        sun.illuminance = if sun_lit { 10_000.0 * style.sun_intensity } else { 0.0 };
        sun.shadow_maps_enabled = sun_lit && style.shadows;
    }
    // The sky's light fills the shade so shadows are not black.
    ambient.color = srgb(style.sky_color);
    ambient.brightness = (if studio_lit {
        0.
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
    e.insert(bevy::camera::Exposure {
        ev100: base - if style.lighting == Lighting::Flat { 0. } else { style.exposure },
    });
    if studio_lit && let Some(studio) = studio {
        let maps = if style.studio_environment == rysmo_model::StudioEnvironment::Warm {
            &studio.warm
        } else {
            &studio.neutral
        };
        e.insert(bevy::light::EnvironmentMapLight {
            diffuse_map: maps[0].clone(),
            specular_map: maps[1].clone(),
            intensity: 2000. * style.studio_intensity,
            rotation: Quat::from_rotation_y(style.studio_rotation.to_radians()),
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
