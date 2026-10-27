//! Mouse, keyboard and touch input for the 3D view.
//!
//! Default mouse controls: middle drag orbits, Shift+middle drag pans, the wheel zooms
//! towards the cursor, the left button drives the tool, the right button opens the
//! context menu. Typing goes to the VCB without clicking it; a letter typed into an
//! empty VCB is a tool shortcut instead.
//!
//! Touch: see `rysmo_tools::gesture`. A one-finger drag orbits with Select and
//! Orbit and drives the tool otherwise, or always while an on-screen modifier is on.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key as LogicalKey, KeyboardInput};
use bevy::input::mouse::MouseScrollUnit;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowEvent};
use bevy_egui::input::EguiWantsInput;
use rysmo_math::DVec2;
use rysmo_model::{Command, Element};
use rysmo_tools::gesture::{Gesture, Phase};
use rysmo_tools::{Key, Pointer, ToolKind};

use crate::files;
use crate::state::{App3d, CameraDrag};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (track_window, keyboard, mouse, touch, walk_tick).chain());
}

fn dv(v: Vec2) -> DVec2 {
    DVec2::new(v.x as f64, v.y as f64)
}

fn track_window(mut app: ResMut<App3d>, windows: Query<&Window, With<PrimaryWindow>>) {
    let Ok(w) = windows.single() else { return };
    app.viewport = DVec2::new(w.width() as f64, w.height() as f64);
    if w.cursor_position().is_none() {
        app.cursor = None;
    }
}

/// Sends a pointer event to the active tool.
pub fn tool_pointer(app: &mut App3d, at: DVec2, ev: Pointer) {
    if matches!(ev, Pointer::Move) {
        app.hover_at = Some(at);
    }
    if app.mirror.is_some() {
        crate::mirror::pointer(app, at, ev);
        return;
    }
    if app.paste_place.is_some() {
        crate::object_clipboard::pointer(app, at, ev);
        return;
    }
    if app.texture_session.is_some() {
        crate::texture_view::pointer(app, at, ev);
        return;
    }
    if matches!(ev, Pointer::Down { .. }) {
        app.terrain_last = None;
    }
    if app.component_place.is_some() {
        crate::component_panel::pointer(app, at, ev);
        return;
    }
    if app.section_placing.is_some() {
        crate::section_panel::pointer(app, at, ev);
        return;
    }
    if app.drape.is_some() {
        crate::drape_tool::pointer(app, at, ev);
        return;
    }
    if app.stamp.is_some() {
        crate::stamp_tool::pointer(app, at, ev);
        return;
    }
    if app.detail.is_some() {
        crate::detail_tool::pointer(app, at, ev);
        return;
    }
    if app.flip.is_some() {
        crate::flip_tool::pointer(app, at, ev);
        return;
    }
    if app.smoove.is_some() {
        crate::smoove_tool::pointer(app, at, ev);
        return;
    }
    if app.terrain.is_some() {
        crate::terrain_tool::pointer(app, at, ev);
        return;
    }
    if app.text3d.is_some() {
        crate::text3d_tool::pointer(app, at, ev);
        return;
    }
    if app.tools.active() == ToolKind::Text {
        crate::text_tool::pointer(app, at, ev);
        return;
    }

    if app.tools.active() == ToolKind::Walk {
        match ev {
            Pointer::Down { .. } => {
                app.remember_view();
                app.camera.parallel = false;
                app.walking = Some((at, at));
            }
            Pointer::Move => {
                if let Some((_, current)) = &mut app.walking {
                    *current = at;
                }
            }
            Pointer::Up => app.walking = None,
        }
        return;
    }
    if app.tools.active() == ToolKind::PositionCamera {
        app.with_ctx(at, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
        let point =
            app.tools.preview(&app.editor).inference.map(|i| app.editor.context_transform().transform_point3(i.point));
        match ev {
            Pointer::Down { .. } => {
                if let Some(point) = point {
                    app.positioning = Some((at, point));
                }
            }
            Pointer::Up => {
                if let Some((start, point)) = app.positioning.take() {
                    let dragged = start.distance(at) > 6.0 * app.pick_scale();
                    let direct = dragged || app.modifiers().shift;
                    let eye = point + rysmo_math::DVec3::Z * if direct { 0.0 } else { app.eye_height };
                    let mut camera = app.camera;
                    // Target is the current inferred point, distinct from the placement anchor.
                    let target = if dragged {
                        app.tools
                            .preview(&app.editor)
                            .inference
                            .map(|i| app.editor.context_transform().transform_point3(i.point))
                    } else {
                        None
                    };
                    if camera.place_eye(eye, target) {
                        app.remember_view();
                        camera.eye_base_z = point.z;
                        app.camera = camera;
                        app.eye_height = eye.z - point.z;
                        app.set_tool(ToolKind::LookAround);
                    }
                }
            }
            _ => {}
        }
        return;
    }
    if app.tools.active() == ToolKind::ZoomWindow {
        match ev {
            Pointer::Down { .. } => {
                app.zoom_window = Some((at, at));
                app.message = None;
            }
            Pointer::Move => {
                if let Some((_, end)) = &mut app.zoom_window {
                    *end = at;
                }
            }
            Pointer::Up => {
                if let Some((start, _)) = app.zoom_window.take() {
                    let mut camera = app.camera;
                    if camera.zoom_window(start - app.canvas_origin(), at - app.canvas_origin(), app.canvas_size()) {
                        app.remember_view();
                        app.camera = camera;
                    }
                }
            }
        }
        return;
    }
    if std::env::var_os("RYSMO_TRACE").is_some() && !matches!(ev, Pointer::Move) {
        eprintln!("pointer {ev:?} at {at:?} tool {:?} busy {}", app.tools.active(), app.tools.busy());
    }
    let annotation =
        if matches!(app.tools.active(), ToolKind::Select | ToolKind::Move | ToolKind::Rotate | ToolKind::Scale)
            && matches!(ev, Pointer::Down { .. })
        {
            app.pick_annotation(at)
        } else {
            None
        };
    let boxed = if app.tools.active() == ToolKind::Select && matches!(ev, Pointer::Up) {
        app.tools
            .preview(&app.editor)
            .select_box
            .filter(|(from, _, _)| from.distance(at) > 6.0 * app.pick_scale())
            .map(|(from, _, _)| app.annotation_box(from, at))
    } else {
        None
    };
    app.with_ctx(at, |tools, editor, ctx| {
        if let Some(pick) = annotation {
            tools.pointer_with_pick(editor, ctx, ev, pick);
        } else if let Some(hits) = boxed {
            tools.pointer_with_box_hits(editor, ctx, ev, hits);
        } else {
            tools.pointer(editor, ctx, ev);
        }
    });
    if matches!(ev, Pointer::Down { .. }) {
        app.message = None;
    }
    if matches!(ev, Pointer::Down { clicks: 2.. })
        && matches!(annotation, Some(rysmo_infer::Pick::Element(Element::Dimension(_) | Element::Text(_))))
    {
        app.show_entity_info = true;
        app.inspector_tab = 1;
    }
}

pub fn tool_key(app: &mut App3d, key: Key) {
    if app.divide.is_some() && key == Key::Escape {
        app.divide = None;
        return;
    }
    if app.fillet.is_some() && key == Key::Escape {
        app.fillet = None;
        return;
    }
    if app.print_check.is_some() && key == Key::Escape {
        app.print_check = None;
        return;
    }
    if app.solid_tools.is_some() && key == Key::Escape {
        app.solid_tools = None;
        return;
    }
    if app.mirror.is_some() {
        crate::mirror::key(app, key);
        return;
    }
    if app.paste_place.is_some() {
        if key == Key::Escape {
            app.paste_place = None;
            app.message = None;
        }
        return;
    }
    if app.texture_session.is_some() {
        if key == Key::Escape {
            crate::texture_view::finish(app, true);
        }
        return;
    }
    if app.component_place.is_some() {
        if key == Key::Escape {
            app.component_place = None;
            app.message = None;
        }
        return;
    }
    if app.section_placing.is_some() {
        if key == Key::Escape {
            app.section_placing = None;
        }
        return;
    }
    if app.drape.is_some() {
        if key == Key::Escape {
            app.drape = None;
        }
        return;
    }
    if app.stamp.is_some() {
        if key == Key::Escape {
            app.stamp = None;
        }
        return;
    }
    if app.detail.is_some() {
        if key == Key::Escape {
            app.detail = None;
        } else if let Some(at) = app.cursor {
            crate::detail_tool::pointer(app, at, Pointer::Move);
        }
        return;
    }
    if app.flip.is_some() {
        if key == Key::Escape {
            app.flip = None;
        }
        return;
    }
    if app.terrain.is_some() {
        crate::terrain_tool::key(app, key);
        return;
    }
    if app.smoove.is_some() {
        if key == Key::Escape {
            app.smoove = None;
        } else if let Some(at) = app.cursor {
            crate::smoove_tool::pointer(app, at, Pointer::Move);
        }
        return;
    }
    if key == Key::Escape {
        app.terrain_last = None;
        app.terrain = None;
        app.text3d = None;
        app.text_creation = None;
        app.zoom_window = None;
        app.positioning = None;
        app.walking = None;
        app.camera_drag = None;
    }
    let at = app.cursor.unwrap_or(app.canvas_centre());
    app.with_ctx(at, |tools, editor, ctx| tools.key(editor, ctx, key));
}

pub fn apply_vcb(app: &mut App3d) {
    let text = app.vcb.trim().to_string();
    if text.is_empty() {
        return;
    }
    if app.stamp.is_some() {
        app.message = crate::stamp_tool::value(app, &text).err();
        app.vcb.clear();
        return;
    }
    if app.detail.is_some() {
        app.message = crate::detail_tool::value(app, &text).err();
        app.vcb.clear();
        return;
    }
    if app.smoove.is_some() {
        app.message = crate::smoove_tool::value(app, &text).err();
        app.vcb.clear();
        return;
    }
    if app.terrain.is_some() {
        app.message = crate::terrain_tool::value(app, &text).err();
        app.vcb.clear();
        return;
    }
    if app.terrain_last.is_some() {
        app.message = crate::terrain_tool::correct(app, &text).err();
        app.vcb.clear();
        return;
    }
    if app.text3d.is_some() {
        app.message = crate::text3d_tool::distance(app, &text).err();
        app.vcb.clear();
        return;
    }
    if matches!(app.tools.active(), ToolKind::PositionCamera | ToolKind::LookAround | ToolKind::Walk) {
        let result = rysmo_model::parse_length(&text, app.editor.doc().units).map_err(|e| e.to_string());
        match result {
            Ok(height) if height.is_finite() => {
                if matches!(app.tools.active(), ToolKind::LookAround | ToolKind::Walk) {
                    app.remember_view();
                    let base_z = app.camera.eye_base_z;
                    app.camera.set_eye_height(base_z, height);
                }
                app.eye_height = height;
                app.message = None;
            }
            _ => app.message = Some("Enter a finite eye height.".into()),
        }
        app.vcb.clear();
        return;
    }
    if app.tools.active() == ToolKind::Zoom {
        let mut camera = app.camera;
        match camera.set_zoom_value(&text) {
            Ok(focal) => {
                app.remember_view();
                app.camera = camera;
                app.zoom_focal = focal;
                app.message = None;
            }
            Err(e) => app.message = Some(e),
        }
        app.vcb.clear();
        return;
    }
    if app.component_place.is_some() {
        app.message = crate::component_panel::distance(app, &text).err();
        app.vcb.clear();
        return;
    }
    let at = app.cursor.unwrap_or(app.canvas_centre());
    match app.with_ctx(at, |tools, editor, ctx| tools.vcb_enter(editor, ctx, &text)) {
        Ok(()) => {
            app.vcb.clear();
            app.message = None;
            // Show the inference for the new state at once, not at the next mouse move.
            tool_pointer(app, at, Pointer::Move);
        }
        Err(e) => {
            // Clear the text so the next letters work as shortcuts again.
            app.message = Some(format!("{e} ({text})"));
            app.vcb.clear();
        }
    }
}

pub fn undo(app: &mut App3d) {
    if app.texture_session.is_some() {
        crate::texture_view::finish(app, true);
        return;
    }
    app.zoom_window = None;
    app.tools.set_tool(app.tools.active());
    let camera = app.editor.doc().camera;
    if !app.editor.undo() {
        app.message = Some("Nothing to undo.".into());
    } else if camera != app.editor.doc().camera {
        app.camera = app.editor.doc().camera.unwrap_or_default();
    }
}

pub fn redo(app: &mut App3d) {
    app.zoom_window = None;
    app.tools.set_tool(app.tools.active());
    let camera = app.editor.doc().camera;
    if !app.editor.redo() {
        app.message = Some("Nothing to redo.".into());
    } else if camera != app.editor.doc().camera {
        app.camera = app.editor.doc().camera.unwrap_or_default();
    }
}

pub fn erase_selection(app: &mut App3d) {
    let elements = app.editor.selected();
    if elements.is_empty() {
        return;
    }
    if let Err(e) = app.editor.apply(Command::Erase { elements }) {
        app.message = Some(e.to_string());
    }
}

pub fn select_all(app: &mut App3d) {
    let active = app.editor.active();
    let document = app.editor.doc();
    let all: Vec<Element> = active
        .mesh
        .edges()
        .filter(|(_, edge)| document.style.show_hidden_geometry || !edge.hidden)
        .map(|(id, _)| Element::Edge(id))
        .chain(
            active
                .mesh
                .faces()
                .filter(|(_, f)| document.style.show_hidden_geometry || !f.hidden)
                .map(|(id, _)| Element::Face(id)),
        )
        .chain(
            active
                .instances
                .iter()
                .filter(|(_, instance)| !instance.hidden && document.tag_visible(instance.tag))
                .map(|(id, _)| Element::Instance(*id)),
        )
        .chain(active.guides.keys().copied().map(Element::Guide))
        .chain(active.dimensions.keys().copied().map(Element::Dimension))
        .chain(active.texts.keys().copied().map(Element::Text))
        .chain(active.sections.planes.keys().copied().filter(|_| document.style.section_planes).map(Element::Section))
        .collect();
    app.editor.set_selection(all);
}

fn shortcut(c: char) -> Option<ToolKind> {
    Some(match c.to_ascii_lowercase() {
        ' ' => ToolKind::Select,
        'e' => ToolKind::Eraser,
        'l' => ToolKind::Line,
        'a' => ToolKind::Arc,
        'r' => ToolKind::Rectangle,
        'c' => ToolKind::Circle,
        'p' => ToolKind::PushPull,
        'f' => ToolKind::Offset,
        'm' => ToolKind::Move,
        'q' => ToolKind::Rotate,
        's' => ToolKind::Scale,
        't' => ToolKind::TapeMeasure,
        'b' => ToolKind::PaintBucket,
        'o' => ToolKind::Orbit,
        'h' => ToolKind::Pan,
        'z' => ToolKind::Zoom,
        _ => return None,
    })
}

fn keyboard(
    mut app: ResMut<App3d>,
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    wants: Res<EguiWantsInput>,
) {
    if app.unsaved.active() || app.welcome {
        typed.clear();
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let was_shift = app.held.shift;
    app.held.shift = shift;
    app.held.ctrl = ctrl;
    app.held.alt = alt;
    if keys.just_pressed(KeyCode::F1) {
        crate::help::open_current(&mut app);
        typed.clear();
        return;
    }
    if wants.wants_any_keyboard_input() {
        if app.command_search.open && was_shift && !shift {
            app.tools.release_shift_lock();
        }
        typed.clear();
        return;
    }
    // A complete chord can arrive between two frames. Reconstruct the modifier
    // state at each key press rather than sampling only the final held state.
    let events = typed.read().cloned().collect::<Vec<_>>();
    if queued_shortcuts(&mut app, &keys, &events) {
        if app.command_search.open && was_shift && !shift {
            app.tools.release_shift_lock();
        }
        return;
    }
    if shift != was_shift {
        tool_key(&mut app, Key::Shift(shift));
    }
    if keys.any_just_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
        app.ctrl_toggle = !app.ctrl_toggle;
    }
    if ctrl {
        return;
    }
    for (code, key) in [
        (KeyCode::ArrowRight, Key::Right),
        (KeyCode::ArrowLeft, Key::Left),
        (KeyCode::ArrowUp, Key::Up),
        (KeyCode::ArrowDown, Key::Down),
    ] {
        if keys.just_pressed(code) {
            tool_key(&mut app, key);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        if app.vcb.is_empty() {
            tool_key(&mut app, Key::Escape);
            app.context_menu = None;
        }
        app.vcb.clear();
        app.message = None;
    }
    // Enter is applied in order with typed characters: "2,1" and Enter can share a frame.
    let typed_enter = events.iter().any(|ev| ev.state == ButtonState::Pressed && ev.logical_key == LogicalKey::Enter);
    if !typed_enter && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)) {
        enter(&mut app);
    }
    // Each press counts, in order with the typed characters; several can arrive in one frame.
    if events.is_empty() && keys.just_pressed(KeyCode::Backspace) {
        app.vcb.pop();
    }
    if keys.just_pressed(KeyCode::Delete) && app.vcb.is_empty() {
        erase_selection(&mut app);
    }
    for ev in &events {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        let text = match &ev.logical_key {
            LogicalKey::Character(s) => s.to_string(),
            LogicalKey::Space => " ".to_string(),
            LogicalKey::Backspace => {
                app.vcb.pop();
                continue;
            }
            LogicalKey::Enter => {
                enter(&mut app);
                continue;
            }
            _ => continue,
        };
        for c in text.chars() {
            if app.vcb.is_empty() {
                if c == 'z' && shift || c == 'Z' {
                    app.zoom_extents();
                    continue;
                }
                if c.eq_ignore_ascii_case(&'g') {
                    crate::actions::run(&mut app, crate::actions::Action::MakeComponent);
                    continue;
                }
                if let Some(t) = shortcut(c) {
                    app.set_tool(t);
                    continue;
                }
                if c == ' ' {
                    continue;
                }
            }
            if !c.is_control() {
                app.vcb.push(c);
            }
        }
    }
}

fn enter(app: &mut App3d) {
    if app.paste_place.is_some() {
        crate::object_clipboard::finish(app);
    } else if app.texture_session.is_some() {
        crate::texture_view::finish(app, false);
    } else {
        apply_vcb(app);
    }
}

/// Replay keyboard modifiers from the state before this frame. Reversing the
/// received transitions from ButtonInput's final state avoids stale modifiers
/// after focus loss and supports left/right Control and Command independently.
fn queued_shortcuts(app: &mut App3d, keys: &ButtonInput<KeyCode>, events: &[KeyboardInput]) -> bool {
    // Programmatic/headless inputs may expose ButtonInput without raw messages.
    let synthetic;
    let events = if events.is_empty() {
        synthetic = keys
            .get_just_pressed()
            .filter(|code| {
                !matches!(
                    code,
                    KeyCode::ControlLeft
                        | KeyCode::ControlRight
                        | KeyCode::SuperLeft
                        | KeyCode::SuperRight
                        | KeyCode::ShiftLeft
                        | KeyCode::ShiftRight
                        | KeyCode::AltLeft
                        | KeyCode::AltRight
                )
            })
            .map(|code| KeyboardInput {
                key_code: *code,
                logical_key: LogicalKey::Character("".into()),
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            })
            .collect::<Vec<_>>();
        &synthetic
    } else {
        events
    };
    let mut state = keys.clone();
    for event in events.iter().rev() {
        match event.state {
            ButtonState::Released => state.press(event.key_code),
            ButtonState::Pressed if !event.repeat => state.release(event.key_code),
            _ => {}
        }
    }
    let mut handled = false;
    for event in events {
        match event.state {
            ButtonState::Pressed => state.press(event.key_code),
            ButtonState::Released => state.release(event.key_code),
        }
        if event.state != ButtonState::Pressed || event.repeat {
            continue;
        }
        let ctrl =
            state.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
        let shift = state.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let alt = state.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        if alt {
            continue;
        }
        if !ctrl && shift && app.vcb.is_empty() && event.key_code == KeyCode::KeyS {
            crate::command_search::open(app);
            handled = true;
            continue;
        }
        if !ctrl {
            continue;
        }
        match event.key_code {
            KeyCode::KeyC => crate::object_clipboard::copy(app, false),
            KeyCode::KeyX => crate::object_clipboard::copy(app, true),
            KeyCode::KeyV => crate::object_clipboard::paste(app, shift),
            KeyCode::KeyD => crate::object_clipboard::duplicate(app),
            KeyCode::KeyW if shift => app.set_tool(ToolKind::ZoomWindow),
            KeyCode::KeyZ => {
                if shift {
                    redo(app)
                } else {
                    undo(app)
                }
            }
            KeyCode::KeyY => redo(app),
            KeyCode::KeyA => select_all(app),
            KeyCode::KeyS => files::save(app, shift),
            KeyCode::KeyO => crate::unsaved::request(app, crate::unsaved::Action::Open),
            KeyCode::KeyN => crate::unsaved::request(app, crate::unsaved::Action::New),
            _ => continue,
        }
        handled = true;
    }
    handled
}

/// Mouse input, read as one ordered stream so that a click lands where it happened
/// even when a slow frame has several moves and clicks queued up.
fn mouse(
    mut app: ResMut<App3d>,
    mut events: MessageReader<WindowEvent>,
    wants: Res<EguiWantsInput>,
    time: Res<Time<Real>>,
) {
    if app.unsaved.active() || app.welcome {
        events.clear();
        return;
    }
    let ui_busy = wants.wants_any_pointer_input();
    for ev in events.read() {
        let over_ui =
            |app: &App3d, p: DVec2| (ui_busy || app.over_panel(p)) && app.camera_drag.is_none() && !app.left_down;
        match ev {
            WindowEvent::CursorMoved(m) => {
                let p = dv(m.position);
                let delta = app.cursor.map(|l| p - l).unwrap_or_default() * app.preferences.orbit_sensitivity;
                app.cursor = Some(p);
                if let Some(drag) = app.camera_drag {
                    let viewport = app.canvas_size();
                    match drag {
                        CameraDrag::Middle if app.held.shift != app.preferences.shift_middle_orbits => {
                            app.camera.pan(delta, viewport)
                        }
                        CameraDrag::Orbit if app.held.shift => app.camera.pan(delta, viewport),
                        CameraDrag::Orbit | CameraDrag::Middle => {
                            let pivot = app.orbit_pivot.unwrap_or(app.camera.target);
                            app.camera.orbit_about(pivot, delta);
                        }
                        CameraDrag::LookAround => app.camera.look_around(delta),
                        CameraDrag::Pan => app.camera.pan(delta, viewport),
                        CameraDrag::Zoom => {
                            let shift = app.held.shift;
                            app.zoom_drag(delta, shift);
                        }
                    }
                } else if !over_ui(&app, p) {
                    tool_pointer(&mut app, p, Pointer::Move);
                }
            }
            WindowEvent::CursorLeft(_) => app.cursor = None,
            WindowEvent::MouseWheel(w) => {
                let Some(p) = app.cursor else { continue };
                if over_ui(&app, p) {
                    continue;
                }
                let lines = match w.unit {
                    MouseScrollUnit::Line => w.y as f64,
                    MouseScrollUnit::Pixel => w.y as f64 / 40.0,
                };
                let viewport = app.canvas_size();
                let now = time.elapsed_secs_f64();
                if app.last_wheel_time.is_none_or(|last| now - last > 0.4) {
                    app.remember_view();
                }
                app.last_wheel_time = Some(now);
                let origin = app.canvas_origin();
                let lines =
                    lines * app.preferences.zoom_sensitivity * if app.preferences.reverse_zoom { -1. } else { 1. };
                app.camera.zoom_at(0.85f64.powf(lines), p - origin, viewport);
            }
            WindowEvent::MouseButtonInput(b) => {
                if b.state == ButtonState::Released && b.button == MouseButton::Left {
                    app.walking = None;
                }
                let Some(p) = app.cursor else {
                    if b.state == ButtonState::Released {
                        app.camera_drag = None;
                        app.left_down = false;
                    }
                    continue;
                };
                let pressed = b.state == ButtonState::Pressed;
                match (b.button, pressed) {
                    (MouseButton::Middle, true) if !over_ui(&app, p) => {
                        crate::texture_view::pause_drag(&mut app);
                        app.zoom_window = None;
                        app.remember_view();
                        app.orbit_pivot = Some(app.pick_orbit_pivot(p));
                        app.camera_drag = Some(if app.tools.active() == ToolKind::Walk {
                            CameraDrag::LookAround
                        } else {
                            CameraDrag::Middle
                        });
                    }
                    (MouseButton::Middle, false) => app.camera_drag = None,
                    (MouseButton::Right, true) if !over_ui(&app, p) => open_context(&mut app, p),
                    (MouseButton::Left, true) if app.context_menu.is_some() && !over_ui(&app, p) => {
                        app.context_menu = None;
                    }
                    (MouseButton::Left, true) if !over_ui(&app, p) => {
                        app.context_menu = None;
                        if matches!(
                            app.tools.active(),
                            ToolKind::Orbit | ToolKind::Pan | ToolKind::Zoom | ToolKind::LookAround
                        ) {
                            app.remember_view();
                        }
                        match app.tools.active() {
                            ToolKind::Orbit => {
                                app.orbit_pivot = Some(app.pick_orbit_pivot(p));
                                app.camera_drag = Some(CameraDrag::Orbit);
                            }
                            ToolKind::LookAround => app.camera_drag = Some(CameraDrag::LookAround),
                            ToolKind::Pan => app.camera_drag = Some(CameraDrag::Pan),
                            ToolKind::Zoom => app.camera_drag = Some(CameraDrag::Zoom),
                            _ => {
                                let now = time.elapsed_secs_f64();
                                let clicks = match app.last_click {
                                    Some((t, q, n)) if now - t < 0.4 && q.distance(p) < 6.0 => (n + 1).min(3),
                                    _ => 1,
                                };
                                app.last_click = Some((now, p, clicks));
                                app.left_down = true;
                                tool_pointer(&mut app, p, Pointer::Move);
                                tool_pointer(&mut app, p, Pointer::Down { clicks });
                            }
                        }
                    }
                    (MouseButton::Left, false) => {
                        if app.camera_drag.is_some() {
                            app.camera_drag = None;
                        }
                        if app.left_down {
                            app.left_down = false;
                            tool_pointer(&mut app, p, Pointer::Up);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    // A menu or dialog can close over the model without a pointer move; refresh the hover
    // so the snap marker is not drawn at a stale position.
    if let Some(p) = app.cursor
        && app.hover_at != Some(p)
        && app.camera_drag.is_none()
        && !(ui_busy || app.over_panel(p))
    {
        tool_pointer(&mut app, p, Pointer::Move);
    }
}

fn touch(
    mut app: ResMut<App3d>,
    mut touches: MessageReader<TouchInput>,
    wants: Res<EguiWantsInput>,
    time: Res<Time<Real>>,
    mut ignore: Local<bool>,
) {
    if app.unsaved.active() || app.welcome {
        touches.clear();
        return;
    }
    let now = time.elapsed_secs_f64();
    let mut gestures = Vec::new();
    for t in touches.read() {
        app.touch.enabled = true;
        let phase = match t.phase {
            TouchPhase::Started => Phase::Start,
            TouchPhase::Moved => Phase::Move,
            TouchPhase::Ended => Phase::End,
            TouchPhase::Canceled => Phase::Cancel,
        };
        // A touch that starts on a panel belongs to egui until every finger lifts.
        if phase == Phase::Start && !app.touch.gestures.active() {
            *ignore = wants.is_pointer_over_area() || app.over_panel(dv(t.position));
        }
        if *ignore {
            if matches!(phase, Phase::End | Phase::Cancel) {
                app.touch.gestures.touch(t.id, phase, dv(t.position), now);
                if !app.touch.gestures.active() {
                    *ignore = false;
                }
            } else {
                app.touch.gestures.touch(t.id, phase, dv(t.position), now);
            }
            continue;
        }
        gestures.extend(app.touch.gestures.touch(t.id, phase, dv(t.position), now));
    }
    if !*ignore {
        gestures.extend(app.touch.gestures.update(now));
    }
    for g in gestures {
        handle_gesture(&mut app, g);
    }
    if !app.touch.gestures.active() {
        app.touch.navigation_started = false;
    }
}

fn handle_gesture(app: &mut App3d, g: Gesture) {
    let tool = app.tools.active();
    let viewport = app.canvas_size();
    match g {
        Gesture::Tap { pos, count } => {
            app.context_menu = None;
            if !matches!(tool, ToolKind::Orbit | ToolKind::Pan | ToolKind::Zoom | ToolKind::LookAround) {
                tool_pointer(app, pos, Pointer::Move);
                tool_pointer(app, pos, Pointer::Down { clicks: count });
                tool_pointer(app, pos, Pointer::Up);
            }
        }
        Gesture::LongPress(pos) => open_context(app, pos),
        Gesture::DragStart(pos) => {
            let navigation = matches!(tool, ToolKind::Orbit | ToolKind::Pan | ToolKind::Zoom | ToolKind::LookAround);
            let to_tool = app.texture_session.is_some()
                || (!navigation && (tool.takes_drags() || app.touch.shift || app.touch.ctrl));
            app.touch.orbiting = !to_tool;
            if !to_tool {
                if !app.touch.navigation_started {
                    app.remember_view();
                }
                app.touch.navigation_started = true;
                app.orbit_pivot = Some(app.pick_orbit_pivot(pos));
            }
            if to_tool {
                tool_pointer(app, pos, Pointer::Move);
                tool_pointer(app, pos, Pointer::Down { clicks: 1 });
            }
        }
        Gesture::DragMove { pos, delta } => {
            if app.touch.orbiting {
                match tool {
                    ToolKind::LookAround => app.camera.look_around(delta),
                    ToolKind::Pan => app.camera.pan(delta, viewport),
                    ToolKind::Orbit if app.touch.shift => app.camera.pan(delta, viewport),
                    ToolKind::Zoom => {
                        let shift = app.touch.shift;
                        app.zoom_drag(delta, shift);
                    }
                    _ => {
                        let pivot = app.orbit_pivot.unwrap_or(app.camera.target);
                        app.camera.orbit_about(pivot, delta);
                    }
                }
            } else {
                tool_pointer(app, pos, Pointer::Move);
            }
        }
        Gesture::DragEnd(pos) => {
            if !app.touch.orbiting {
                tool_pointer(app, pos, Pointer::Up);
            }
            app.touch.orbiting = false;
        }
        Gesture::DragCancel => {
            if app.texture_session.is_some() {
                crate::texture_view::pause_drag(app);
            } else if !app.touch.orbiting {
                tool_key(app, Key::Escape);
            }
            app.touch.orbiting = false;
        }
        // Two fingers tapping undo, three redo, as in other tablet editors.
        Gesture::MultiTap { fingers: 2 } => undo(app),
        Gesture::MultiTap { fingers: 3 } => redo(app),
        Gesture::MultiTap { .. } => {}
        Gesture::PanZoom { pan, zoom, centre } => {
            app.zoom_window = None;
            if !app.touch.navigation_started {
                app.remember_view();
            }
            app.touch.navigation_started = true;
            app.camera.pan(pan, viewport);
            app.camera.zoom_at(zoom, centre - app.canvas_origin(), viewport);
        }
    }
}

/// Walking continues while the pointer is held away from the starting crosshair.
fn walk_tick(mut app: ResMut<App3d>, time: Res<Time>) {
    if app.tools.active() != ToolKind::Walk || app.camera_drag.is_some() {
        return;
    }
    let Some((anchor, cursor)) = app.walking else {
        return;
    };
    if app.cursor.is_none() && !app.touch.gestures.active() {
        return;
    }
    let offset = cursor - anchor;
    let input = offset / (app.canvas_size().min_element() * 0.25).max(1.0);
    let input = input.clamp(rysmo_math::DVec2::splat(-2.0), rysmo_math::DVec2::splat(2.0));
    let mods = app.modifiers();
    let forward = rysmo_math::DVec3::new(-app.camera.yaw.cos(), -app.camera.yaw.sin(), 0.0);
    let right = forward.cross(rysmo_math::DVec3::Z);
    let vertical = if mods.shift { rysmo_math::DVec3::Z } else { forward };
    let speed = if mods.ctrl { 6.0 } else { 1.5 };
    let delta = (right * input.x - vertical * input.y) * speed * time.delta_secs_f64().min(0.1);
    let mut camera = app.camera;
    app.editor.doc().walk_camera(&mut camera, delta, mods.alt, mods.shift);
    app.camera = camera;
    app.eye_height = camera.eye().z - camera.eye_base_z;
}

#[cfg(test)]
mod search_shortcut_tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn shift_s_opens_search_without_locking_the_line_or_stealing_segment_input() {
        for measurements in ["", "24"] {
            let mut model = App3d::new(rysmo_model::Editor::new());
            model.set_tool(ToolKind::Line);
            let start = model.local_view().project(DVec3::ZERO).unwrap();
            model.with_ctx(start, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Down { clicks: 1 }));
            let along = model.local_view().project(DVec3::X * 2.).unwrap();
            model.cursor = Some(along);
            model.with_ctx(along, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
            model.vcb = measurements.into();
            let before = model.editor.doc().clone();
            let mut test = bevy::prelude::App::new();
            test.insert_resource(model)
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<EguiWantsInput>()
                .add_message::<KeyboardInput>()
                .add_systems(bevy::prelude::Update, keyboard);
            let mut keys = test.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ShiftLeft);
            keys.press(KeyCode::KeyS);
            test.update();
            let mut model = test.world_mut().resource_mut::<App3d>();
            assert_eq!(model.command_search.open, measurements.is_empty());
            assert_eq!(model.vcb, measurements);
            assert_eq!(model.editor.doc(), &before);
            if measurements.is_empty() {
                let point = model.local_view().project(DVec3::new(2., 1., 0.)).unwrap();
                model.with_ctx(point, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
                let inferred = model.tools.preview(&model.editor).inference.unwrap().point;
                assert!(inferred.y > 0.8, "search must not leave an X-axis lock: {inferred:?}");
            }
        }
    }
}

#[cfg(test)]
mod queued_keyboard_tests {
    use super::*;
    use rysmo_math::DVec3;
    fn rig() -> App {
        let mut editor = rysmo_model::Editor::new();
        let out = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let group = editor.apply(Command::MakeGroup { elements: out.created }).unwrap().created;
        editor.set_selection(group);
        let mut app = App::new();
        app.insert_resource(App3d::new(editor))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EguiWantsInput>()
            .add_message::<KeyboardInput>()
            .add_systems(Update, keyboard);
        app
    }
    fn events(app: &mut App, sequence: &[(KeyCode, bool)]) {
        for &(code, pressed) in sequence {
            let logical_key = match code {
                KeyCode::ControlLeft | KeyCode::ControlRight => LogicalKey::Control,
                KeyCode::ShiftLeft | KeyCode::ShiftRight => LogicalKey::Shift,
                KeyCode::SuperLeft | KeyCode::SuperRight => LogicalKey::Super,
                KeyCode::Backspace => LogicalKey::Backspace,
                KeyCode::Enter => LogicalKey::Enter,
                _ => LogicalKey::Character(
                    match code {
                        KeyCode::Digit1 => "1",
                        KeyCode::Digit2 => "2",
                        KeyCode::Digit3 => "3",
                        KeyCode::Digit4 => "4",
                        KeyCode::KeyC => "c",
                        KeyCode::KeyD => "d",
                        KeyCode::KeyS => "s",
                        KeyCode::KeyZ => "z",
                        _ => "r",
                    }
                    .into(),
                ),
            };
            if pressed {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(code);
            } else {
                app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(code);
            }
            app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key,
                state: if pressed { ButtonState::Pressed } else { ButtonState::Released },
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        }
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    }
    fn chord(app: &mut App, modifier: KeyCode, key: KeyCode) {
        events(app, &[(modifier, true), (key, true), (key, false), (modifier, false)]);
    }
    #[test]
    fn every_backspace_in_one_frame_deletes_in_typing_order() {
        let mut app = rig();
        let mut sequence = Vec::new();
        for code in
            [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Backspace, KeyCode::Backspace, KeyCode::Digit4]
        {
            sequence.extend([(code, true), (code, false)]);
        }
        events(&mut app, &sequence);
        assert_eq!(app.world().resource::<App3d>().vcb, "14");
    }
    #[test]
    fn enter_in_one_frame_applies_only_the_characters_typed_before_it() {
        let mut app = rig();
        let mut sequence = Vec::new();
        for code in [KeyCode::Digit2, KeyCode::Enter, KeyCode::Digit3] {
            sequence.extend([(code, true), (code, false)]);
        }
        events(&mut app, &sequence);
        assert_eq!(app.world().resource::<App3d>().vcb, "3");
    }
    #[test]
    fn complete_copy_and_duplicate_chords_in_one_frame_do_not_activate_tools() {
        let mut app = rig();
        chord(&mut app, KeyCode::ControlLeft, KeyCode::KeyC);
        assert!(app.world().resource::<App3d>().object_clipboard.is_some());
        assert_eq!(app.world().resource::<App3d>().tools.active(), ToolKind::Select);
        let before = app.world().resource::<App3d>().editor.doc().clone();
        chord(&mut app, KeyCode::SuperLeft, KeyCode::KeyD);
        assert!(app.world().resource::<App3d>().paste_place.is_some());
        crate::object_clipboard::finish(app.world_mut().resource_mut::<App3d>().into_inner());
        assert_eq!(app.world().resource::<App3d>().editor.active().instances.len(), 2);
        chord(&mut app, KeyCode::ControlRight, KeyCode::KeyZ);
        assert_eq!(app.world().resource::<App3d>().editor.doc(), &before);
    }
    #[test]
    fn fast_shift_search_and_redo_keep_their_modifiers() {
        let mut app = rig();
        let state = app.world_mut().resource_mut::<App3d>().into_inner();
        state.editor.apply(Command::Move { elements: state.editor.selected(), offset: DVec3::X }).unwrap();
        let moved = state.editor.doc().clone();
        assert!(state.editor.undo());
        events(
            &mut app,
            &[
                (KeyCode::ControlLeft, true),
                (KeyCode::ShiftLeft, true),
                (KeyCode::KeyZ, true),
                (KeyCode::KeyZ, false),
                (KeyCode::ShiftLeft, false),
                (KeyCode::ControlLeft, false),
            ],
        );
        assert_eq!(app.world().resource::<App3d>().editor.doc(), &moved);
        chord(&mut app, KeyCode::ShiftLeft, KeyCode::KeyS);
        assert!(app.world().resource::<App3d>().command_search.open);
        assert_eq!(app.world().resource::<App3d>().tools.active(), ToolKind::Select);
    }
    #[test]
    fn modifier_pressed_after_a_character_does_not_retroactively_make_a_shortcut() {
        let mut app = rig();
        events(
            &mut app,
            &[
                (KeyCode::KeyC, true),
                (KeyCode::KeyC, false),
                (KeyCode::ControlLeft, true),
                (KeyCode::ControlLeft, false),
            ],
        );
        assert!(app.world().resource::<App3d>().object_clipboard.is_none());
        assert_eq!(app.world().resource::<App3d>().tools.active(), ToolKind::Circle);
    }
}

/// Right-click preserves an existing multi-selection when its target is already selected.
fn open_context(app: &mut App3d, at: DVec2) {
    if app.texture_session.is_none() {
        let hit = app
            .pick_annotation(at)
            .unwrap_or_else(|| app.with_ctx(at, |tools, editor, ctx| tools.pick_full(editor, ctx)));
        if let rysmo_infer::Pick::Element(element) = hit {
            if !app.editor.selection().contains(&element) {
                app.editor.set_selection(vec![element]);
            }
        } else {
            app.editor.set_selection(Vec::new());
        }
    }
    app.context_menu = Some(at);
    app.context_menu_armed = false;
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn right_click_selects_its_target_without_running_the_active_tool() {
        use rysmo_math::DVec3;
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        let edge = app.editor.mesh().edges().next().unwrap().0;
        app.set_tool(ToolKind::PushPull);
        let at = app.view().project(DVec3::new(0.45, 0.5, 0.)).unwrap();
        let before = app.editor.doc().clone();
        open_context(&mut app, at);
        assert_eq!(app.editor.selected(), vec![Element::Face(face)]);
        assert_eq!(app.tools.active(), ToolKind::PushPull);
        app.editor.set_selection(vec![Element::Face(face), Element::Edge(edge)]);
        open_context(&mut app, at);
        assert_eq!(app.editor.selection().len(), 2);
        assert_eq!(&before, app.editor.doc());
    }
}
