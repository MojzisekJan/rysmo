//! Rysmo's vector tool icons. Geometry and hit areas stay fixed in every interaction state.
use crate::actions::Action;
use bevy_egui::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke};
use rysmo_tools::ToolKind;

#[derive(Clone, Copy)]
pub enum Icon {
    Tool(ToolKind),
    Action(Action),
    Extents,
}

fn chrome(ui: &egui::Ui, r: &Response, selected: bool) -> Color32 {
    let hovered = r.hovered();
    let fill = if selected && hovered {
        Color32::from_rgb(43, 77, 119)
    } else if selected {
        Color32::from_rgb(35, 61, 96)
    } else if hovered {
        Color32::from_rgb(49, 53, 62)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r.rect.shrink(1.), 6., fill);
    if selected {
        ui.painter().rect_stroke(
            r.rect.shrink(1.5),
            6.,
            Stroke::new(1., crate::theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    if selected {
        Color32::from_rgb(105, 169, 255)
    } else if hovered {
        Color32::from_rgb(238, 242, 250)
    } else {
        Color32::from_rgb(157, 166, 181)
    }
}

pub fn button(ui: &mut egui::Ui, icon: Icon, selected: bool, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size + 14., size + 14.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            selected,
            match icon {
                Icon::Tool(tool) => tool.name(),
                Icon::Action(action) => action.label(),
                Icon::Extents => "Zoom Extents",
            },
        )
    });
    let color = chrome(ui, &response, selected);
    draw(ui, Rect::from_center_size(rect.center(), egui::vec2(size, size)), icon, color);
    response
}

pub fn tab(ui: &mut egui::Ui, label: &str, selected: bool) -> Response {
    #[cfg(target_os = "macos")]
    return ui.add(egui_native_look::Tab::new(label, selected));
    #[cfg_attr(target_os = "macos", allow(unreachable_code))]
    let font = egui::FontId::proportional(14.);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(galley.size().x + 16., 30.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, label)
    });
    let hover_color = chrome(ui, &response, false);
    let color = if selected { Color32::from_rgb(105, 169, 255) } else { hover_color };
    ui.painter().galley(rect.center() - galley.size() * 0.5, galley, color);
    if selected {
        ui.painter().line_segment(
            [rect.left_bottom() + egui::vec2(8., -1.), rect.right_bottom() + egui::vec2(-8., -1.)],
            Stroke::new(2., crate::theme::ACCENT),
        );
    }
    response
}

struct Pen<'a> {
    painter: &'a egui::Painter,
    rect: Rect,
    stroke: Stroke,
}
impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + egui::vec2(x, y) * (self.rect.width() / 24.)
    }
    fn line(&self, points: &[(f32, f32)]) {
        self.painter.add(egui::Shape::line(points.iter().map(|&(x, y)| self.p(x, y)).collect(), self.stroke));
    }
    fn bezier(&self, points: [(f32, f32); 4]) {
        self.painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            points.map(|(x, y)| self.p(x, y)),
            false,
            Color32::TRANSPARENT,
            self.stroke,
        ));
    }
    fn rounded_rect(&self, min: (f32, f32), max: (f32, f32), radius: f32) {
        self.painter.rect_stroke(
            Rect::from_min_max(self.p(min.0, min.1), self.p(max.0, max.1)),
            radius * self.rect.width() / 24.,
            self.stroke,
            egui::StrokeKind::Middle,
        );
    }
    fn ring(&self, x: f32, y: f32, r: f32) {
        self.painter.circle_stroke(self.p(x, y), r * self.rect.width() / 24., self.stroke);
    }
    fn arc(&self, x: f32, y: f32, r: f32, start: f32, end: f32) {
        let points: Vec<_> = (0..=24)
            .map(|i| {
                let a = (start + (end - start) * i as f32 / 24.).to_radians();
                (x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.line(&points);
    }
    fn arrow(&self, a: (f32, f32), b: (f32, f32)) {
        self.line(&[a, b]);
        let d = egui::vec2(b.0 - a.0, b.1 - a.1).normalized();
        let n = egui::vec2(-d.y, d.x);
        self.line(&[
            (b.0 - d.x * 3. + n.x * 2., b.1 - d.y * 3. + n.y * 2.),
            b,
            (b.0 - d.x * 3. - n.x * 2., b.1 - d.y * 3. - n.y * 2.),
        ]);
    }
    fn colored(&self, color: Color32) -> Pen<'_> {
        Pen { painter: self.painter, rect: self.rect, stroke: Stroke::new(1.55, color) }
    }
    fn polygon(&self, points: &[(f32, f32)], alpha: u8) {
        self.painter.add(egui::Shape::convex_polygon(
            points.iter().map(|&(x, y)| self.p(x, y)).collect(),
            self.stroke.color.gamma_multiply(alpha as f32 / 255.),
            self.stroke,
        ));
    }
    fn cube(&self) {
        self.polygon(&[(4., 10.), (12., 6.), (20., 10.), (12., 14.)], 40);
        self.polygon(&[(4., 10.), (12., 14.), (12., 22.), (4., 18.)], 18);
        self.polygon(&[(12., 14.), (20., 10.), (20., 18.), (12., 22.)], 28);
    }
    /// Elliptical path with an arrowhead tangent to its actual endpoint.
    fn curved_arrow(&self, centre: (f32, f32), radii: (f32, f32), start: f32, end: f32) {
        let points: Vec<_> = (0..=40)
            .map(|i| {
                let a = (start + (end - start) * i as f32 / 40.).to_radians();
                (centre.0 + radii.0 * a.cos(), centre.1 + radii.1 * a.sin())
            })
            .collect();
        self.line(&points);
        let angle = end.to_radians();
        let tip = *points.last().unwrap();
        let tangent = egui::vec2(-radii.0 * angle.sin(), radii.1 * angle.cos()).normalized() * (end - start).signum();
        let normal = egui::vec2(-tangent.y, tangent.x);
        self.line(&[
            (tip.0 - tangent.x * 3. + normal.x * 1.8, tip.1 - tangent.y * 3. + normal.y * 1.8),
            tip,
            (tip.0 - tangent.x * 3. - normal.x * 1.8, tip.1 - tangent.y * 3. - normal.y * 1.8),
        ]);
    }
}

fn draw(ui: &egui::Ui, rect: Rect, icon: Icon, color: Color32) {
    let body = if color == Color32::from_rgb(105, 169, 255) { Color32::from_rgb(182, 193, 209) } else { color };
    let p = Pen { painter: ui.painter(), rect, stroke: Stroke::new(1.25, body) };
    let action = p.colored(Color32::from_rgb(239, 155, 67));
    let guide = p.colored(Color32::from_rgb(89, 157, 240));
    let measure = p.colored(Color32::from_rgb(112, 199, 142));
    match icon {
        Icon::Tool(ToolKind::Select) => {
            let points = [(5., 2.), (5., 19.), (9.3, 14.8), (12.8, 22.), (16.1, 20.4), (12.6, 13.4), (19., 13.4)];
            let mut fill = egui::Mesh::default();
            for (x, y) in points {
                fill.colored_vertex(p.p(x, y), guide.stroke.color.gamma_multiply(0.22));
            }
            for [a, b, c] in [[0, 1, 2], [0, 2, 5], [0, 5, 6], [2, 3, 4], [2, 4, 5]] {
                fill.add_triangle(a, b, c);
            }
            p.painter.add(egui::Shape::mesh(fill));
            guide.line(&[
                (5., 2.),
                (5., 19.),
                (9.3, 14.8),
                (12.8, 22.),
                (16.1, 20.4),
                (12.6, 13.4),
                (19., 13.4),
                (5., 2.),
            ]);
        }
        Icon::Tool(ToolKind::Eraser) => {
            action.line(&[(3., 14.5), (14.8, 2.7)]);
            action.bezier([(14.8, 2.7), (15.7, 1.8), (16.8, 1.8), (17.7, 2.7)]);
            action.line(&[(17.7, 2.7), (21.3, 6.3)]);
            action.bezier([(21.3, 6.3), (22.2, 7.2), (22.2, 8.3), (21.3, 9.2)]);
            action.line(&[(21.3, 9.2), (10.5, 20.), (5.5, 20.), (2.8, 17.3)]);
            action.bezier([(2.8, 17.3), (2., 16.5), (2.2, 15.3), (3., 14.5)]);
            p.line(&[(7.5, 10.), (14.3, 16.2)]);
        }
        Icon::Tool(
            ToolKind::Line
            | ToolKind::Freehand
            | ToolKind::Arc
            | ToolKind::ThreePointArc
            | ToolKind::CenterArc
            | ToolKind::Pie
            | ToolKind::Rectangle
            | ToolKind::RotatedRectangle
            | ToolKind::Circle
            | ToolKind::Polygon,
        ) => match icon {
            Icon::Tool(ToolKind::Line) => {
                action.line(&[(4., 19.), (20., 5.)]);
                guide.ring(4., 19., 1.5);
                guide.ring(20., 5., 1.5);
            }
            Icon::Tool(ToolKind::Freehand) => {
                let points: Vec<_> = (0..=40)
                    .map(|i| {
                        let t = i as f32 / 40.;
                        (3. + 18. * t, 12. + 5. * (t * std::f32::consts::TAU).sin() - 2. * t)
                    })
                    .collect();
                action.line(&points);
            }
            Icon::Tool(ToolKind::ThreePointArc) => {
                action.arc(12., 17., 8., 180., 360.);
                guide.ring(4., 17., 1.2);
                guide.ring(12., 9., 1.2);
                guide.ring(20., 17., 1.2);
            }
            Icon::Tool(ToolKind::CenterArc) => {
                action.arc(12., 13., 8., 195., 345.);
                guide.line(&[(4.3, 11.), (12., 13.), (19.7, 11.)]);
                guide.ring(12., 13., 1.2);
            }
            Icon::Tool(ToolKind::Pie) => {
                let mut sector = vec![(4., 20.)];
                sector.extend((0..=24).map(|i| {
                    let angle = (-90. + 90. * i as f32 / 24.).to_radians();
                    (4. + 16. * angle.cos(), 20. + 16. * angle.sin())
                }));
                action.polygon(&sector, 65);
                guide.line(&[(4., 4.), (4., 20.), (20., 20.)]);
                guide.ring(4., 20., 1.1);
            }
            Icon::Tool(ToolKind::Arc) => {
                action.arc(12., 17., 8., 180., 360.);
                guide.ring(4., 17., 1.2);
                guide.ring(20., 17., 1.2);
            }
            Icon::Tool(ToolKind::RotatedRectangle) => {
                action.polygon(&[(3., 10.), (15., 3.), (21., 14.), (9., 21.)], 25);
                guide.ring(3., 10., 1.3);
                guide.ring(15., 3., 1.3);
                guide.ring(21., 14., 1.3);
            }
            Icon::Tool(ToolKind::Rectangle) => {
                action.polygon(&[(3., 6.), (21., 6.), (21., 18.), (3., 18.)], 25);
            }
            Icon::Tool(ToolKind::Circle) => {
                action.ring(12., 12., 8.);
                guide.line(&[(12., 12.), (20., 12.)]);
            }
            _ => {
                action.polygon(&[(8., 4.), (16., 4.), (21., 12.), (16., 20.), (8., 20.), (3., 12.)], 25);
            }
        },
        Icon::Tool(ToolKind::PushPull) => {
            p.cube();
            action.polygon(&[(4., 10.), (12., 6.), (20., 10.), (12., 14.)], 55);
            action.arrow((12., 10.), (12., 1.));
        }
        Icon::Tool(ToolKind::Offset) => {
            p.line(&[(3., 4.), (21., 4.), (21., 20.), (3., 20.), (3., 4.)]);
            action.polygon(&[(7., 8.), (17., 8.), (17., 16.), (7., 16.)], 20);
        }
        Icon::Tool(ToolKind::FollowMe) => {
            p.line(&[
                (3., 21.),
                (3., 11.),
                (13., 11.),
                (13., 4.),
                (20., 4.),
                (20., 8.),
                (17., 8.),
                (17., 15.),
                (7., 15.),
                (7., 21.),
            ]);
            p.line(&[(3., 11.), (7., 15.), (7., 21.)]);
            p.line(&[(13., 11.), (17., 15.)]);
            action.polygon(&[(3., 17.), (7., 17.), (7., 21.), (3., 21.)], 60);
            guide.arrow((9., 9.), (19., 2.));
        }
        Icon::Tool(ToolKind::Move) => {
            for tip in [(12., 2.), (22., 12.), (12., 22.), (2., 12.)] {
                guide.arrow((12., 12.), tip);
            }
        }
        Icon::Tool(ToolKind::Rotate) => {
            action.curved_arrow((12., 12.), (9., 9.), 25., 325.);
            p.ring(12., 12., 1.3);
        }
        Icon::Tool(ToolKind::Scale) => {
            p.line(&[(3., 10.), (3., 21.), (14., 21.)]);
            action.polygon(&[(3., 15.), (9., 15.), (9., 21.), (3., 21.)], 25);
            guide.arrow((10., 14.), (21., 3.));
            guide.line(&[(14., 3.), (21., 3.), (21., 10.)]);
        }
        Icon::Tool(ToolKind::TapeMeasure) => {
            p.line(&[(2., 19.), (2., 9.)]);
            p.bezier([(2., 9.), (2., 6.8), (3.8, 5.), (6., 5.)]);
            p.line(&[(6., 5.), (12., 5.)]);
            p.bezier([(12., 5.), (14.2, 5.), (16., 6.8), (16., 9.)]);
            p.line(&[(16., 9.), (16., 19.), (2., 19.)]);
            p.ring(9., 12., 3.2);
            measure.line(&[(16., 15.), (22., 15.), (22., 19.), (16., 19.)]);
            measure.line(&[(19., 15.), (19., 17.)]);
            measure.line(&[(22., 14.), (22., 20.)]);
        }
        Icon::Tool(ToolKind::Protractor) => {
            p.arc(12., 18., 9., 180., 360.);
            p.line(&[(3., 18.), (21., 18.)]);
            for angle in [210_f32, 240., 270., 300., 330.] {
                let a = angle.to_radians();
                p.line(&[(12. + 7. * a.cos(), 18. + 7. * a.sin()), (12. + 9. * a.cos(), 18. + 9. * a.sin())]);
            }
            measure.line(&[(12., 18.), (18., 8.)]);
        }
        Icon::Tool(ToolKind::Axes) => {
            p.colored(Color32::from_rgb(225, 100, 85)).arrow((11., 14.), (22., 20.));
            p.colored(Color32::from_rgb(120, 191, 113)).arrow((11., 14.), (2., 20.));
            guide.arrow((11., 14.), (11., 2.));
        }
        Icon::Tool(ToolKind::PaintBucket) => {
            // A tapered pail with an oval opening, tilted toward the paint drop.
            let tilt = |(x, y): (f32, f32)| {
                let (sin, cos) = 35_f32.to_radians().sin_cos();
                (11. + (x - 11.) * cos - (y - 12.) * sin, 12. + (x - 11.) * sin + (y - 12.) * cos)
            };
            p.line(&[(5., 8.), (6.8, 18.)].map(tilt));
            p.bezier([(6.8, 18.), (7., 19.5), (15., 19.5), (15.2, 18.)].map(tilt));
            p.line(&[(15.2, 18.), (17., 8.)].map(tilt));
            action.bezier([(5., 8.), (5., 5.6), (17., 5.6), (17., 8.)].map(tilt));
            action.bezier([(17., 8.), (17., 10.4), (5., 10.4), (5., 8.)].map(tilt));
            action.bezier([(18.2, 12.2), (19.5, 12.5), (20.5, 13.), (20.5, 14.)]);
            action.bezier([(20.5, 14.), (20., 16.), (18.4, 17.), (18.4, 18.5)]);
            action.bezier([(18.4, 18.5), (18.4, 21.3), (22.6, 21.3), (22.6, 18.5)]);
            action.bezier([(22.6, 18.5), (22.6, 17.), (21., 16.), (20.5, 14.)]);
        }
        Icon::Action(Action::Fillet) => {
            guide.line(&[(3., 3.), (3., 21.), (21., 21.)]);
            action.line(&[(7., 3.), (7., 11.)]);
            action.bezier([(7., 11.), (7., 16.5), (10.5, 20.), (16., 20.)]);
            action.line(&[(16., 20.), (22., 20.)]);
        }
        Icon::Action(Action::MakeComponent) => {
            p.cube();
            guide.line(&[(12., 6.), (20., 10.), (20., 18.), (12., 22.), (4., 18.), (4., 10.), (12., 6.)]);
            action.polygon(&[(12., 14.), (20., 10.), (20., 18.), (12., 22.)], 55);
        }
        Icon::Tool(ToolKind::Orbit) => {
            let points: Vec<_> = (0..=32)
                .map(|i| {
                    let a = i as f32 / 32. * std::f32::consts::TAU;
                    (12. + 4. * a.cos(), 12. + 10. * a.sin())
                })
                .collect();
            p.line(&points);
            guide.curved_arrow((12., 12.), (10., 5.), 205., 510.);
        }
        Icon::Tool(ToolKind::Pan) => {
            let hand = Pen { painter: p.painter, rect: p.rect, stroke: Stroke::new(1.25, guide.stroke.color) };
            hand.bezier([(8., 22.), (6., 20.), (4., 16.), (2.5, 14.)]);
            hand.bezier([(2.5, 14.), (0.8, 11.5), (2.8, 10.2), (4.2, 11.8)]);
            hand.line(&[(4.2, 11.8), (6., 14.), (6., 6.5)]);
            hand.bezier([(6., 6.5), (6., 3.5), (8.4, 3.5), (8.4, 6.5)]);
            hand.line(&[(8.4, 6.5), (8.4, 11.)]);
            hand.bezier([(8.4, 11.), (8.4, 12.), (9.8, 12.), (9.8, 11.)]);
            hand.line(&[(9.8, 11.), (9.8, 3.5)]);
            hand.bezier([(9.8, 3.5), (9.8, 0.5), (12.2, 0.5), (12.2, 3.5)]);
            hand.line(&[(12.2, 3.5), (12.2, 10.)]);
            hand.bezier([(12.2, 10.), (12.2, 11.), (13.6, 11.), (13.6, 10.)]);
            hand.line(&[(13.6, 10.), (13.6, 5.)]);
            hand.bezier([(13.6, 5.), (13.6, 2.), (16., 2.), (16., 5.)]);
            hand.line(&[(16., 5.), (16., 11.5)]);
            hand.bezier([(16., 11.5), (16., 12.5), (17.4, 12.5), (17.4, 11.5)]);
            hand.line(&[(17.4, 11.5), (17.4, 8.)]);
            hand.bezier([(17.4, 8.), (17.4, 5.), (19.8, 5.), (19.8, 8.)]);
            hand.line(&[(19.8, 8.), (19.8, 15.)]);
            hand.bezier([(19.8, 15.), (19.8, 18.), (17., 20.), (17., 22.)]);
            hand.line(&[(17., 22.), (8., 22.)]);
        }
        Icon::Tool(ToolKind::Zoom | ToolKind::ZoomWindow) => {
            if matches!(icon, Icon::Tool(ToolKind::Zoom)) {
                guide.ring(10., 10., 7.);
                guide.line(&[(15., 15.), (22., 22.)]);
                action.line(&[(7., 10.), (13., 10.)]);
                action.line(&[(10., 7.), (10., 13.)]);
            } else {
                guide.line(&[(3., 9.), (3., 3.), (9., 3.)]);
                guide.line(&[(15., 3.), (21., 3.), (21., 9.)]);
                guide.line(&[(3., 15.), (3., 21.), (9., 21.)]);
                guide.line(&[(15., 21.), (21., 21.), (21., 15.)]);
                action.ring(11., 11., 4.);
                action.line(&[(14., 14.), (18., 18.)]);
            }
        }
        Icon::Tool(ToolKind::PositionCamera) => {
            guide.rounded_rect((2., 7.), (22., 20.), 2.);
            guide.line(&[(7., 7.), (9., 4.), (15., 4.), (17., 7.)]);
            action.ring(12., 13.5, 4.);
        }
        Icon::Tool(ToolKind::LookAround) => {
            guide.bezier([(2., 12.), (7., 3.), (17., 3.), (22., 12.)]);
            guide.bezier([(22., 12.), (17., 21.), (7., 21.), (2., 12.)]);
            action.ring(12., 12., 3.);
        }
        Icon::Tool(ToolKind::Walk) => {
            guide.ring(12., 3., 2.);
            guide.line(&[(12., 7.), (12., 13.5)]);
            guide.line(&[(12., 8.), (8., 10.5), (6., 14.)]);
            guide.line(&[(12., 8.), (16., 11.), (19., 11.)]);
            guide.line(&[(12., 13.5), (15., 17.), (17., 22.)]);
            guide.line(&[(12., 13.5), (9., 18.), (5., 21.)]);
        }
        Icon::Tool(ToolKind::Dimension) => {
            p.line(&[(3., 3.), (3., 21.)]);
            p.line(&[(21., 3.), (21., 21.)]);
            measure.arrow((12., 16.), (3., 16.));
            measure.arrow((12., 16.), (21., 16.));
            p.line(&[(9., 8.), (12., 4.), (12., 11.)]);
        }
        Icon::Tool(ToolKind::Text) => {
            action.line(&[(8., 3.), (22., 3.)]);
            action.line(&[(15., 3.), (15., 15.)]);
            guide.line(&[(3., 20.), (8., 15.), (12., 15.)]);
            guide.ring(3., 20., 1.);
        }
        Icon::Action(Action::Text3d) => {
            p.line(&[(3., 8.), (7., 3.), (22., 3.), (18., 8.), (3., 8.)]);
            p.line(&[(18., 8.), (22., 3.), (22., 7.), (18., 12.)]);
            action.polygon(&[(3., 8.), (18., 8.), (18., 12.), (3., 12.)], 50);
            action.polygon(&[(9., 12.), (13., 12.), (13., 22.), (9., 22.)], 50);
            p.line(&[(13., 22.), (17., 17.), (17., 12.)]);
        }
        Icon::Action(Action::SectionPlane) => {
            p.cube();
            action.polygon(&[(1., 12.), (15., 5.), (23., 14.), (9., 21.)], 60);
            guide.line(&[(12., 6.), (12., 22.)]);
        }
        Icon::Action(Action::TerrainGrid) => {
            for x in [4., 12., 20.] {
                p.line(&[(x, 4.), (x, 20.)]);
            }
            for y in [4., 12., 20.] {
                action.line(&[(4., y), (20., y)]);
            }
        }
        Icon::Action(Action::FromContours) => {
            for y in [6., 12., 18.] {
                action.line(&[(2., y + 2.), (7., y - 1.), (14., y + 1.), (22., y - 2.)]);
            }
        }
        Icon::Action(Action::Smoove) => {
            p.line(&[(2., 20.), (7., 18.), (12., 12.), (17., 18.), (22., 20.)]);
            action.arrow((12., 11.), (12., 2.));
        }
        Icon::Action(Action::Drape) => {
            p.line(&[(2., 21.), (8., 16.), (16., 20.), (22., 15.)]);
            action.line(&[(4., 4.), (20., 4.), (20., 8.), (4., 8.), (4., 4.)]);
            guide.arrow((12., 9.), (12., 18.));
        }
        Icon::Action(Action::Stamp) => {
            p.line(&[(2., 21.), (8., 16.), (16., 16.), (22., 21.)]);
            action.polygon(&[(7., 8.), (17., 8.), (17., 13.), (7., 13.)], 30);
            action.line(&[(10., 8.), (10., 3.), (14., 3.), (14., 8.)]);
        }
        Icon::Action(Action::AddDetail) => {
            p.line(&[(3., 20.), (12., 4.), (21., 20.), (3., 20.)]);
            guide.line(&[(3., 20.), (12., 14.), (21., 20.)]);
            guide.line(&[(12., 4.), (12., 14.)]);
            action.line(&[(9., 14.), (15., 14.)]);
            action.line(&[(12., 11.), (12., 17.)]);
        }
        Icon::Action(Action::FlipEdge) => {
            p.line(&[(3., 5.), (21., 5.), (21., 19.), (3., 19.), (3., 5.)]);
            guide.line(&[(3., 19.), (9., 14.)]);
            guide.line(&[(15., 10.), (21., 5.)]);
            action.line(&[(3., 5.), (21., 19.)]);
        }
        Icon::Extents => {
            for (corner, tip) in
                [((3., 3.), (9., 9.)), ((21., 3.), (15., 9.)), ((21., 21.), (15., 15.)), ((3., 21.), (9., 15.))]
            {
                guide.arrow(corner, tip);
            }
        }
        _ => p.cube(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_and_hover_preserve_toolbar_and_tab_positions() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut positions = Vec::new();
        for (selected, pointer) in
            [(false, None), (true, None), (false, Some(egui::pos2(18., 18.))), (true, Some(egui::pos2(110., 60.)))]
        {
            let mut rects = Vec::new();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(600., 200.))),
                    events: pointer.map(|p| vec![egui::Event::PointerMoved(p)]).unwrap_or_default(),
                    ..Default::default()
                },
                |ui| {
                    ui.horizontal(|ui| {
                        rects.push(button(ui, Icon::Tool(ToolKind::Select), selected, 22.).rect);
                        rects.push(button(ui, Icon::Tool(ToolKind::Move), !selected, 22.).rect);
                        rects.push(button(ui, Icon::Extents, false, 22.).rect);
                    });
                    ui.horizontal(|ui| {
                        for (i, label) in ["Model", "Properties", "Appearance"].iter().enumerate() {
                            rects.push(tab(ui, label, if selected { i == 1 } else { i == 0 }).rect);
                        }
                    });
                },
            );
            output.textures_delta.clear();
            positions.push(rects);
        }
        for actual in &positions[1..] {
            assert_eq!(actual, &positions[0]);
        }
    }
}
