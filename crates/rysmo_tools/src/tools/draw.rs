//! Drawing tools: Line, Rectangle, Circle and the 2-point Arc.

use rysmo_infer::{Inference, Kind};
use rysmo_math::{DVec3, EPS, Plane};
use rysmo_mesh::{Element, Mesh};
use rysmo_model::{Command, Editor, command::arc_points};

use super::{BLACK, Ctx, Last, MAGENTA, Preview, PreviewLine, State, ToolBox, ToolKind, line_color};

/// How close, in pixels, the arc's bulge snaps to a half circle.
const HALF_CIRCLE_PX: f64 = 8.0;

impl ToolBox {
    pub(crate) fn draw_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        if self.active() == ToolKind::RotatedRectangle {
            self.rotated_click(editor, ctx, inf);
            return;
        }
        let p = inf.point;
        match (self.active(), self.state.clone()) {
            (ToolKind::Axes, State::Idle) => self.state = State::AxesRed { origin: p },
            (ToolKind::Axes, State::AxesRed { origin }) if p.distance(origin) > EPS => {
                self.state = State::AxesGreen { origin, red: (p - origin).normalize() };
            }
            (ToolKind::Axes, State::AxesGreen { origin, red }) => {
                if self.run(editor, Command::SetAxes { origin, x: red, y: p - origin }).is_some() {
                    self.axes = editor.active().axes;
                    self.reset();
                    self.last = Last::None;
                }
            }
            (ToolKind::Freehand, State::Idle) => {
                let plane = drawing_plane(&inf, editor.mesh(), self.axes);
                self.state = State::Freehand { points: vec![plane.project(p)], plane };
                self.last = Last::None;
            }
            (ToolKind::Line, State::Idle) => self.state = State::Line { start: p },
            (ToolKind::Line, State::Line { start }) => self.finish_line(editor, ctx, start, p),
            (ToolKind::Rectangle, State::Idle) => {
                let plane = self.locked_plane(p).unwrap_or_else(|| drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::Rectangle { corner: plane.project(p), plane };
            }
            (ToolKind::Rectangle, State::Rectangle { corner, plane }) => {
                let (u, v) = self.axes.rect_axes(&plane);
                let d = p - corner;
                self.finish_rectangle(editor, ctx, corner, plane, u * d.dot(u), v * d.dot(v));
            }
            (ToolKind::Circle | ToolKind::Polygon, State::Idle) => {
                let plane = self.locked_plane(p).unwrap_or_else(|| drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::Circle { centre: plane.project(p), plane };
            }
            (ToolKind::Circle | ToolKind::Polygon, State::Circle { centre, plane }) => {
                let r = p.distance(centre);
                self.finish_circle(editor, centre, plane, r, p - centre);
            }
            (ToolKind::Arc, State::Idle) => {
                let plane = self.locked_plane(p).unwrap_or_else(|| drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::ArcChord { start: plane.project(p), plane };
            }
            (ToolKind::Arc, State::ArcChord { start, plane }) => {
                let end = plane.project(p);
                if end.distance(start) > EPS {
                    self.state = State::ArcBulge { start, end, plane, bulge: 0.0 };
                }
            }
            (ToolKind::Arc, State::ArcBulge { start, end, plane, bulge }) if bulge.abs() > EPS => {
                self.finish_arc(editor, start, end, plane, bulge);
            }
            _ => {}
        }
    }

    pub(crate) fn snap_rectangle(&self, ctx: &Ctx, corner: DVec3, plane: Plane, point: DVec3) -> DVec3 {
        let (u, v) = self.axes.rect_axes(&plane);
        let delta = point - corner;
        let x = delta.dot(u);
        let y = delta.dot(v);
        if x.abs() < EPS || y.abs() < EPS {
            return point;
        }
        let phi = (1.0 + 5.0_f64.sqrt()) * 0.5;
        [1.0, phi, 1.0 / phi]
            .into_iter()
            .filter_map(|ratio| {
                if (x.abs() / y.abs() - ratio).abs() / ratio > 0.03 {
                    return None;
                }
                let extent = (ratio * x.abs() + y.abs()) / (ratio * ratio + 1.0);
                let candidate = corner + u * (x.signum() * extent * ratio) + v * (y.signum() * extent);
                let distance = ctx.view.project(point)?.distance(ctx.view.project(candidate)?);
                (distance <= 8.0 * ctx.scale).then_some((distance, candidate))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, point)| point)
            .unwrap_or(point)
    }

    pub(crate) fn freehand_move(&mut self, ctx: &Ctx) {
        let Some(inf) = self.inference else { return };
        if let State::Freehand { points, plane } = &mut self.state {
            let point = plane.project(inf.point);
            let Some(last) = points.last().copied() else { return };
            if let (Some(a), Some(b)) = (ctx.view.project(last), ctx.view.project(point))
                && a.distance(b) >= 3.0 * ctx.scale
            {
                points.push(point);
            }
        }
    }

    pub(crate) fn finish_freehand(&mut self, editor: &mut Editor, ctx: &Ctx) {
        let State::Freehand { mut points, plane } = self.state.clone() else { return };
        if let Some(inf) = self.inference {
            let mut point = plane.project(inf.point);
            if points.len() >= 3
                && let (Some(a), Some(b)) = (ctx.view.project(points[0]), ctx.view.project(point))
                && a.distance(b) < 8.0 * ctx.scale
            {
                point = points[0];
            }
            if points.last().is_some_and(|last| last.distance(point) > EPS) {
                points.push(point);
            } else if points.len() >= 3 && point == points[0] {
                *points.last_mut().unwrap() = point;
            }
        }
        let lines: Vec<_> = points.windows(2).map(|p| [p[0], p[1]]).collect();
        if !lines.is_empty() {
            self.run(editor, Command::DrawFreehand { points, normal: plane.normal });
        }
        self.reset();
        self.last = Last::None;
    }

    /// Follows the cursor with the arc's bulge, snapping to a half circle.
    pub(crate) fn arc_move(&mut self, editor: &Editor, ctx: &Ctx) {
        let State::ArcBulge { start, end, plane, bulge: previous } = self.state.clone() else { return };
        let ray = ctx.view.ray(ctx.cursor);
        let Some(t) = plane.intersect_ray(&ray).or_else(|| plane.flipped().intersect_ray(&ray)) else { return };
        let q = ray.at(t);
        let chord = end - start;
        let left = plane.normal.cross(chord.normalize());
        let mut bulge = (q - (start + end) * 0.5).dot(left);
        let half = chord.length() * 0.5 * bulge.signum();
        let px = ctx.view.pixel_size_at((start + end) * 0.5);
        let tangents = tangent_bulges(editor.mesh(), start, end, plane);
        let locked =
            ctx.modifiers.shift.then(|| tangents.iter().copied().find(|t| (previous - t).abs() < 1e-9)).flatten();
        if let Some(tangent) = locked.or_else(|| {
            tangents
                .into_iter()
                .filter(|t| (bulge - t).abs() < HALF_CIRCLE_PX * px * ctx.scale)
                .min_by(|a, b| (bulge - a).abs().total_cmp(&(bulge - b).abs()))
        }) {
            bulge = tangent;
        } else if (bulge - half).abs() < HALF_CIRCLE_PX * px * ctx.scale {
            bulge = half;
        } else {
            bulge = editor.doc().measurement.snap_length(bulge);
        }
        if let State::ArcBulge { bulge: b, .. } = &mut self.state {
            *b = bulge;
        }
    }

    pub(crate) fn arc_radius(
        &mut self,
        editor: &mut Editor,
        start: DVec3,
        end: DVec3,
        plane: Plane,
        r: f64,
    ) -> Result<(), String> {
        let half = start.distance(end) * 0.5;
        if r < half - EPS {
            return Err("The radius must be at least half the chord.".into());
        }
        let current = if let State::ArcBulge { bulge, .. } = self.state { bulge } else { 0.0 };
        // The shallow arc on the side the cursor is on.
        let b = (r - (r * r - half * half).max(0.0).sqrt()) * if current < 0.0 { -1.0 } else { 1.0 };
        self.finish_arc(editor, start, end, plane, b);
        self.update_status();
        Ok(())
    }

    /// The plane picked with an arrow key before the first click, through `p`.
    fn locked_plane(&self, p: DVec3) -> Option<Plane> {
        self.plane_lock.map(|axis| Plane::new(self.axes.dir(axis), p))
    }

    pub(crate) fn finish_arc(&mut self, editor: &mut Editor, start: DVec3, end: DVec3, plane: Plane, bulge: f64) {
        let segments = self.arc_segments;
        let out = self.run(editor, Command::DrawArc { start, end, normal: plane.normal, bulge, segments });
        self.reset();
        self.last = match out {
            Some(out) if out.changed => {
                Last::Arc { start, end, normal: plane.normal, bulge, segments, revision: editor.revision() }
            }
            _ => Last::None,
        };
    }

    pub(crate) fn finish_line(&mut self, editor: &mut Editor, ctx: &Ctx, start: DVec3, end: DVec3) {
        if start.distance(end) < EPS {
            return;
        }
        let out = self.run(editor, Command::DrawLines { lines: vec![[start, end]], facing: Some(Self::facing(ctx)) });
        let closed = out.as_ref().is_some_and(|o| o.created.iter().any(|e| matches!(e, Element::Face(_))));
        self.reset();
        // Continue drawing from the end point until a line closes a face.
        if !closed && out.is_some() {
            self.state = State::Line { start: end };
        }
        self.last = Last::None;
    }

    pub(crate) fn finish_rectangle(
        &mut self,
        editor: &mut Editor,
        ctx: &Ctx,
        corner: DVec3,
        plane: Plane,
        u: DVec3,
        v: DVec3,
    ) {
        if u.length() < EPS || v.length() < EPS {
            self.message = Some("The rectangle needs a width and a depth.".into());
            return;
        }
        let facing = if rysmo_math::parallel(plane.normal, DVec3::Z) { None } else { Some(Self::facing(ctx)) };
        let out = self.run(editor, Command::DrawRectangle { corner, u, v, facing });
        self.reset();
        // Typing new dimensions right away corrects the rectangle, as for a rotated one.
        self.last = match out {
            Some(out) if out.changed => Last::RotatedRectangle { corner, u, v, facing, revision: editor.revision() },
            _ => Last::None,
        };
    }

    pub(crate) fn radial_command(
        &self,
        centre: DVec3,
        plane: Plane,
        radius: f64,
        towards: DVec3,
        segments: u32,
    ) -> Command {
        if self.active() == ToolKind::Polygon {
            Command::DrawPolygon { centre, normal: plane.normal, radius, segments, start: Some(towards) }
        } else {
            Command::DrawCircle { centre, normal: plane.normal, radius, segments, start: Some(towards) }
        }
    }

    pub(crate) fn finish_circle(
        &mut self,
        editor: &mut Editor,
        centre: DVec3,
        plane: Plane,
        radius: f64,
        towards: DVec3,
    ) {
        if radius < EPS {
            return;
        }
        let segments = self.radial_segments();
        if self.run(editor, self.radial_command(centre, plane, radius, towards, segments)).is_some() {
            self.reset();
            self.last = Last::Radial { centre, plane, radius, towards, segments, revision: editor.revision() };
        }
    }

    /// Lines for the drawing tools' states; the editing tools add theirs in edit.rs.
    pub(crate) fn preview_state(&self, editor: &Editor, pv: &mut Preview) {
        let push = |pv: &mut Preview, a: DVec3, b: DVec3, color: [f32; 3], dashed: bool| {
            pv.lines.push(PreviewLine { a, b, color, dashed, thick: true });
        };
        match &self.state {
            State::AxesRed { origin } => {
                if let Some(inf) = self.inference {
                    push(pv, *origin, inf.point, [0.9, 0.0, 0.0], false);
                }
            }
            State::AxesGreen { origin, red } => {
                let length = self.inference.map(|i| i.point.distance(*origin)).unwrap_or(1.0).max(self.px * 40.0);
                push(pv, *origin, *origin + *red * length, [0.9, 0.0, 0.0], false);
                if let Some(inf) = self.inference {
                    let green = inf.point - *origin;
                    let green = green - *red * green.dot(*red);
                    if green.length() > EPS {
                        let green = green.normalize();
                        push(pv, *origin, *origin + green * length, [0.0, 0.7, 0.0], false);
                        push(pv, *origin, *origin + red.cross(green) * length, [0.0, 0.0, 0.9], false);
                    }
                }
            }
            State::Freehand { points, .. } => {
                for pair in points.windows(2) {
                    push(pv, pair[0], pair[1], BLACK, false);
                }
                if let (Some(last), Some(inf)) = (points.last(), self.inference) {
                    push(pv, *last, inf.point, BLACK, false);
                }
            }
            State::AngleBase { centre, .. } => {
                if let Some(i) = self.inference {
                    push(pv, *centre, i.point, BLACK, true);
                }
            }
            State::AngleSweep { centre, plane, from, angle } => {
                let length = self.inference.map(|i| i.point.distance(*centre)).unwrap_or(1.0).max(self.px * 40.0);
                let dir = rysmo_math::DQuat::from_axis_angle(plane.normal, angle.to_radians()) * *from;
                push(pv, *centre, *centre + *from * length, BLACK, true);
                push(pv, *centre, *centre + dir * length, MAGENTA, true);
                let radius = self.px * 45.;
                let side = plane.normal.cross(*from);
                for i in 0..36 {
                    let point = |k: f64| {
                        let a = k / 36. * std::f64::consts::TAU;
                        *centre + (*from * a.cos() + side * a.sin()) * radius
                    };
                    push(pv, point(i as f64), point((i + 1) as f64), BLACK, false);
                }
                super::protractor::append_ticks(pv, *centre, plane.normal, *from, radius, editor.doc().measurement);
                pv.label = Some(editor.doc().measurement.format_angle(*angle));
            }
            State::Line { start } => {
                if let Some(i) = &self.inference {
                    push(pv, *start, i.point, line_color(Some(i)), false);
                }
            }
            State::RotatedEdge { corner, .. } => {
                if let Some(i) = self.inference {
                    push(pv, *corner, i.point, line_color(Some(&i)), false);
                }
            }
            State::RotatedWidth { corner, u, baseline, .. } => {
                if let Some(i) = self.inference {
                    let v = super::rotated_rectangle::width_vector(*corner, *u, i.point);
                    let pts = [*corner, *corner + *u, *corner + *u + v, *corner + v];
                    for k in 0..4 {
                        push(pv, pts[k], pts[(k + 1) % 4], BLACK, false);
                    }
                    push(pv, *corner, *corner + v, MAGENTA, true);
                    let side = u.normalize().cross(*baseline);
                    let angle = v.dot(side).atan2(v.dot(*baseline)).to_degrees();
                    pv.label = Some(editor.doc().measurement.format_angle(angle));
                    let radius = self.px * 28.;
                    for k in 0..36 {
                        let point = |k: f64| {
                            let t = k / 36. * std::f64::consts::TAU;
                            *corner + (*baseline * t.cos() + side * t.sin()) * radius
                        };
                        push(pv, point(k as f64), point((k + 1) as f64), MAGENTA, true);
                    }
                    super::protractor::append_ticks(
                        pv,
                        *corner,
                        u.normalize(),
                        *baseline,
                        radius,
                        editor.doc().measurement,
                    );
                }
            }
            State::Rectangle { corner, plane } => {
                if let Some(i) = &self.inference {
                    let (u, v) = self.axes.rect_axes(plane);
                    let d = i.point - *corner;
                    let pts = [
                        *corner,
                        *corner + u * d.dot(u),
                        *corner + u * d.dot(u) + v * d.dot(v),
                        *corner + v * d.dot(v),
                    ];
                    let label = rectangle_proportion(d.dot(u).abs(), d.dot(v).abs());
                    let color = if label.is_some() { MAGENTA } else { BLACK };
                    for k in 0..4 {
                        push(pv, pts[k], pts[(k + 1) % 4], color, false);
                    }
                    if label == Some("Square") {
                        push(pv, pts[0], pts[2], MAGENTA, true);
                    }
                    if let Some(label) = label {
                        pv.label = Some(label.into());
                    }
                }
            }
            State::CentreRadius { centre, plane, .. } | State::Circle { centre, plane } => {
                if let Some(i) = &self.inference {
                    let r = i.point.distance(*centre);
                    let (u, v) = start_axes(plane, i.point - *centre);
                    let n = self.radial_segments() as usize;
                    let pts: Vec<DVec3> = (0..n)
                        .map(|k| {
                            let a = k as f64 / n as f64 * std::f64::consts::TAU;
                            *centre + (u * a.cos() + v * a.sin()) * r
                        })
                        .collect();
                    for k in 0..n {
                        push(pv, pts[k], pts[(k + 1) % n], BLACK, false);
                    }
                    push(pv, *centre, i.point, BLACK, true);
                }
            }
            State::ArcChord { start, .. } => {
                if let Some(i) = &self.inference {
                    push(pv, *start, i.point, line_color(Some(i)), true);
                }
            }
            State::ArcBulge { start, end, plane, bulge } => {
                match arc_points(*start, *end, plane.normal, *bulge, self.arc_segments) {
                    Some(pts) => {
                        let semicircle = (bulge.abs() - start.distance(*end) * 0.5).abs() < 1e-9;
                        let tangent = tangent_bulges(editor.mesh(), *start, *end, *plane)
                            .iter()
                            .any(|target| (target - bulge).abs() < 1e-9);
                        let color = if tangent {
                            [0., 0.8, 0.85]
                        } else if semicircle {
                            MAGENTA
                        } else {
                            BLACK
                        };
                        for w in pts.windows(2) {
                            push(pv, w[0], w[1], color, false);
                        }
                        if tangent {
                            pv.label = Some("Tangent".into());
                        } else if semicircle {
                            pv.label = Some("Half Circle".into());
                        }
                    }
                    None => push(pv, *start, *end, BLACK, true),
                }
            }
            _ => self.preview_edit(editor, pv),
        }
    }
}

/// Tangency at either end, including straight wire edges and analytic arcs.
fn tangent_bulges(mesh: &Mesh, start: DVec3, end: DVec3, plane: Plane) -> Vec<f64> {
    [tangent_bulge(mesh, start, end, plane), tangent_bulge(mesh, end, start, plane).map(|b| -b)]
        .into_iter()
        .flatten()
        .collect()
}

fn tangent_bulge(mesh: &Mesh, start: DVec3, end: DVec3, plane: Plane) -> Option<f64> {
    let vertex = mesh.find_vertex(start)?;
    let incident = mesh.edges_of_vertex(vertex);
    if incident.len() != 1 {
        return None;
    }
    let edge = mesh.edge(incident[0])?;
    if !edge.visible() || edge.procedural {
        return None;
    }
    let neighbour = mesh.pos(if edge.v[0] == vertex { edge.v[1] } else { edge.v[0] });
    let tangent = if let Some(curve) = edge.curve {
        let curve = mesh.curve(curve)?;
        if curve.kind != rysmo_mesh::CurveKind::Arc || curve.normal.cross(plane.normal).length() > EPS {
            return None;
        }
        let mut tangent = curve.normal.cross(start - curve.centre).try_normalize()?;
        if tangent.dot(neighbour - start) > 0. {
            tangent = -tangent;
        }
        tangent
    } else {
        let tangent = (start - neighbour).try_normalize()?;
        if tangent.dot(plane.normal).abs() > EPS {
            return None;
        }
        tangent
    };
    let perpendicular = plane.normal.cross(tangent);
    let chord = end - start;
    let denominator = 2. * chord.dot(perpendicular);
    if denominator.abs() < EPS {
        return None;
    }
    let offset = chord.length_squared() / denominator;
    let centre = start + perpendicular * offset;
    let left = plane.normal.cross(chord.try_normalize()?);
    let along = (centre - (start + end) * 0.5).dot(left);
    [along - offset.abs(), along + offset.abs()].into_iter().find(|bulge| {
        bulge.abs() > EPS
            && arc_points(start, end, plane.normal, *bulge, 64)
                .is_some_and(|points| (points[1] - start).dot(tangent) > 0.)
    })
}

/// The plane a rectangle, circle or arc is drawn in: the face under the first click
/// (also a face of a group), or the horizontal plane through the point.
pub(crate) fn drawing_plane(inf: &Inference, mesh: &Mesh, axes: rysmo_math::DrawingAxes) -> Plane {
    if let Some(pl) = inf.plane
        && pl.contains(inf.point)
        && !matches!(inf.kind, Kind::Free)
    {
        return pl;
    }
    if let Some(pl) = inf.face.and_then(|f| mesh.face_plane(f))
        && pl.contains(inf.point)
    {
        return pl;
    }
    Plane::new(axes.z(), inf.point)
}

/// Rectangle sides in a plane, lined up with the model axes where possible.
pub(crate) fn rect_axes(plane: &Plane) -> (DVec3, DVec3) {
    let n = plane.normal;
    let axis = [DVec3::X, DVec3::Y, DVec3::Z]
        .into_iter()
        .min_by(|a, b| a.dot(n).abs().total_cmp(&b.dot(n).abs()))
        .unwrap_or(DVec3::X);
    let u = (axis - n * axis.dot(n)).normalize();
    let v = n.cross(u);
    (u, v)
}

pub(crate) fn start_axes(plane: &Plane, towards: DVec3) -> (DVec3, DVec3) {
    let n = plane.normal;
    let t = towards - n * towards.dot(n);
    let u = if t.length() > EPS { t.normalize() } else { rect_axes(plane).0 };
    (u, n.cross(u))
}

fn rectangle_proportion(width: f64, height: f64) -> Option<&'static str> {
    if width < EPS || height < EPS {
        return None;
    }
    let ratio = width / height;
    if (ratio - 1.0).abs() < 1e-7 {
        Some("Square")
    } else {
        let phi = (1.0 + 5.0_f64.sqrt()) * 0.5;
        ((ratio - phi).abs() < 1e-7 || (ratio - 1.0 / phi).abs() < 1e-7).then_some("Golden Section")
    }
}
