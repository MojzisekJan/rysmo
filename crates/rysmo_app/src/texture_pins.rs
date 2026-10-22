//! A face-plane four-pin editor, independent of the camera perspective.
use bevy_egui::egui;
use rysmo_math::DVec2;
use rysmo_model::{TexturePlacement, pin_mapping};

pub const TILE: [[f64; 2]; 4] = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];

/// Fixed pins preserve their documented constraints; free pins move one corner.
pub fn drag(pins: [[f64; 2]; 4], index: usize, target: DVec2, fixed: bool) -> Option<[[f64; 2]; 4]> {
    let mut p = pins.map(DVec2::from_array);
    if !fixed || index == 2 {
        p[index] = target;
    } else if index == 0 {
        let delta = target - p[0];
        p = p.map(|v| v + delta);
    } else if index == 1 {
        let old = p[1] - p[0];
        let new = target - p[0];
        if old.length_squared() < 1e-12 {
            return None;
        }
        let c = new.dot(old) / old.length_squared();
        let s = old.perp_dot(new) / old.length_squared();
        let origin = p[0];
        p = p.map(|v| {
            let q = v - origin;
            origin + DVec2::new(c * q.x - s * q.y, s * q.x + c * q.y)
        });
    } else {
        let delta = target - p[3];
        p[2] += delta;
        p[3] = target;
    }
    let pins = p.map(|v| v.to_array());
    pin_mapping(pins).map(|_| pins)
}

pub fn controls(ui: &mut egui::Ui, placement: &mut TexturePlacement) -> bool {
    let id = ui.id().with("texture-pins-fixed");
    let mut fixed = ui.ctx().data_mut(|data| data.get_temp::<bool>(id).unwrap_or(true));
    crate::widgets::checkbox(ui, &mut fixed, "Fixed pins");
    ui.ctx().data_mut(|data| data.insert_temp(id, fixed));
    ui.weak(if fixed {
        "Red: move · Green: rotate/scale · Blue: shear/scale · Yellow: perspective"
    } else {
        "Drag any corner to position the texture."
    });
    let pins = placement.pins.unwrap_or(TILE);
    let available = ui.available_width();
    // Keep the previous fallback for an unavailable layout measurement.
    let width = if available.is_nan() { 340. } else { available.clamp(160., 340.) };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, width * 0.8), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    // Keep the coordinate frame stable throughout a drag, so moving a pin cannot move the widget.
    let view_id = ui.id().with("texture-pins-view");
    let fit = || {
        let points = pins.map(DVec2::from_array);
        let lo = points.into_iter().reduce(DVec2::min).unwrap();
        let hi = points.into_iter().reduce(DVec2::max).unwrap();
        let span = ((hi.x - lo.x).max((hi.y - lo.y) / 0.8).max(0.2)) * 1.6;
        ((lo + hi) / 2., span)
    };
    let (center, span) = ui.ctx().data_mut(|data| {
        let view = data.get_temp::<(DVec2, f64)>(view_id).unwrap_or_else(fit);
        data.insert_temp(view_id, view);
        view
    });
    let unit = width / span as f32;
    let origin = rect.center() - egui::vec2(center.x as f32 * unit, center.y as f32 * unit);
    let screen = |p: [f64; 2]| origin + egui::vec2(p[0] as f32 * unit, p[1] as f32 * unit);
    let mapping = pin_mapping(pins).expect("validated placement").inverse();
    for axis in 0..2 {
        for line in 0..=4 {
            let a = if axis == 0 { [line as f64 / 4., 0.] } else { [0., line as f64 / 4.] };
            let b = if axis == 0 { [line as f64 / 4., 1.] } else { [1., line as f64 / 4.] };
            let project = |p: [f64; 2]| {
                let q = mapping * rysmo_math::DVec3::new(p[0], p[1], 1.);
                screen([q.x / q.z, q.y / q.z])
            };
            painter.line_segment([project(a), project(b)], egui::Stroke::new(1., ui.visuals().weak_text_color()));
        }
    }
    let colors = [
        egui::Color32::from_rgb(230, 85, 65),
        egui::Color32::from_rgb(85, 185, 105),
        egui::Color32::from_rgb(240, 195, 65),
        egui::Color32::from_rgb(80, 150, 235),
    ];
    let names = if fixed {
        ["Move", "Rotate and scale", "Perspective", "Shear and scale"]
    } else {
        ["Corner 1", "Corner 2", "Corner 3", "Corner 4"]
    };
    let mut changed = false;
    for i in 0..4 {
        let pos = screen(pins[i]);
        let response = ui
            .interact(
                egui::Rect::from_center_size(pos, egui::vec2(24., 24.)).intersect(rect),
                ui.id().with(("texture-pin", i)),
                egui::Sense::drag(),
            )
            .on_hover_text(names[i]);
        painter.circle_filled(pos, if response.hovered() || response.dragged() { 8. } else { 6. }, colors[i]);
        painter.circle_stroke(pos, 8., egui::Stroke::new(1., ui.visuals().text_color()));
        if response.dragged()
            && let Some(cursor) = response.interact_pointer_pos()
        {
            let q = (cursor - origin) / unit;
            if let Some(next) = drag(pins, i, DVec2::new(q.x as f64, q.y as f64), fixed) {
                placement.pins = Some(next);
                changed = next != pins;
            }
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Reset pins").clicked() {
            changed |= placement.pins.take().is_some();
        }
        if ui.button("Fit pins").clicked() {
            ui.ctx().data_mut(|data| data.insert_temp(view_id, fit()));
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_pins_preserve_their_anchors() {
        let moved = drag(TILE, 0, DVec2::new(2., 3.), true).unwrap();
        assert_eq!(moved[2], [3., 4.]);
        let rotated = drag(TILE, 1, DVec2::new(0., 2.), true).unwrap();
        assert_eq!(rotated[0], TILE[0]);
        assert_eq!(rotated[3], [-2., 0.]);
        let sheared = drag(TILE, 3, DVec2::new(0.5, 2.), true).unwrap();
        assert_eq!(sheared[..2], TILE[..2]);
        assert_eq!(sheared[2], [1.5, 2.]);
    }
    #[test]
    fn free_and_perspective_pins_keep_other_corners_and_reject_folds() {
        for fixed in [false, true] {
            let p = drag(TILE, 2, DVec2::new(1.2, 0.8), fixed).unwrap();
            for i in [0, 1, 3] {
                assert_eq!(p[i], TILE[i]);
            }
            assert!(drag(TILE, 2, DVec2::new(-1., -1.), fixed).is_none());
        }
    }
    #[test]
    fn pointer_drag_moves_all_fixed_pins_without_moving_the_diagram() {
        let ctx = egui::Context::default();
        let mut placement = TexturePlacement::default();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400., 500.))),
            events,
            ..Default::default()
        };
        let frame = |placement: &mut TexturePlacement, events| {
            let mut output = ctx.run_ui(input(events), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    controls(ui, placement);
                });
            });
            output.textures_delta.clear();
            output
        };
        let out = frame(&mut placement, vec![]);
        let red = out
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Circle(circle) if circle.fill == egui::Color32::from_rgb(230, 85, 65) => {
                    Some(circle.center)
                }
                _ => None,
            })
            .unwrap();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut placement, vec![egui::Event::PointerMoved(red), button(red, true)]);
        frame(&mut placement, vec![egui::Event::PointerMoved(red + egui::vec2(30., 20.))]);
        let pins = placement.pins.unwrap();
        assert!(pins[0][0] > 0. && pins[0][1] > 0.);
        for i in 1..4 {
            assert!((pins[i][0] - pins[0][0] - TILE[i][0]).abs() < 1e-8);
            assert!((pins[i][1] - pins[0][1] - TILE[i][1]).abs() < 1e-8);
        }
        frame(&mut placement, vec![button(red + egui::vec2(30., 20.), false)]);
        assert_eq!(placement.pins, Some(pins));
    }
}
