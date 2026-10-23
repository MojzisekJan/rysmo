//! The 3D view: the camera, and edges, guides and tool previews drawn as gizmos. Faces
//! are built in `faces.rs`, the style's light and quality settings applied in
//! `render.rs`.

use std::collections::BTreeSet;

use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use rysmo_math::DVec3;
use rysmo_mesh::Element;
use rysmo_model::{EdgeColor, FaceStyle, Guide};
use rysmo_tools::Preview;

use crate::state::{App3d, to_bevy};

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EdgeGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SelectedGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct AxisGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct DottedAxisGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct OverlayGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct DashedOverlayGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FadedGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GuideGizmos;

/// Profiles: thick outlines where a surface turns away from the view.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct ProfileGizmos;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct SectionGizmos;

#[derive(Component)]
pub struct MainCamera;

pub const SELECTED: [f32; 3] = [0.35, 0.55, 1.0];
pub const SKY: Color = Color::srgb(0.84, 0.89, 0.94);

pub fn plugin(app: &mut App) {
    app.init_gizmo_group::<EdgeGizmos>()
        .init_gizmo_group::<SelectedGizmos>()
        .init_gizmo_group::<AxisGizmos>()
        .init_gizmo_group::<DottedAxisGizmos>()
        .init_gizmo_group::<OverlayGizmos>()
        .init_gizmo_group::<DashedOverlayGizmos>()
        .init_gizmo_group::<FadedGizmos>()
        .init_gizmo_group::<GuideGizmos>()
        .insert_resource(ClearColor(SKY))
        .add_systems(Startup, (setup, crate::render_view::setup))
        .init_gizmo_group::<ProfileGizmos>()
        .init_gizmo_group::<SectionGizmos>()
        .add_systems(PostUpdate, sync_section_settings.after(sync_camera))
        .add_systems(
            PostUpdate,
            (
                // Resize image assets and set the target before Bevy caches target
                // metadata and emits asset events for the render-world extraction.
                sync_camera.after(crate::render::ApplyStyle).before(bevy::camera::CameraUpdateSystems),
                draw_gizmos.after(crate::faces::update_edge_cache),
            )
                .before(TransformSystems::Propagate),
        );
}

fn setup(mut commands: Commands, mut gizmo_store: ResMut<GizmoConfigStore>) {
    #[cfg_attr(not(target_arch = "wasm32"), expect(unused_variables, unused_mut))]
    let mut camera = commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Sample4,
        MainCamera,
        Transform::default(),
    ));
    // WebGL2 keeps clustered lights in uniform buffers with room for 16384 light indices.
    // Glow lights reach far, so each can touch every cluster: 64 lights × 192 clusters fits,
    // where the default 4096 clusters overflowed and dropped lights from some clusters.
    #[cfg(target_arch = "wasm32")]
    camera.insert(bevy::light::cluster::ClusterConfig::FixedZ {
        total: 192,
        z_slices: 8,
        z_config: default(),
        dynamic_resizing: true,
    });

    commands.spawn((
        Camera2d,
        // sync_camera matches the window target sample count before extraction.
        Msaa::Off,
        Camera { order: 1, clear_color: bevy::camera::ClearColorConfig::None, ..default() },
        bevy_egui::PrimaryEguiContext,
        bevy::camera::visibility::RenderLayers::layer(31),
    ));
    let (c, _) = gizmo_store.config_mut::<ProfileGizmos>();
    c.line.width = 3.4;
    c.depth_bias = -0.002;
    let (c, _) = gizmo_store.config_mut::<FadedGizmos>();
    c.line.width = 1.2;
    c.depth_bias = -0.002;
    let (c, _) = gizmo_store.config_mut::<GuideGizmos>();
    c.line.width = 1.2;
    c.line.style = GizmoLineStyle::Dashed { gap_scale: 4.0, line_scale: 4.0 };
    c.depth_bias = -0.001;

    let (c, _) = gizmo_store.config_mut::<EdgeGizmos>();
    c.line.width = 1.6;
    c.depth_bias = -0.002;
    let (c, _) = gizmo_store.config_mut::<SelectedGizmos>();
    c.line.width = 3.0;
    c.depth_bias = -0.001;
    let (c, _) = gizmo_store.config_mut::<AxisGizmos>();
    c.line.width = 1.4;
    c.depth_bias = -0.0005;
    let (c, _) = gizmo_store.config_mut::<DottedAxisGizmos>();
    c.line.width = 1.4;
    c.line.style = GizmoLineStyle::Dotted;
    c.depth_bias = -0.0005;
    let (c, _) = gizmo_store.config_mut::<OverlayGizmos>();
    c.line.width = 2.4;
    c.depth_bias = -1.0;
    let (c, _) = gizmo_store.config_mut::<DashedOverlayGizmos>();
    c.line.width = 1.6;
    c.line.style = GizmoLineStyle::Dashed { gap_scale: 3.0, line_scale: 4.0 };
    c.depth_bias = -1.0;
}

fn sync_camera(
    app: Res<App3d>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cams: Query<(&mut Transform, &mut Projection, &mut Camera, &mut bevy::camera::RenderTarget), With<MainCamera>>,
    main_samples: Query<&Msaa, With<MainCamera>>,
    mut overlays: Query<&mut Msaa, (With<bevy_egui::PrimaryEguiContext>, Without<MainCamera>)>,
    target_image: Res<crate::render_view::RenderView>,
    mut images: ResMut<Assets<Image>>,
) {
    // Cameras compositing onto the same window must agree on multisampling.
    // Offscreen 3D targets are resolved before egui draws their image.
    let samples = if (app.editor.doc().style.render_scale - 1.).abs() > 0.001 || app.features.photo.active {
        Msaa::Off
    } else {
        main_samples.single().copied().unwrap_or(Msaa::Off)
    };
    for mut overlay in &mut overlays {
        if *overlay != samples {
            *overlay = samples;
        }
    }
    let Ok((mut t, mut proj, mut camera, mut render_target)) = cams.single_mut() else { return };
    camera.clear_color = if app.features.photo.active {
        bevy::camera::ClearColorConfig::Custom(Color::NONE)
    } else {
        bevy::camera::ClearColorConfig::Default
    };
    if let Ok(window) = windows.single() {
        let scale = window.resolution.scale_factor() as f64;
        let origin = app.canvas_origin() * scale;
        let size = app.canvas_size() * scale;
        let target = UVec2::new(window.resolution.physical_width().max(1), window.resolution.physical_height().max(1));
        let position =
            UVec2::new(origin.x.max(0.).round() as u32, origin.y.max(0.).round() as u32).min(target - UVec2::ONE);
        let size = UVec2::new(size.x.max(1.).round() as u32, size.y.max(1.).round() as u32).min(target - position);
        if (app.editor.doc().style.render_scale - 1.).abs() > 0.001 || app.features.photo.active {
            let rendered = crate::render_view::target_size(size, app.editor.doc().style.render_scale);
            if images.get(&target_image.image).is_some_and(|i| {
                i.texture_descriptor.size.width != rendered.x || i.texture_descriptor.size.height != rendered.y
            }) && let Some(mut image) = images.get_mut(&target_image.image)
            {
                image.resize(bevy::render::render_resource::Extent3d {
                    width: rendered.x,
                    height: rendered.y,
                    depth_or_array_layers: 1,
                });
            }
            *render_target = target_image.image.clone().into();
            camera.viewport =
                Some(bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: rendered, ..default() });
        } else {
            if images
                .get(&target_image.image)
                .is_some_and(|i| i.texture_descriptor.size.width != 1 || i.texture_descriptor.size.height != 1)
                && let Some(mut image) = images.get_mut(&target_image.image)
            {
                image.resize(bevy::render::render_resource::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            }
            *render_target = bevy::camera::RenderTarget::default();
            camera.viewport =
                Some(bevy::camera::Viewport { physical_position: position, physical_size: size, ..default() });
        }
    }
    let eye = to_bevy(app.camera.eye());
    let target = to_bevy(app.camera.target);
    *t = Transform::from_translation(eye).looking_at(target, to_bevy(app.camera.up()));
    let view = app.view();
    // Same near and far as the core's projection, so picking matches the picture.
    let near = (app.camera.distance * 0.001).max(0.001) as f32;
    let far = (app.camera.distance * 1000.0 + 1000.0) as f32;
    *proj = if app.camera.parallel {
        let height = (2.0 * app.camera.distance * (app.camera.fov_y * 0.5).tan()) as f32;
        Projection::Orthographic(OrthographicProjection {
            near,
            far,
            scaling_mode: bevy::camera::ScalingMode::FixedVertical { viewport_height: height },
            ..OrthographicProjection::default_3d()
        })
    } else {
        Projection::Perspective(PerspectiveProjection {
            fov: app.camera.fov_y as f32,
            aspect_ratio: (view.size.x / view.size.y.max(1.0)) as f32,
            near,
            far,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -near),
        })
    };
}

fn color3(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

#[allow(clippy::too_many_arguments)]
fn draw_gizmos(
    app: Res<App3d>,
    mut edges: Gizmos<EdgeGizmos>,
    mut faded: Gizmos<FadedGizmos>,
    mut selected: Gizmos<SelectedGizmos>,
    mut axes: Gizmos<AxisGizmos>,
    mut dotted: Gizmos<DottedAxisGizmos>,
    mut overlay: Gizmos<OverlayGizmos>,
    mut dashed: Gizmos<DashedOverlayGizmos>,
    mut guides: Gizmos<GuideGizmos>,
    mut profiles: Gizmos<ProfileGizmos>,
    mut section_lines: Gizmos<SectionGizmos>,
    cache: Res<crate::faces::EdgeCache>,
) {
    let editor = &app.editor;
    let doc = editor.doc();
    let preview: Preview = app.tools.preview(editor);
    let marked: BTreeSet<_> = preview.edges.iter().copied().collect();
    let ctx_t = editor.context_transform();
    let to_world = |p: DVec3| to_bevy(ctx_t.transform_point3(p));
    let eye = app.camera.eye();
    let forward = (app.camera.target - eye).normalize();
    let mut active_seen = false;
    for occ in doc.occurrences(editor.context()) {
        let Some(def) = doc.def(occ.definition) else { continue };
        let is_active = occ.in_context && !active_seen;
        if occ.in_context {
            active_seen = true;
        }
        let t = occ.transform;
        let style = &doc.style;
        let cuts = doc.section_planes(&occ.path);
        let eye_local = t.inverse().transform_point3(eye);
        if let Some(list) = cache.defs.get(&occ.definition) {
            for info in list {
                let lit = is_active && (editor.is_selected(Element::Edge(info.id)) || marked.contains(&info.id));
                // Profiles: an edge with one face, or between a face turned to the eye and
                // one turned away.
                let profile = !info.ghost
                    && style.profiles
                    && style.faces != FaceStyle::Wireframe
                    && match info.normals.as_slice() {
                        [_] => true,
                        [n1, n2] => n1.dot(eye_local - info.a).signum() != n2.dot(eye_local - info.a).signum(),
                        _ => false,
                    };
                if !lit && !info.ghost && !profile && !style.edges && style.faces != FaceStyle::Wireframe {
                    continue;
                }
                let Some([a, b]) = rysmo_model::sections::clip_segment(
                    [t.transform_point3(info.a), t.transform_point3(info.b)],
                    &cuts,
                ) else {
                    continue;
                };
                let (a, b) = (to_bevy(a), to_bevy(b));
                let color = match style.edge_color {
                    EdgeColor::Black => Color::BLACK,
                    EdgeColor::ByMaterial => color3(info.material),
                    EdgeColor::ByAxis => {
                        let d = (info.b - info.a).normalize_or_zero();
                        if d.x.abs() > 0.9999 {
                            Color::srgb(0.85, 0.1, 0.1)
                        } else if d.y.abs() > 0.9999 {
                            Color::srgb(0.1, 0.6, 0.1)
                        } else if d.z.abs() > 0.9999 {
                            Color::srgb(0.1, 0.2, 0.9)
                        } else {
                            Color::BLACK
                        }
                    }
                };
                if info.ghost && !lit {
                    dashed.line(a, b, Color::srgb(0.5, 0.55, 0.6));
                } else if lit {
                    selected.line(a, b, color3(SELECTED));
                } else if !occ.in_context {
                    faded.line(a, b, Color::srgb(0.55, 0.58, 0.62));
                } else if profile {
                    profiles.line(a, b, color);
                } else {
                    edges.line(a, b, color);
                }
            }
        }
        // No active cut means there can be no section intersections. Avoid
        // evaluating and triangulating every occurrence on every frame.
        if doc.style.section_lines && !cuts.is_empty() {
            let mesh = doc.effective_mesh(occ.definition);
            for (fid, face) in mesh.faces() {
                if face.hidden && !doc.style.show_hidden_geometry {
                    continue;
                }
                for triangle in rysmo_mesh::triangulate_face(&mesh, fid) {
                    let points = triangle.map(|v| t.transform_point3(mesh.pos(v)));
                    for plane in &cuts {
                        let mut crossings = Vec::new();
                        for i in 0..3 {
                            let (a, b) = (points[i], points[(i + 1) % 3]);
                            let (da, db) = (plane.distance(a), plane.distance(b));
                            if da.abs() <= 1e-9 {
                                crossings.push(a);
                            }
                            if da * db < 0. {
                                crossings.push(a.lerp(b, da / (da - db)));
                            }
                        }
                        crossings.dedup_by(|a, b| a.distance(*b) < 1e-9);
                        if crossings.len() == 2
                            && let Some([a, b]) =
                                rysmo_model::sections::clip_segment([crossings[0], crossings[1]], &cuts)
                        {
                            section_lines.line(to_bevy(a), to_bevy(b), color3(doc.style.section_color));
                        }
                    }
                }
            }
        }
        if doc.style.section_planes && !def.sections.planes.is_empty() {
            let bounds = doc.definition_bounds(occ.definition);
            for (id, plane) in &def.sections.planes {
                let color = if occ.in_context && occ.top.is_none() && editor.is_selected(Element::Section(*id)) {
                    color3(SELECTED)
                } else if def.sections.active == Some(*id) {
                    color3(doc.style.section_active_color)
                } else {
                    color3(doc.style.section_inactive_color)
                };
                for [a, b] in rysmo_model::sections::glyph(plane, bounds) {
                    overlay.line(to_bevy(t.transform_point3(a)), to_bevy(t.transform_point3(b)), color);
                }
            }
        }

        // Guides of every context; those outside the one being edited are faded.
        for (gid, g) in &def.guides {
            if !doc.style.show_guides || (g.hidden() && !doc.style.show_hidden_geometry) {
                continue;
            }
            let lit = is_active && editor.is_selected(Element::Guide(*gid));
            let color = if lit {
                color3(SELECTED)
            } else if occ.in_context {
                Color::srgb(0.15, 0.15, 0.15)
            } else {
                Color::srgb(0.6, 0.6, 0.6)
            };
            match g {
                Guide::Line { origin, dir, .. } => {
                    let o = t.transform_point3(*origin);
                    let d = t.transform_vector3(*dir).normalize_or(DVec3::X);
                    let reach = app.camera.distance * 50.0 + 100.0;
                    if let Some((a, b)) = clip_in_front(o - d * reach, o + d * reach, eye, forward) {
                        guides.line(to_bevy(a), to_bevy(b), color);
                    }
                }
                Guide::Point { p, origin, .. } => {
                    let c = t.transform_point3(*p);
                    if let Some(origin) = origin
                        && let Some((a, b)) = clip_in_front(t.transform_point3(*origin), c, eye, forward)
                    {
                        guides.line(to_bevy(a), to_bevy(b), color);
                    }
                    let s = app.camera.distance * 0.01;
                    for d in [DVec3::X, DVec3::Y, DVec3::Z] {
                        edges.line(to_bevy(c - d * s), to_bevy(c + d * s), color);
                    }
                }
            }
        }
    }
    // Selected and highlighted groups and components get their bounding box in blue;
    // the one being edited gets a dashed box.
    let active = editor.active();
    for (iid, inst) in &active.instances {
        let lit = editor.is_selected(Element::Instance(*iid)) || preview.instances.contains(iid);
        if !lit || inst.hidden || !doc.tag_visible(inst.tag) {
            continue;
        }
        if let Some((lo, hi)) = doc.definition_bounds(Some(inst.definition)) {
            for (a, b) in rysmo_tools::box_lines(lo, hi, &(ctx_t * inst.transform)) {
                selected.line(to_bevy(a), to_bevy(b), color3(SELECTED));
            }
        }
    }
    if !editor.context().is_empty()
        && let Some((lo, hi)) = doc.definition_bounds(editor.active_definition())
    {
        for (a, b) in rysmo_tools::box_lines(lo, hi, &ctx_t) {
            dashed.line(to_bevy(a), to_bevy(b), Color::srgb(0.3, 0.3, 0.3));
        }
    }
    // The axes: solid on the positive side, dotted on the negative. Inside
    // a group they follow the group's axes.
    let len = (app.camera.distance * 200.0).max(1000.0);
    let frame = app.editor.active().axes;
    for (dir, c) in [(frame.x, [0.85, 0.1, 0.1]), (frame.y, [0.1, 0.6, 0.1]), (frame.z(), [0.1, 0.2, 0.9])] {
        let o = ctx_t.transform_point3(frame.origin);
        let d = ctx_t.transform_vector3(dir).normalize_or(dir);
        if let Some((a, b)) = clip_in_front(o, o + d * len, eye, forward) {
            axes.line(to_bevy(a), to_bevy(b), color3(c));
        }
        if let Some((a, b)) = clip_in_front(o, o - d * len, eye, forward) {
            dotted.line(to_bevy(a), to_bevy(b), color3(c));
        }
    }
    for l in &preview.lines {
        let (a, b) = (to_world(l.a), to_world(l.b));
        if l.dashed {
            dashed.line(a, b, color3(l.color));
        } else {
            overlay.line(a, b, color3(l.color));
        }
    }
}

/// The part of a segment in front of the camera. Lines that reach behind the eye are
/// not drawn reliably, so long guide lines are cut at a plane just ahead of it.
fn clip_in_front(a: DVec3, b: DVec3, eye: DVec3, forward: DVec3) -> Option<(DVec3, DVec3)> {
    let near = 0.05;
    let da = (a - eye).dot(forward) - near;
    let db = (b - eye).dot(forward) - near;
    match (da >= 0.0, db >= 0.0) {
        (true, true) => Some((a, b)),
        (false, false) => None,
        (true, false) => Some((a, a + (b - a) * (da / (da - db)))),
        (false, true) => Some((a + (b - a) * (da / (da - db)), b)),
    }
}

fn sync_section_settings(
    app: Res<App3d>,
    mut store: ResMut<GizmoConfigStore>,
    cameras: Query<&Camera, With<MainCamera>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let factor = if (app.editor.doc().style.render_scale - 1.).abs() > 0.001 || app.features.photo.active {
        let native = app.canvas_size().x as f32 * windows.single().map_or(1., |w| w.resolution.scale_factor());
        cameras
            .single()
            .ok()
            .and_then(|c| c.viewport.as_ref())
            .map_or(1., |vp| vp.physical_size.x as f32 / native.max(1.))
    } else {
        1.
    };
    // Keep screen-space line widths stable when the 3D target is resized.
    macro_rules! width {
        ($group:ty,$value:expr) => {
            store.config_mut::<$group>().0.line.width = $value * factor;
        };
    }
    width!(EdgeGizmos, 1.6);
    width!(SelectedGizmos, 3.);
    width!(AxisGizmos, 1.4);
    width!(DottedAxisGizmos, 1.4);
    width!(OverlayGizmos, 2.4);
    width!(DashedOverlayGizmos, 1.6);
    width!(FadedGizmos, 1.2);
    width!(GuideGizmos, 1.2);
    width!(ProfileGizmos, 3.4);
    width!(SectionGizmos, app.editor.doc().style.section_width);
    store.config_mut::<SectionGizmos>().0.depth_bias = -0.002;
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    #[test]
    fn scaled_targets_reach_camera_metadata_and_asset_events_in_the_same_frame() {
        use bevy::asset::{AssetEvent, AssetEventSystems, AssetPlugin};
        use bevy::camera::CameraUpdateSystems;
        use bevy::ecs::message::MessageCursor;
        use bevy::gizmos::GizmoAsset;
        use bevy::render::texture::ManualTextureViews;
        use bevy::window::{WindowCreated, WindowResized, WindowScaleFactorChanged};
        for camera_first in [false, true] {
            let mut app = App::new();
            let mut state = App3d::new(rysmo_model::Editor::new());
            state.canvas = Some((rysmo_math::DVec2::new(80., 60.), rysmo_math::DVec2::new(800., 600.)));
            app.add_plugins((MinimalPlugins, AssetPlugin::default()))
                .init_asset::<Image>()
                .init_resource::<Assets<GizmoAsset>>()
                .init_resource::<ManualTextureViews>()
                .init_resource::<crate::faces::EdgeCache>()
                .insert_resource(state)
                .add_message::<WindowCreated>()
                .add_message::<WindowResized>()
                .add_message::<WindowScaleFactorChanged>();
            if !camera_first {
                app.add_plugins(plugin);
            }
            app.add_systems(
                PostUpdate,
                bevy::render::camera::camera_system.in_set(CameraUpdateSystems).before(AssetEventSystems),
            );
            if camera_first {
                app.add_plugins(plugin);
            }
            app.edit_schedule(PostUpdate, |schedule| {
                schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
            });
            app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow));
            app.update();
            let camera = app.world_mut().query_filtered::<Entity, With<MainCamera>>().single(app.world()).unwrap();
            let overlay = app
                .world_mut()
                .query_filtered::<&Msaa, With<bevy_egui::PrimaryEguiContext>>()
                .single(app.world())
                .unwrap();
            assert_eq!(*overlay, Msaa::Sample4);
            let image = app.world().resource::<crate::render_view::RenderView>().image.clone();
            let mut events = MessageCursor::<AssetEvent<Image>>::default();
            events.read(app.world().resource::<Messages<AssetEvent<Image>>>()).for_each(drop);
            for scale in [0.5, 1.5, 1., 0.75, 1.] {
                let mut state = app.world_mut().resource_mut::<App3d>();
                let mut style = state.editor.doc().style;
                style.render_scale = scale;
                state.editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
                app.update();
                let overlay = app
                    .world_mut()
                    .query_filtered::<&Msaa, With<bevy_egui::PrimaryEguiContext>>()
                    .single(app.world())
                    .unwrap();
                assert_eq!(*overlay, if scale == 1. { Msaa::Sample4 } else { Msaa::Off });
                let camera = app.world().get::<Camera>(camera).unwrap();
                let target = camera.physical_target_size().expect("updated camera target");
                let viewport = camera.viewport.as_ref().unwrap();
                assert!(viewport.physical_position.x + viewport.physical_size.x <= target.x);
                assert!(viewport.physical_position.y + viewport.physical_size.y <= target.y);
                if scale != 1. {
                    let expected = crate::render_view::target_size(UVec2::new(800, 600), scale);
                    assert_eq!(target, expected);
                }
                assert!(
                    events
                        .read(app.world().resource::<Messages<AssetEvent<Image>>>())
                        .any(|event| matches!(event, AssetEvent::Modified { id } if *id == image.id())),
                    "image resize must be extracted in the same frame as the new viewport"
                );
            }
        }
    }

    #[test]
    fn resolution_switches_targets_without_changing_editor_projection() {
        let mut app = App::new();
        let mut state = App3d::new(rysmo_model::Editor::new());
        state.canvas = Some((rysmo_math::DVec2::new(80., 60.), rysmo_math::DVec2::new(800., 600.)));
        let projected = state.view().project(rysmo_math::DVec3::ZERO);
        let style = rysmo_model::Style { render_scale: 0.5, ..Default::default() };
        state.editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        let mut images = Assets::<Image>::default();
        let handle = images.add(Image::default());
        app.insert_resource(state)
            .insert_resource(images)
            .insert_resource(crate::render_view::RenderView { image: handle.clone() })
            .add_systems(Update, sync_camera);
        app.world_mut().spawn((Window::default(), bevy::window::PrimaryWindow));
        let camera = app.world_mut().spawn((MainCamera, Camera3d::default(), Transform::default())).id();
        app.update();
        let world = app.world();
        assert!(matches!(
            world.get::<bevy::camera::RenderTarget>(camera).unwrap(),
            bevy::camera::RenderTarget::Image(_)
        ));
        let size = world.get::<Camera>(camera).unwrap().viewport.as_ref().unwrap().physical_size;
        assert_eq!(size, UVec2::new(400, 300));
        assert_eq!(world.resource::<App3d>().view().project(rysmo_math::DVec3::ZERO), projected);
        let mut state = app.world_mut().resource_mut::<App3d>();
        let mut style = state.editor.doc().style;
        style.render_scale = 1.;
        state.editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        app.update();
        assert!(matches!(
            app.world().get::<bevy::camera::RenderTarget>(camera).unwrap(),
            bevy::camera::RenderTarget::Window(_)
        ));
        let image = app.world().resource::<Assets<Image>>().get(&handle).unwrap();
        assert_eq!(image.texture_descriptor.size.width, 1);
        assert_eq!(image.texture_descriptor.size.height, 1);
        assert_eq!(
            app.world().get::<Camera>(camera).unwrap().viewport.as_ref().unwrap().physical_position,
            UVec2::new(80, 60)
        );
    }
}
