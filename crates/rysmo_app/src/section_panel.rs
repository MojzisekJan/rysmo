use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, DVec3};
use rysmo_model::{Command, sections::SectionPlane};
use rysmo_tools::{Pointer, ToolKind};

pub fn begin(app: &mut App3d) {
    app.set_tool(ToolKind::Select);
    app.show_sections = true;
    app.section_placing = Some((app.editor.revision(), app.editor.context().to_vec()));
    app.message = Some("Section Plane: click a face. Escape cancels.".into());
}

pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    let Some((revision, context)) = &app.section_placing else { return };
    if *revision != app.editor.revision() || context != app.editor.context() {
        app.section_placing = None;
        return;
    }
    if !matches!(event, Pointer::Down { .. }) {
        return;
    }
    let ray = app.view().ray(at);
    let mut best = f64::INFINITY;
    let mut hit = None;
    for occurrence in app.editor.doc().occurrences(app.editor.context()) {
        let Some(definition) = app.editor.doc().def(occurrence.definition) else { continue };
        let inverse = occurrence.transform.inverse();
        let local = rysmo_math::Ray::new(inverse.transform_point3(ray.origin), inverse.transform_vector3(ray.dir));
        for (id, _) in definition.mesh.faces() {
            let Some(plane) = definition.mesh.face_plane(id) else { continue };
            let Some(distance) = plane.intersect_ray(&local) else { continue };
            let point = local.at(distance);
            if !definition.mesh.face_contains(id, point) {
                continue;
            }
            let world = occurrence.transform.transform_point3(point);
            let distance = (world - ray.origin).dot(ray.dir);
            if distance < 0. || distance >= best {
                continue;
            }
            if app.editor.doc().section_planes(&occurrence.path).iter().any(|p| p.distance(world) > 1e-9) {
                continue;
            }
            let mut normal = inverse.transpose().transform_vector3(plane.normal).normalize();
            if normal.dot(ray.dir) > 0. {
                normal = -normal;
            }
            let transform = app.editor.context_transform();
            hit = occurrence.in_context.then(|| SectionPlane {
                name: "Section".into(),
                origin: transform.inverse().transform_point3(world),
                normal: transform.transpose().transform_vector3(normal).normalize(),
            });
            best = distance;
        }
    }
    if let Some(plane) = hit {
        match app.editor.apply(Command::AddSection { plane }) {
            Ok(out) => {
                app.editor.set_selection(out.created);
                app.message = None;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
        app.section_placing = None;
    } else {
        app.message = Some("Click a visible face in the editing context.".into());
    }
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.show_sections {
        return;
    }
    let mut open = true;
    let state = app.editor.active().sections.clone();
    let mut command = None;
    let mut place = false;
    let mut choose = None;
    let mut style = app.editor.doc().style;
    let mut styled = false;
    let window = crate::theme::window("Section Planes")
        .beside_model()
        .open(&mut open)
        .vscroll(true)
        .max_height((ctx.content_rect().height() - 120.).max(200.))
        .show(ctx, |ui| {
            // Only edits count: a colour button's round trip can change the stored value.
            styled |= crate::widgets::checkbox(ui, &mut style.section_planes, "Show planes").changed();
            styled |= crate::widgets::checkbox(ui, &mut style.section_cuts, "Enable cuts").changed();
            styled |= crate::widgets::checkbox(ui, &mut style.section_lines, "Show cut edges").changed();
            ui.horizontal(|ui| {
                ui.label("Cut colour");
                styled |= ui.color_edit_button_rgb(&mut style.section_color).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Active plane");
                styled |= ui.color_edit_button_rgb(&mut style.section_active_color).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Inactive plane");
                styled |= ui.color_edit_button_rgb(&mut style.section_inactive_color).changed();
            });
            styled |= ui
                .add(
                    crate::widgets::Number::new(&mut style.section_width)
                        .prefix("Cut width ")
                        .suffix(" px")
                        .range(0.5..=100.),
                )
                .changed();
            if ui.button("Place on a face").clicked() {
                place = true;
            }
            if app.section_placing.is_some() {
                ui.label("Click a face. Escape cancels placement.");
            }
            ui.horizontal(|ui| {
                for (label, normal) in [("Add X", DVec3::X), ("Add Y", DVec3::Y), ("Add Z", DVec3::Z)] {
                    if ui.button(label).clicked() {
                        let mut origin = app.editor.active().axes.origin;
                        if let Some((lo, hi)) = app.editor.doc().definition_bounds(app.editor.active_definition()) {
                            origin = (lo + hi) * 0.5;
                        }
                        command = Some(Command::AddSection {
                            plane: SectionPlane { name: format!("Section {}", state.planes.len() + 1), origin, normal },
                        });
                    }
                }
            });
            ui.separator();
            for (id, saved) in &state.planes {
                ui.push_id(id, |ui| {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(state.active == Some(*id), &saved.name).clicked() {
                            command = Some(Command::ActivateSection { id: Some(*id) });
                        }
                        if ui.small_button("Select").clicked() {
                            choose = Some((*id, ToolKind::Select));
                        }
                        if ui.small_button("Move").clicked() {
                            choose = Some((*id, ToolKind::Move));
                        }
                        if ui.small_button("Rotate").clicked() {
                            choose = Some((*id, ToolKind::Rotate));
                        }
                        if ui.small_button("Delete").clicked() {
                            command = Some(Command::SetSection { id: *id, plane: None });
                        }
                    });
                    if state.active == Some(*id) || app.editor.is_selected(rysmo_model::Element::Section(*id)) {
                        let mut plane = saved.clone();
                        ui.add(crate::theme::singleline(&mut plane.name).desired_width(f32::INFINITY));
                        ui.horizontal(|ui| {
                            ui.label("Origin (m)");
                            for value in [&mut plane.origin.x, &mut plane.origin.y, &mut plane.origin.z] {
                                ui.add(crate::widgets::Number::new(value).speed(0.01));
                            }
                        });
                        let mut distance = plane.origin.dot(plane.normal);
                        if ui
                            .add(crate::widgets::Number::new(&mut distance).prefix("Offset ").suffix(" m").speed(0.01))
                            .changed()
                        {
                            plane.origin += plane.normal * (distance - plane.origin.dot(plane.normal));
                        }
                        let mut azimuth = plane.normal.y.atan2(plane.normal.x).to_degrees();
                        let mut elevation = plane.normal.z.clamp(-1., 1.).asin().to_degrees();
                        let mut changed = false;
                        ui.horizontal(|ui| {
                            changed |= ui
                                .add(crate::widgets::Number::new(&mut azimuth).prefix("Azimuth ").suffix("°"))
                                .changed();
                            changed |= ui
                                .add(
                                    crate::widgets::Number::new(&mut elevation)
                                        .prefix("Elevation ")
                                        .suffix("°")
                                        .range(-90. ..=90.),
                                )
                                .changed();
                        });
                        if changed {
                            let (a, e) = (azimuth.to_radians(), elevation.to_radians());
                            plane.normal = DVec3::new(e.cos() * a.cos(), e.cos() * a.sin(), e.sin());
                        }
                        if ui.button("Create group from slice").clicked() {
                            command = Some(Command::CreateSectionSlice { id: *id });
                        }
                        if ui.button("Reverse direction").clicked() {
                            plane.normal = -plane.normal;
                        }
                        if plane != *saved {
                            command = Some(Command::SetSection { id: *id, plane: Some(plane) });
                        }
                    }
                });
            }
            if state.active.is_some() && ui.button("Deactivate cut").clicked() {
                command = Some(Command::ActivateSection { id: None });
            }
        });
    app.show_sections = open;
    if let Some(window) = window {
        let r = window.response.rect;
        app.ui_rects.push((DVec2::new(r.min.x as f64, r.min.y as f64), DVec2::new(r.max.x as f64, r.max.y as f64)));
    }
    if styled && style != app.editor.doc().style {
        app.message = app.editor.apply(Command::SetStyle { style }).err().map(|e| e.to_string());
    }
    if let Some(command) = command {
        match app.editor.apply(command) {
            Ok(out) => {
                if !out.created.is_empty() {
                    app.editor.set_selection(out.created);
                }
                app.message = None;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
    if let Some((id, tool)) = choose {
        app.set_tool(tool);
        app.editor.set_selection([rysmo_model::Element::Section(id)]);
    }
    if place {
        begin(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn face_placement_is_one_history_step_and_escape_cancels() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::Z * 2.,
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        app.camera.set_standard_view(rysmo_math::StandardView::Top);
        app.zoom_extents();
        let before = app.editor.doc().clone();
        app.section_placing = Some((app.editor.revision(), Vec::new()));
        let at = app.local_view().project(DVec3::new(1., 1., 2.)).unwrap();
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.section_placing.is_none());
        let id = app.editor.active().sections.active.unwrap();
        let plane = &app.editor.active().sections.planes[&id];
        assert!((plane.origin.z - 2.).abs() < 1e-9);
        assert!(plane.normal.distance(DVec3::Z) < 1e-9);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        app.section_placing = Some((app.editor.revision(), Vec::new()));
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.section_placing.is_none());
        assert_eq!(app.editor.doc(), &before);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use rysmo_model::{Editor, Element};
    fn sample() -> (App3d, u64) {
        let mut app = App3d::new(Editor::new());
        app.camera.target = DVec3::new(0., 0., 1.);
        app.camera.distance = 8.;
        app.canvas = Some((DVec2::new(0., 90.), DVec2::new(900., 650.)));
        let out = app
            .editor
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Cut".into(), origin: DVec3::Z, normal: DVec3::Z },
            })
            .unwrap();
        let Element::Section(id) = out.created[0] else { panic!() };
        (app, id)
    }
    #[test]
    fn glyph_click_selects_then_moves_a_plane_with_exact_input() {
        let (mut app, id) = sample();
        let plane = &app.editor.active().sections.planes[&id];
        let line = rysmo_model::sections::glyph(plane, None)[0];
        let grab = (line[0] + line[1]) * 0.5;
        let pixel = app.view().project(grab).unwrap();
        crate::input::tool_pointer(&mut app, pixel, Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, pixel, Pointer::Up);
        assert!(app.editor.is_selected(Element::Section(id)));
        app.set_tool(ToolKind::Move);
        crate::input::tool_pointer(&mut app, pixel, Pointer::Down { clicks: 1 });
        let inferred = app.tools.preview(&app.editor).inference.unwrap().point;
        assert!(inferred.distance(grab) < 1e-8);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Up);
        let end = app.view().project(grab + DVec3::Z).unwrap();
        crate::input::tool_pointer(&mut app, end, Pointer::Move);
        app.vcb = "500".into();
        crate::input::apply_vcb(&mut app);
        assert!(app.editor.active().sections.planes[&id].origin.distance(DVec3::Z * 1.5) < 1e-8);
        crate::input::undo(&mut app);
        assert_eq!(app.editor.active().sections.planes[&id].origin, DVec3::Z);
    }
    #[test]
    fn selected_section_rotates_about_locked_axis_and_undo_restores_it() {
        let (mut app, id) = sample();
        app.editor.set_selection([Element::Section(id)]);
        app.set_tool(ToolKind::Rotate);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Left);
        let line = rysmo_model::sections::glyph(&app.editor.active().sections.planes[&id], None)[0];
        let pivot = (line[0] + line[1]) * 0.5;
        let pixel = app.view().project(pivot).unwrap();
        crate::input::tool_pointer(&mut app, pixel, Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, pixel, Pointer::Up);
        let pixel = app.view().project(pivot + DVec3::X).unwrap();
        crate::input::tool_pointer(&mut app, pixel, Pointer::Move);
        crate::input::tool_pointer(&mut app, pixel, Pointer::Down { clicks: 1 });
        crate::input::tool_pointer(&mut app, pixel, Pointer::Up);
        app.vcb = "90".into();
        crate::input::apply_vcb(&mut app);
        assert!(app.editor.active().sections.planes[&id].normal.distance(DVec3::X) < 1e-8);
        crate::input::undo(&mut app);
        assert_eq!(app.editor.active().sections.planes[&id].normal, DVec3::Z);
    }
    #[test]
    fn section_window_crossing_selection_and_visibility_match_rendered_glyph() {
        let (mut app, id) = sample();
        let lines = rysmo_model::sections::glyph(&app.editor.active().sections.planes[&id], None);
        let projected = lines
            .iter()
            .flat_map(|[a, b]| [app.view().project(*a).unwrap(), app.view().project(*b).unwrap()])
            .collect::<Vec<_>>();
        let lo = projected.iter().copied().reduce(DVec2::min).unwrap() - DVec2::splat(1.);
        let hi = projected.iter().copied().reduce(DVec2::max).unwrap() + DVec2::splat(1.);
        assert!(app.annotation_box(lo, hi).elements.contains(&Element::Section(id)));
        let edge = (projected[0] + projected[1]) * 0.5;
        let a = edge - DVec2::splat(4.);
        let b = edge + DVec2::splat(4.);
        assert!(!app.annotation_box(a, b).elements.contains(&Element::Section(id)));
        assert!(app.annotation_box(b, a).elements.contains(&Element::Section(id)));
        let mut style = app.editor.doc().style;
        style.section_planes = false;
        app.editor.apply(Command::SetStyle { style }).unwrap();
        assert!(app.pick_annotation(edge).is_none());
        assert!(!app.annotation_box(lo, hi).elements.contains(&Element::Section(id)));
    }
}
