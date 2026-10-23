//! Texture pin interaction in the model viewport, with a guarded edit transaction.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, DVec3, Plane};
use rysmo_model::{Command, Element, FaceId, InstanceId, TexturePlacement, pin_mapping, texture_axes, uv_at};
use rysmo_tools::Pointer;

#[derive(Clone)]
pub struct Session {
    pub face: FaceId,
    context: Vec<InstanceId>,
    revision: u64,
    depth: usize,
    was_dirty: bool,
    original: Option<TexturePlacement>,
    placement: TexturePlacement,
    size: [f64; 2],
    plane: Plane,
    pub fixed: bool,
    handles: [[f64; 2]; 4],
    relocating: Option<usize>,
    dragging: Option<Drag>,
}

#[derive(Clone, Copy)]
struct Drag {
    index: usize,
    start: DVec2,
    pixel: DVec2,
    handles: [[f64; 2]; 4],
    pins: [[f64; 2]; 4],
    moved: bool,
}

fn base_uv(point: DVec3, session: &Session) -> DVec2 {
    DVec2::from_array(uv_at(
        point,
        session.plane.normal,
        session.size,
        &TexturePlacement { pins: None, ..session.placement },
    ))
}

/// Intersect a texture coordinate's projection line with the target face plane.
fn point_at(uv: [f64; 2], session: &Session) -> Option<DVec3> {
    let p = session.placement;
    let (u, v) = texture_axes(p.projected_normal.map(DVec3::from_array).unwrap_or(session.plane.normal));
    let q = DVec2::new(uv[0] * session.size[0] * p.scale[0], -uv[1] * session.size[1] * p.scale[1]);
    let (s, c) = p.rotation.to_radians().sin_cos();
    let q = DVec2::new(q.x * c - q.y * s, q.x * s + q.y * c) + DVec2::from_array(p.offset);
    let origin = u * q.x + v * q.y;
    let direction = u.cross(v);
    let denominator = session.plane.normal.dot(direction);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let point = origin + direction * ((session.plane.d - session.plane.normal.dot(origin)) / denominator);
    point.is_finite().then_some(point)
}

pub fn begin(app: &mut App3d) {
    finish(app, false);
    app.set_tool(rysmo_tools::ToolKind::Select);
    let selected = app.editor.selected();
    let Some(face) = selected.iter().find_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }) else {
        app.message = Some("Select a textured face first.".into());
        return;
    };
    let mesh = app.editor.mesh();
    let entity = mesh.face(face).unwrap();
    let texture = [entity.material, entity.back_material]
        .into_iter()
        .flatten()
        .filter_map(|id| app.editor.doc().materials.get(&id).and_then(|m| m.texture))
        .next();
    let Some(texture) = texture else {
        app.message = Some("The selected face needs a textured material.".into());
        return;
    };
    let Some(plane) = mesh.face_plane(face) else { return };
    let original = entity.uv;
    let mut session = Session {
        face,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
        depth: app.editor.undo_depth(),
        was_dirty: app.editor.is_dirty(),
        original,
        placement: original.unwrap_or_default(),
        size: texture.size,
        plane,
        fixed: true,
        handles: crate::texture_pins::TILE,
        relocating: None,
        dragging: None,
    };
    let centre = mesh.loop_points(&entity.outer).iter().copied().sum::<DVec3>() / entity.outer.len() as f64;
    let uv = uv_at(centre, plane.normal, texture.size, &session.placement);
    let origin = DVec2::from_array(uv).floor();
    let forward = session.placement.pins.and_then(pin_mapping).map(|m| m.inverse());
    let pins = crate::texture_pins::TILE.map(|corner| {
        let image = DVec2::from_array(corner) + origin;
        if let Some(m) = forward {
            let q = m * image.extend(1.);
            [q.x / q.z, q.y / q.z]
        } else {
            image.to_array()
        }
    });
    if pin_mapping(pins).is_none() {
        app.message = Some("This texture tile crosses the projection horizon. Reset its position first.".into());
        return;
    }
    session.placement.pins = Some(pins);
    session.handles = pins;
    if point_at(pins[0], &session).is_none() {
        app.message = Some("Texture projection is parallel to this face.".into());
        return;
    }
    app.texture_session = Some(session);
    app.message = Some("Texture Position: drag a pin or the face. Enter finishes; Escape cancels.".into());
}

fn valid(app: &App3d, session: &Session) -> bool {
    app.editor.revision() == session.revision
        && app.editor.context() == session.context
        && app.editor.mesh().face(session.face).is_some()
        && (app.editor.undo_depth() == session.depth || app.editor.undo_depth() == session.depth + 1)
}

pub fn finish(app: &mut App3d, cancel: bool) {
    let Some(session) = app.texture_session.take() else { return };
    if cancel && valid(app, &session) && app.editor.undo_depth() == session.depth + 1 {
        if let Err(error) = app
            .editor
            .amend_last(Command::SetTexturePlacement { faces: vec![session.face], placement: session.original })
        {
            app.message = Some(error.to_string());
            return;
        }
        if !session.was_dirty && app.editor.undo_depth() == session.depth {
            app.editor.mark_saved();
        }
    }
    app.message = None;
}

fn update(app: &mut App3d, session: &mut Session, pins: [[f64; 2]; 4]) -> bool {
    let placement = TexturePlacement { pins: Some(pins), ..session.placement };
    let command = Command::SetTexturePlacement { faces: vec![session.face], placement: Some(placement) };
    let result = if app.editor.undo_depth() == session.depth + 1 {
        app.editor.amend_last(command)
    } else {
        app.editor.apply(command)
    };
    match result {
        Ok(_) => {
            session.placement = placement;
            session.revision = app.editor.revision();
            true
        }
        Err(error) => {
            app.message = Some(error.to_string());
            false
        }
    }
}

fn drag_in_plane(
    session: &Session,
    pins: [[f64; 2]; 4],
    index: usize,
    target: DVec2,
    fixed: bool,
) -> Option<[[f64; 2]; 4]> {
    let size = DVec2::new(session.size[0] * session.placement.scale[0], session.size[1] * session.placement.scale[1]);
    let physical = pins.map(|p| (DVec2::from_array(p) * size).to_array());
    crate::texture_pins::drag(physical, index, target * size, fixed)
        .map(|pins| pins.map(|p| (DVec2::from_array(p) / size).to_array()))
}

pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    let Some(mut session) = app.texture_session.take() else { return };
    if !valid(app, &session) {
        app.message = Some("Texture editing ended because the model changed.".into());
        return;
    }
    let view = app.local_view();
    let ray = view.ray(at);
    let point = session.plane.intersect_ray(&ray).map(|d| ray.at(d));
    let pins = session.placement.pins.unwrap();
    match event {
        Pointer::Down { .. } => {
            if session.relocating.take().is_some() {
                app.texture_session = Some(session);
                return;
            }
            let closest = session
                .handles
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    point_at(*p, &session).and_then(|p| view.project(p)).map(|pixel| (i, pixel.distance(at)))
                })
                .filter(|(_, distance)| *distance <= 12. * app.pick_scale())
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some(point) = point {
                let index = if let Some((index, _)) = closest {
                    index
                } else if app.editor.mesh().face_contains(session.face, point) {
                    4
                } else {
                    app.message = None;
                    return;
                };
                session.dragging = Some(Drag {
                    index,
                    start: base_uv(point, &session),
                    pixel: at,
                    handles: session.handles,
                    pins,
                    moved: false,
                });
            }
        }
        Pointer::Move => {
            if let (Some(index), Some(point)) = (session.relocating, point) {
                let mut next = session.handles;
                next[index] = base_uv(point, &session).to_array();
                if pin_mapping(next).is_some() {
                    session.handles = next;
                }
            } else if let (Some(mut drag), Some(point)) = (session.dragging, point) {
                drag.moved |= at.distance(drag.pixel) >= 3. * app.pick_scale();
                session.dragging = Some(drag);
                if drag.moved {
                    let (index, fixed) = if drag.index == 4 { (0, true) } else { (drag.index, session.fixed) };
                    let mut target = DVec2::from_array(drag.handles[index]) + base_uv(point, &session) - drag.start;
                    if index == 1 && fixed {
                        let size = DVec2::new(
                            session.size[0] * session.placement.scale[0],
                            session.size[1] * session.placement.scale[1],
                        );
                        let anchor = DVec2::from_array(drag.handles[0]);
                        let direction = (target - anchor) * size;
                        let radius = ((DVec2::from_array(drag.handles[1]) - anchor) * size).length();
                        if direction.length_squared() > 1e-12 {
                            let snapped = anchor + direction.normalize() * radius / size;
                            let near_arc = point_at(snapped.to_array(), &session)
                                .and_then(|p| view.project(p))
                                .is_some_and(|p| p.distance(at) < 6. * app.pick_scale());
                            if app.modifiers().shift || near_arc {
                                target = snapped;
                            }
                        }
                    }
                    if let Some(handles) = drag_in_plane(&session, drag.handles, index, target, fixed) {
                        let map = pin_mapping(handles).unwrap().inverse() * pin_mapping(drag.handles).unwrap();
                        let next = drag.pins.map(|p| {
                            let q = map * DVec3::new(p[0], p[1], 1.);
                            [q.x / q.z, q.y / q.z]
                        });
                        if pin_mapping(next).is_some()
                            && next != session.placement.pins.unwrap()
                            && update(app, &mut session, next)
                        {
                            session.handles = handles;
                        }
                    }
                }
            }
        }
        Pointer::Up => {
            if let Some(drag) = session.dragging.take()
                && !drag.moved
                && drag.index < 4
            {
                session.relocating = Some(drag.index);
            }
        }
    }
    app.texture_session = Some(session);
}

/// Switching from a drag to camera navigation keeps the session and its edits.
pub fn pause_drag(app: &mut App3d) {
    if let Some(session) = app.texture_session.as_mut() {
        session.dragging = None;
        session.relocating = None;
    }
}

pub fn reset(app: &mut App3d) {
    let Some(mut session) = app.texture_session.take() else { return };
    if !valid(app, &session) {
        return;
    }
    let command = Command::SetTexturePlacement { faces: vec![session.face], placement: None };
    let result = if app.editor.undo_depth() == session.depth + 1 {
        app.editor.amend_last(command)
    } else {
        app.editor.apply(command)
    };
    match result {
        Ok(_) => {
            session.placement = TexturePlacement::default();
            let mesh = app.editor.mesh();
            let face = mesh.face(session.face).unwrap();
            let centre = mesh.loop_points(&face.outer).iter().copied().sum::<DVec3>() / face.outer.len() as f64;
            let origin = base_uv(centre, &session).floor();
            session.handles = crate::texture_pins::TILE.map(|p| (DVec2::from_array(p) + origin).to_array());
            session.placement.pins = Some(session.handles);
            session.revision = app.editor.revision();
            if !session.was_dirty && app.editor.undo_depth() == session.depth {
                app.editor.mark_saved();
            }
            session.dragging = None;
            session.relocating = None;
        }
        Err(error) => app.message = Some(error.to_string()),
    }
    app.texture_session = Some(session);
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(session) = app.texture_session.as_ref() else { return };
    if !valid(app, session) {
        app.texture_session = None;
        return;
    }
    let view = app.local_view();
    let painter =
        ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("viewport-texture-pins")));
    let origin = app.canvas_origin();
    let size = app.canvas_size();
    let painter = painter.with_clip_rect(egui::Rect::from_min_size(
        egui::pos2(origin.x as f32, origin.y as f32),
        egui::vec2(size.x as f32, size.y as f32),
    ));
    let pins = session.placement.pins.unwrap();
    let forward = pin_mapping(pins).unwrap().inverse();
    let screen =
        |uv: [f64; 2]| point_at(uv, session).and_then(|p| view.project(p)).map(|p| egui::pos2(p.x as f32, p.y as f32));
    for axis in 0..2 {
        for i in 0..=4 {
            let map = |x: f64, y: f64| {
                let q = forward * DVec3::new(x, y, 1.);
                screen([q.x / q.z, q.y / q.z])
            };
            let f = i as f64 / 4.;
            let (a, b) = if axis == 0 { (map(f, 0.), map(f, 1.)) } else { (map(0., f), map(1., f)) };
            if let (Some(a), Some(b)) = (a, b) {
                painter.line_segment([a, b], egui::Stroke::new(1., egui::Color32::from_white_alpha(180)));
            }
        }
    }
    if let Some(drag) = session.dragging
        && drag.index == 1
        && session.fixed
    {
        let size =
            DVec2::new(session.size[0] * session.placement.scale[0], session.size[1] * session.placement.scale[1]);
        let anchor = DVec2::from_array(drag.handles[0]);
        let radius = ((DVec2::from_array(drag.handles[1]) - anchor) * size).length();
        for i in (0..64).step_by(2) {
            let at = |i: usize| {
                let a = i as f64 * std::f64::consts::TAU / 64.;
                screen((anchor + DVec2::new(a.cos(), a.sin()) * radius / size).to_array())
            };
            if let (Some(a), Some(b)) = (at(i), at(i + 1)) {
                painter.line_segment([a, b], egui::Stroke::new(1., egui::Color32::from_rgb(85, 185, 105)));
            }
        }
    }
    let colors = [
        egui::Color32::from_rgb(230, 85, 65),
        egui::Color32::from_rgb(85, 185, 105),
        egui::Color32::from_rgb(240, 195, 65),
        egui::Color32::from_rgb(80, 150, 235),
    ];
    for (i, p) in session.handles.into_iter().enumerate() {
        if let Some(at) = screen(p) {
            painter.circle_filled(at, 8., colors[i]);
            painter.circle_stroke(at, 10., egui::Stroke::new(2., egui::Color32::WHITE));
        }
    }
    let mut fixed = session.fixed;
    let mut finish_edit = false;
    let mut cancel = false;
    let mut reset_texture = false;
    let window = crate::theme::window("Texture Position")
        .default_pos(egui::pos2(origin.x as f32 + 8., origin.y as f32 + 8.))
        .default_width(320.)
        .max_width((ctx.content_rect().width() - 32.).max(160.))
        .constrain_to(ctx.content_rect().shrink(8.))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            crate::widgets::checkbox(ui, &mut fixed, "Fixed pins");
            ui.label(if session.relocating.is_some() { "Click to place the pin." } else { "Drag a pin or the face." });
            crate::widgets::section(ui, "Help", false, |ui| {
                ui.label("Click a pin to relocate its anchor; click again to place it.");
                ui.weak("Green ring snaps to rotation only. Shift also locks the scale.");
                ui.weak("Red: move · Green: rotate/scale · Blue: shear/scale · Yellow: perspective");
            });
            ui.horizontal(|ui| {
                reset_texture = ui.button("Reset").clicked();
                finish_edit = ui.button("Done").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if let Some(window) = window {
        let r = window.response.rect;
        app.ui_rects.push((DVec2::new(r.min.x as f64, r.min.y as f64), DVec2::new(r.max.x as f64, r.max.y as f64)));
    }
    if let Some(session) = app.texture_session.as_mut() {
        session.fixed = fixed;
    }
    finish_edit |= ctx.input(|i| i.key_pressed(egui::Key::Enter));
    cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if reset_texture {
        reset(app);
    }
    if finish_edit || cancel {
        finish(app, cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Editor, Texture};
    fn fixture() -> App3d {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(12., 23., 2.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: None,
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let image = editor
            .apply(Command::AddImage { name: "test.png".into(), mime: "image/png".into(), data: vec![1] })
            .unwrap()
            .image
            .unwrap();
        let material = editor
            .apply(Command::AddMaterial {
                name: "Tile".into(),
                color: [1.; 4],
                texture: Some(Texture { image, size: [1., 1.] }),
            })
            .unwrap()
            .material;
        editor.apply(Command::Paint { faces: vec![face], material, back: false }).unwrap();
        editor.set_selection([Element::Face(face)]);
        let mut app = App3d::new(editor);
        app.camera.frame(DVec3::new(12., 23., 2.), DVec3::new(14., 25., 2.));
        app
    }
    fn move_pin(app: &mut App3d, index: usize, delta: DVec3) {
        let session = app.texture_session.as_ref().unwrap();
        let point = point_at(session.handles[index], session).unwrap();
        let start = app.local_view().project(point).unwrap();
        let end = app.local_view().project(point + delta).unwrap();
        pointer(app, start, Pointer::Down { clicks: 1 });
        pointer(app, end, Pointer::Move);
        pointer(app, end, Pointer::Up);
    }
    #[test]
    fn viewport_pins_near_faraway_face_have_one_undo_and_cancel_removes_the_step() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        assert_eq!(app.editor.doc(), &before);
        let session = app.texture_session.as_ref().unwrap();
        let pin = point_at(session.placement.pins.unwrap()[0], session).unwrap();
        assert!((pin - DVec3::new(13., 24., 2.)).length() < 1e-9);
        move_pin(&mut app, 0, DVec3::new(0.2, 0.1, 0.));
        assert_eq!(app.editor.undo_depth(), depth + 1);
        move_pin(&mut app, 1, DVec3::new(0.1, 0.1, 0.));
        assert_eq!(app.editor.undo_depth(), depth + 1);
        assert_ne!(app.editor.doc(), &before);
        finish(&mut app, true);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
        assert_eq!(app.editor.redo_label(), None);
        begin(&mut app);
        move_pin(&mut app, 0, DVec3::new(0.2, 0.1, 0.));
        finish(&mut app, false);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        app.editor.redo();
        assert_ne!(app.editor.doc(), &before);
    }
    #[test]
    fn cancel_after_unrelated_edit_does_not_undo_either_operation() {
        let mut app = fixture();
        begin(&mut app);
        move_pin(&mut app, 0, DVec3::X * 0.2);
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        finish(&mut app, true);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
    }
    #[test]
    fn pins_and_pointer_work_inside_rotated_scaled_component() {
        let mut app = fixture();
        let elements = app.editor.mesh().faces().map(|(f, _)| Element::Face(f)).collect();
        let group = app.editor.apply(Command::MakeGroup { elements }).unwrap().created[0];
        let Element::Instance(id) = group else { panic!() };
        app.editor
            .apply(Command::Rotate {
                elements: vec![group],
                centre: DVec3::ZERO,
                axis: DVec3::X,
                angle: 35.,
                copies: 0,
            })
            .unwrap();
        app.editor
            .apply(Command::Scale { elements: vec![group], origin: DVec3::ZERO, factors: DVec3::new(2., 0.7, 1.3) })
            .unwrap();
        app.editor.enter(id).unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.set_selection([Element::Face(face)]);
        let (lo, hi) = app.editor.doc().definition_bounds(None).unwrap();
        app.camera.frame(lo, hi);
        begin(&mut app);
        let original = app.texture_session.as_ref().unwrap().placement.pins.unwrap();
        move_pin(&mut app, 0, DVec3::new(0.2, 0.1, 0.));
        let after = app.texture_session.as_ref().unwrap().placement.pins.unwrap();
        assert!((after[0][0] - original[0][0] - 0.2).abs() < 1e-8);
        assert!((after[0][1] - original[0][1] + 0.1).abs() < 1e-8);
    }
    #[test]
    fn free_mode_face_drag_translates_all_pins() {
        let mut app = fixture();
        begin(&mut app);
        app.texture_session.as_mut().unwrap().fixed = false;
        let original = app.texture_session.as_ref().unwrap().placement.pins.unwrap();
        let start = app.local_view().project(DVec3::new(12.3, 23.4, 2.)).unwrap();
        let end = app.local_view().project(DVec3::new(12.5, 23.5, 2.)).unwrap();
        pointer(&mut app, start, Pointer::Down { clicks: 1 });
        pointer(&mut app, end, Pointer::Move);
        pointer(&mut app, end, Pointer::Up);
        let after = app.texture_session.as_ref().unwrap().placement.pins.unwrap();
        for i in 0..4 {
            assert!((after[i][0] - original[i][0] - 0.2).abs() < 1e-8);
            assert!((after[i][1] - original[i][1] + 0.1).abs() < 1e-8);
        }
    }
    #[test]
    fn relocating_an_anchor_changes_no_texture_and_later_rotation_uses_that_anchor() {
        let mut app = fixture();
        begin(&mut app);
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        let session = app.texture_session.as_ref().unwrap();
        let pin = point_at(session.handles[0], session).unwrap();
        let start = app.local_view().project(pin).unwrap();
        let place = app.local_view().project(pin + DVec3::new(0.2, 0.1, 0.)).unwrap();
        pointer(&mut app, start, Pointer::Down { clicks: 1 });
        pointer(&mut app, start, Pointer::Up);
        pointer(&mut app, place, Pointer::Move);
        pointer(&mut app, place, Pointer::Down { clicks: 1 });
        pointer(&mut app, place, Pointer::Up);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
        let anchor = app.texture_session.as_ref().unwrap().handles[0];
        move_pin(&mut app, 1, DVec3::new(0.1, 0.2, 0.));
        assert_eq!(app.texture_session.as_ref().unwrap().handles[0], anchor);
        assert_ne!(app.editor.doc(), &before);
        finish(&mut app, true);
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn rotation_respects_rectangular_images_and_shift_locks_scale() {
        let mut app = fixture();
        let material = app.editor.mesh().faces().next().unwrap().1.material.unwrap();
        let mut texture = app.editor.doc().materials[&material].texture.unwrap();
        texture.size = [2., 1.];
        app.editor.apply(Command::SetMaterialTexture { material, texture: Some(texture) }).unwrap();
        begin(&mut app);
        let session = app.texture_session.as_ref().unwrap();
        let pins = crate::texture_pins::TILE;
        let after = drag_in_plane(session, pins, 1, DVec2::new(0., -2.), true).unwrap();
        assert_eq!(after[0], [0., 0.]);
        assert_eq!(after[3], [0.5, 0.]);
        app.held.shift = true;
        let session = app.texture_session.as_ref().unwrap();
        let before =
            (DVec2::from_array(session.handles[1]) - DVec2::from_array(session.handles[0])) * DVec2::new(2., 1.);
        move_pin(&mut app, 1, DVec3::new(0.3, 0.4, 0.));
        let session = app.texture_session.as_ref().unwrap();
        let after =
            (DVec2::from_array(session.handles[1]) - DVec2::from_array(session.handles[0])) * DVec2::new(2., 1.);
        assert!((before.length() - after.length()).abs() < 1e-8);
    }
    #[test]
    fn reset_and_cancel_preserve_saved_state_and_do_not_leave_empty_history_steps() {
        let mut app = fixture();
        app.editor.mark_saved();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        move_pin(&mut app, 0, DVec3::X * 0.2);
        assert!(app.editor.is_dirty());
        reset(&mut app);
        assert_eq!(app.editor.undo_depth(), depth);
        assert!(!app.editor.is_dirty());
        move_pin(&mut app, 0, DVec3::X * 0.2);
        finish(&mut app, true);
        assert_eq!(app.editor.undo_depth(), depth);
        assert!(!app.editor.is_dirty());
    }
    #[test]
    fn navigation_can_interrupt_a_drag_without_canceling_the_session() {
        let mut app = fixture();
        begin(&mut app);
        let session = app.texture_session.as_ref().unwrap();
        let pin = point_at(session.handles[0], session).unwrap();
        let start = app.local_view().project(pin).unwrap();
        let end = app.local_view().project(pin + DVec3::X * 0.2).unwrap();
        pointer(&mut app, start, Pointer::Down { clicks: 1 });
        pointer(&mut app, end, Pointer::Move);
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        pause_drag(&mut app);
        pointer(&mut app, end + DVec2::new(100., 100.), Pointer::Move);
        assert!(app.texture_session.is_some());
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
        finish(&mut app, true);
    }
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn saving_finishes_the_preview_so_cancel_cannot_disagree_with_the_saved_file() {
        let mut app = fixture();
        app.editor.mark_saved();
        begin(&mut app);
        move_pin(&mut app, 0, DVec3::X * 0.2);
        let path = std::env::temp_dir().join(format!("rysmo-texture-session-save-{}.rysmo", std::process::id()));
        app.path = Some(path.clone());
        crate::files::save(&mut app, false);
        assert!(app.texture_session.is_none());
        assert!(!app.editor.is_dirty());
        let saved = rysmo_io::load(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        assert_eq!(saved.root.mesh.face(face).unwrap().uv, app.editor.mesh().face(face).unwrap().uv);
        std::fs::remove_file(path).unwrap();
    }
}
