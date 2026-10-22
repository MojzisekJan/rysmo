//! Editing tools: Push/Pull, Move, Rotate, Scale, Offset, Follow Me, Tape Measure,
//! Paint Bucket and the Eraser's endings.

use std::collections::BTreeSet;

use rysmo_infer::{Inference, Kind};
use rysmo_math::{DMat4, DQuat, DVec3, EPS, Plane, distance_point_segment_2d};
use rysmo_mesh::{EdgeId, Element, FaceId, offset_polyline};
use rysmo_model::{Command, Editor, parse_length};

use super::{
    BLACK, Ctx, GRIP, Grip, Last, MAGENTA, OffsetTarget, Preview, PreviewLine, State, ToolBox, ToolKind,
    element_bounds_in_axes, element_lines, hovered, line_color,
};
use crate::vcb::{VcbInput, parse_vcb};

/// Angles within this tolerance snap to the configured increment in the Protractor tool.
const ANGLE_SNAP_DEG: f64 = 2.5;
/// Protractor radius in pixels.
const PROTRACTOR_PX: f64 = 45.0;

struct ScaleCompletion {
    elements: Vec<Element>,
    origin: DVec3,
    grip: Grip,
    factors: DVec3,
    basis: DMat4,
    extent: DVec3,
    uniform: bool,
}

pub(crate) fn format_factors(f: DVec3, g: Grip) -> String {
    let axes: Vec<f64> = (0..3).filter(|i| g.0[*i] != 0).map(|i| f[i]).collect();
    let all_same = axes.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9);
    let fmt = |x: f64| {
        let s = format!("{x:.3}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if all_same {
        axes.first().map(|x| fmt(*x)).unwrap_or_default()
    } else {
        axes.iter().map(|x| fmt(*x)).collect::<Vec<_>>().join(", ")
    }
}

fn rotate_about(centre: DVec3, axis: DVec3, degrees: f64) -> DMat4 {
    DMat4::from_translation(centre)
        * DMat4::from_quat(DQuat::from_axis_angle(axis.normalize(), degrees.to_radians()))
        * DMat4::from_translation(-centre)
}

/// The 26 grips of a box: corners, edge middles and face middles.
pub(crate) fn grips(lo: DVec3, hi: DVec3) -> Vec<(Grip, DVec3)> {
    let c = (lo + hi) * 0.5;
    let h = (hi - lo) * 0.5;
    let mut out = Vec::new();
    for x in -1i8..=1 {
        for y in -1i8..=1 {
            for z in -1i8..=1 {
                if (x, y, z) == (0, 0, 0) {
                    continue;
                }
                let g = DVec3::new(x as f64, y as f64, z as f64);
                // A flat box has no grips across its flat direction.
                if (0..3).any(|i| g[i] != 0.0 && h[i] < EPS) {
                    continue;
                }
                out.push((Grip([x, y, z]), c + h * g));
            }
        }
    }
    out
}

/// Screen-space handles remain pickable through model geometry. In a projected
/// overlap, the closest handle to the camera wins instead of enumeration order.
fn pick_handle<T: Copy>(ctx: &Ctx, handles: impl IntoIterator<Item = (T, DVec3)>) -> Option<(T, DVec3)> {
    handles
        .into_iter()
        .filter_map(|(id, point)| {
            let distance = ctx.view.project(point)?.distance(ctx.cursor);
            (distance <= 12.0 * ctx.scale).then_some((id, point, distance))
        })
        .min_by(|a, b| {
            a.2.total_cmp(&b.2)
                .then_with(|| a.1.distance_squared(ctx.view.eye).total_cmp(&b.1.distance_squared(ctx.view.eye)))
        })
        .map(|(id, point, _)| (id, point))
}

fn rotation_handles(editor: &Editor, basis: DMat4) -> Vec<(DVec3, DVec3)> {
    let elements = editor.selected();
    if elements.is_empty() || !elements.iter().all(|e| matches!(e, Element::Instance(_))) {
        return Vec::new();
    }
    let Some((lo, hi)) = element_bounds_in_axes(editor, &elements, basis, None) else { return Vec::new() };
    let centre = (lo + hi) * 0.5;
    let mut handles = Vec::new();
    for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
        let extent = (hi - lo).dot(axis) * 0.5;
        if extent <= EPS {
            continue;
        }
        for sign in [-1.0, 1.0] {
            handles.push((basis.transform_vector3(axis), basis.transform_point3(centre + axis * extent * sign)));
        }
    }
    handles
}

impl ToolBox {
    // ----- Push/Pull -----

    pub(crate) fn push_pull_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference, clicks: u8) {
        let p = inf.point;
        match self.state.clone() {
            State::Idle => {
                let face = editor.mesh().pick_face(&ctx.view.ray(ctx.cursor)).map(|(f, _)| f);
                if clicks >= 2 {
                    if let (Some(f), Some(d)) = (face, self.last_distance) {
                        self.finish_push_pull(editor, ctx, f, d);
                    }
                    return;
                }
                if let Some(f) = face {
                    let normal = editor.mesh().face_normal(f).unwrap_or(DVec3::Z);
                    let start = editor.mesh().face_plane(f).map(|pl| pl.project(p)).unwrap_or(p);
                    self.state = State::PushPull { face: f, start, normal, distance: 0.0 };
                }
            }
            State::PushPull { face, distance, .. } => {
                if distance.abs() < EPS {
                    // A click without moving: cancel, so a double click can repeat.
                    self.reset();
                    if clicks >= 2
                        && let Some(d) = self.last_distance
                    {
                        self.finish_push_pull(editor, ctx, face, d);
                    }
                } else {
                    self.finish_push_pull(editor, ctx, face, distance);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn finish_push_pull(&mut self, editor: &mut Editor, ctx: &Ctx, face: FaceId, distance: f64) {
        let keep_base = ctx.modifiers.ctrl;
        // A face standing alone builds a solid whichever way it is pulled, so its
        // distance counts as positive for repeating on other faces.
        let mesh = editor.mesh();
        let free = mesh.face(face).is_some() && mesh.face_edges(face).iter().all(|e| mesh.faces_of_edge(*e).len() == 1);
        if self.run(editor, Command::PushPull { face, distance, keep_base }).is_some() {
            self.last = Last::PushPull { face, distance, keep_base, free };
            self.last_distance = Some(if free { distance.abs() } else { distance });
        }
        self.reset();
    }

    // ----- Move -----

    /// The selection, or the hovered element when nothing is selected (which also
    /// becomes the selection).
    fn targets(&mut self, editor: &mut Editor, inf: &Inference) -> Vec<Element> {
        let mut elements = editor.selected();
        if elements.is_empty()
            && let Some(h) = self
                .override_pick
                .and_then(|p| if let rysmo_infer::Pick::Element(e) = p { Some(e) } else { None })
                .or_else(|| hovered(inf))
        {
            editor.set_selection([h]);
            elements = vec![h];
        }
        elements
    }

    pub(crate) fn move_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        match self.state.clone() {
            State::Idle => {
                let elements = self.targets(editor, &inf);
                if !elements.is_empty() {
                    self.glue_plane = if let [Element::Instance(id)] = elements.as_slice() {
                        editor
                            .active()
                            .instances
                            .get(id)
                            .and_then(|i| i.glued_to)
                            .and_then(|face| editor.mesh().face_plane(face))
                            .map(|plane| Plane::new(plane.normal, inf.point))
                    } else {
                        None
                    };
                    self.state = State::Move { elements, start: inf.point, offset: DVec3::ZERO };
                }
            }
            State::Move { elements, start, .. } => {
                let offset = inf.point - start;
                if offset.length() >= EPS {
                    self.finish_move(editor, ctx, elements, offset);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn finish_move(&mut self, editor: &mut Editor, ctx: &Ctx, elements: Vec<Element>, offset: DVec3) {
        if ctx.modifiers.ctrl {
            let pivot = if let State::Move { start, .. } = self.state { start } else { DVec3::ZERO };
            let text_view = elements.iter().any(|e| matches!(e, Element::Text(_))).then_some((*ctx.view, pivot));
            let command = if text_view.is_some() {
                Command::CopyWithText { elements: elements.clone(), offset, copies: 1, pivot, view: *ctx.view }
            } else {
                Command::Copy { elements: elements.clone(), offset, copies: 1 }
            };
            if let Some(out) = self.run(editor, command) {
                self.select_result(editor, out.created);
                self.last = Last::Copy { elements, offset, copies: 1, divide: false, text_view };
            }
        } else {
            let pivot = if let State::Move { start, .. } = self.state { start } else { DVec3::ZERO };
            let text_view = elements.iter().any(|e| matches!(e, Element::Text(_))).then_some((*ctx.view, pivot));
            let command = if text_view.is_some() {
                Command::MoveWithText { elements: elements.clone(), offset, pivot, view: *ctx.view }
            } else {
                Command::Move { elements: elements.clone(), offset }
            };
            if let Some(out) = self.run(editor, command) {
                self.select_result(editor, out.created);
                self.last = Last::Move { elements, offset, text_view };
            }
        }
        self.reset();
    }

    // ----- Rotate -----

    pub(crate) fn rotate_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        match self.state.clone() {
            State::Idle => {
                let elements = self.targets(editor, &inf);
                if elements.is_empty() {
                    self.message = Some("Select something to rotate first.".into());
                    return;
                }
                if let Some((normal, centre)) = pick_handle(ctx, rotation_handles(editor, self.axes.basis())) {
                    let axis = self.axis_lock_dir().unwrap_or(normal);
                    self.state = State::RotatePivot { elements, centre, axis };
                    return;
                }
                // The protractor lies on the face under the cursor, or in the plane an
                // arrow key picked, or flat on the ground.
                let axis = match (self.axis_lock_dir(), inf.plane) {
                    (Some(a), _) => a,
                    (None, Some(pl)) if !matches!(inf.kind, Kind::Ground | Kind::Free) => pl.normal,
                    _ => self.axes.z(),
                };
                self.state = State::RotatePivot { elements, centre: inf.point, axis };
            }
            State::RotatePivot { elements, centre, axis } => {
                let from = project_to_plane(inf.point - centre, axis);
                if from.length() > EPS {
                    self.state = State::Rotating { elements, centre, axis, from: from.normalize(), angle: 0.0 };
                }
            }
            State::Rotating { elements, centre, axis, angle, .. } if angle.abs() > 1e-9 => {
                self.finish_rotate(editor, ctx, elements, centre, axis, angle);
            }
            _ => {}
        }
    }

    fn axis_lock_dir(&self) -> Option<DVec3> {
        self.axis_lock.map(|a| self.axes.dir(a))
    }

    pub(crate) fn rotate_move(&mut self, editor: &Editor) {
        let Some(inf) = self.inference else { return };
        let State::Rotating { centre, axis, from, .. } = self.state.clone() else { return };
        let to = project_to_plane(inf.point - centre, axis);
        if to.length() < EPS {
            return;
        }
        let to = to.normalize();
        let mut angle = from.cross(to).dot(axis.normalize()).atan2(from.dot(to)).to_degrees();
        if !matches!(
            inf.kind,
            Kind::Endpoint
                | Kind::Midpoint
                | Kind::Centre
                | Kind::Intersection
                | Kind::OnEdge
                | Kind::GuidePoint
                | Kind::OnGuide
        ) {
            angle = editor.doc().measurement.snap_angle(angle, ANGLE_SNAP_DEG);
        }
        if let State::Rotating { angle: a, .. } = &mut self.state {
            *a = angle;
        }
    }

    fn finish_rotate(
        &mut self,
        editor: &mut Editor,
        ctx: &Ctx,
        elements: Vec<Element>,
        centre: DVec3,
        axis: DVec3,
        angle: f64,
    ) {
        let copies = if ctx.modifiers.ctrl { 1 } else { 0 };
        if let Some(out) = self.run(editor, Command::Rotate { elements: elements.clone(), centre, axis, angle, copies })
        {
            if copies > 0 {
                self.select_result(editor, out.created);
            }
            self.last = Last::Rotate { elements, centre, axis, angle, copies, divide: false };
        }
        self.reset();
    }

    pub(crate) fn rotate_vcb(&mut self, editor: &mut Editor, ctx: &Ctx, text: &str) -> Result<(), String> {
        let t = text.trim().trim_end_matches('°');
        if let State::Rotating { elements, centre, axis, .. } = self.state.clone() {
            let angle: f64 = t.replace(',', ".").parse().map_err(|_| format!("\"{text}\" is not an angle"))?;
            self.finish_rotate(editor, ctx, elements, centre, axis, angle);
            self.update_status();
            return Ok(());
        }
        if self.state != State::Idle {
            return Err("Pick the centre and the start of the angle first.".into());
        }
        // After a rotation: a new angle, or an array for a copy.
        if let Ok(angle) = t.replace(',', ".").parse::<f64>()
            && let Last::Rotate { elements, centre, axis, copies, .. } = self.last.clone()
        {
            let out =
                self.correct_last(editor, Command::Rotate { elements: elements.clone(), centre, axis, angle, copies })?;
            if copies > 0 {
                self.select_result(editor, out.created);
            }
            self.last = Last::Rotate { elements, centre, axis, angle, copies, divide: false };
            return Ok(());
        }
        let input = parse_vcb(text, editor.doc().units, 1)?;
        self.amend_rotate(editor, input)
    }

    fn amend_rotate(&mut self, editor: &mut Editor, input: VcbInput) -> Result<(), String> {
        let Last::Rotate { elements, centre, axis, angle, copies, .. } = self.last.clone() else {
            return Err("There is nothing to apply this value to.".into());
        };
        if copies == 0 {
            return Err("Copy with Ctrl first, then type 5x or /5.".into());
        }
        let (step, n, divide) = match input {
            VcbInput::Copies(n) => (angle, n, false),
            VcbInput::Divide(n) => (angle / n as f64, n, true),
            _ => return Err("Type an angle, 5x or /5.".into()),
        };
        let out = self.correct_last(
            editor,
            Command::Rotate { elements: elements.clone(), centre, axis, angle: step, copies: n },
        )?;
        self.select_result(editor, out.created);
        self.last = Last::Rotate { elements, centre, axis, angle, copies: n, divide };
        Ok(())
    }

    // ----- Scale -----

    pub(crate) fn scale_click(&mut self, editor: &mut Editor, ctx: &Ctx) {
        match self.state.clone() {
            State::Idle => {
                let elements = editor.selected();
                let basis = self.axes.basis();
                let Some((lo, hi)) = element_bounds_in_axes(editor, &elements, basis, Some(ctx.view)) else {
                    // Nothing selected: the click selects, and the grips appear.
                    if let Some(e) = self.pick(editor, ctx) {
                        editor.set_selection([e]);
                    }
                    return;
                };
                let hit =
                    pick_handle(ctx, grips(lo, hi).into_iter().map(|(grip, p)| (grip, basis.transform_point3(p))));
                let Some((grip, handle)) = hit else {
                    match self.pick(editor, ctx) {
                        Some(e) => editor.set_selection([e]),
                        None => editor.clear_selection(),
                    }
                    return;
                };
                let c = basis.transform_point3((lo + hi) * 0.5);
                let fixed = if ctx.modifiers.ctrl { c } else { c - (handle - c) };
                let opposite = c - (handle - c);
                let extent = hi - lo;
                let dimensions = (0..3).filter(|i| extent[*i] > EPS).count();
                let uniform = (grip.0.iter().filter(|v| **v != 0).count() == dimensions) != ctx.modifiers.shift;
                let normal = (0..3)
                    .find(|i| grip.0[*i] == 0 && extent[*i] > EPS)
                    .map(|i| {
                        basis.transform_vector3(match i {
                            0 => DVec3::X,
                            1 => DVec3::Y,
                            _ => DVec3::Z,
                        })
                    })
                    .unwrap_or_else(|| {
                        if dimensions == 2 { basis.z_axis.truncate() } else { (ctx.view.eye - handle).normalize() }
                    });
                self.state = State::Scaling {
                    elements,
                    grip,
                    fixed,
                    handle,
                    factors: DVec3::ONE,
                    basis,
                    extent,
                    centre: c,
                    opposite,
                    uniform,
                    drag_plane: Plane::new(normal, handle),
                };
            }
            State::Scaling { elements, grip, fixed, factors, basis, extent, uniform, .. } => {
                self.finish_scale(
                    editor,
                    ScaleCompletion { elements, origin: fixed, grip, factors, basis, extent, uniform },
                );
            }
            _ => {}
        }
    }

    pub(crate) fn scale_modifiers(&mut self, ctx: &Ctx) {
        self.preview_copy = ctx.modifiers.ctrl;
        if let State::Scaling { fixed, centre, opposite, uniform, extent, grip, .. } = &mut self.state {
            *fixed = if ctx.modifiers.ctrl { *centre } else { *opposite };
            let dimensions = (0..3).filter(|i| extent[*i] > EPS).count();
            *uniform = (grip.0.iter().filter(|v| **v != 0).count() == dimensions) != ctx.modifiers.shift;
        }
    }

    pub(crate) fn scale_move(&mut self) {
        let Some(inf) = self.inference else { return };
        let State::Scaling { grip, fixed, handle, basis, uniform, extent, .. } = self.state.clone() else { return };
        let d = handle - fixed;
        if d.length_squared() < EPS * EPS {
            return;
        }
        let scalar = (inf.point - fixed).dot(d) / d.length_squared();
        let local_d = basis.inverse().transform_vector3(d);
        let local_to = basis.inverse().transform_vector3(inf.point - fixed);
        let clamp = |v: f64| if v.abs() < 1e-3 { 1e-3f64.copysign(v) } else { v };
        let mut f = DVec3::ONE;
        for i in 0..3 {
            if uniform && extent[i] > EPS {
                f[i] = clamp(scalar);
            } else if grip.0[i] != 0 && local_d[i].abs() > EPS {
                f[i] = clamp(local_to[i] / local_d[i]);
            }
        }
        if let State::Scaling { factors, .. } = &mut self.state {
            *factors = f;
        }
    }

    fn scale_command(elements: Vec<Element>, origin: DVec3, factors: DVec3, basis: DMat4) -> Command {
        Command::ScaleInAxes { elements, origin, factors, x: basis.x_axis.truncate(), y: basis.y_axis.truncate() }
    }

    fn finish_scale(&mut self, editor: &mut Editor, completion: ScaleCompletion) {
        let ScaleCompletion { elements, origin, grip, factors, basis, extent, uniform } = completion;
        if self.run(editor, Self::scale_command(elements.clone(), origin, factors, basis)).is_some() {
            self.last =
                Last::Scale { elements, origin, grip, factors, basis, extent, revision: editor.revision(), uniform };
        }
        self.reset();
    }

    pub(crate) fn scale_vcb(&mut self, editor: &mut Editor, text: &str) -> Result<(), String> {
        let (elements, origin, grip, extent, basis, uniform) = match self.state.clone() {
            State::Scaling { elements, fixed, grip, extent, basis, uniform, .. } => {
                (elements, fixed, grip, extent, basis, uniform)
            }
            State::Idle => match self.last.clone() {
                Last::Scale { elements, origin, grip, extent, basis, revision, uniform, .. } => {
                    if revision != editor.revision() {
                        return Err("The model changed. Pick a scale grip again.".into());
                    }
                    (elements, origin, grip, extent, basis, uniform)
                }
                _ => return Err("Pick a grip first.".into()),
            },
            _ => return Err("This value does not apply here.".into()),
        };
        let involved: Vec<usize> = (0..3).filter(|i| grip.0[*i] != 0).collect();
        let parts: Vec<&str> = if text.contains(';') { text.split(';').collect() } else { text.split(',').collect() };
        let mut values = Vec::new();
        for (k, part) in parts.iter().enumerate() {
            let p = part.trim();
            if p.chars().any(|c| c.is_ascii_alphabetic()) {
                let len = parse_length(p, editor.doc().units).map_err(|e| e.to_string())?;
                let axis = involved.get(k).copied().unwrap_or(involved[0]);
                let size = extent[axis].abs();
                if size < EPS {
                    return Err("That direction has no size to scale.".into());
                }
                values.push(len / size);
            } else {
                values.push(p.replace(',', ".").parse::<f64>().map_err(|_| format!("\"{p}\" is not a factor"))?);
            }
        }
        if values.iter().any(|v| !v.is_finite() || v.abs() < 1e-9) {
            return Err("Enter finite nonzero scale values.".into());
        }
        let mut f = DVec3::ONE;
        for (k, axis) in involved.iter().enumerate() {
            f[*axis] = *values.get(k).or(values.first()).ok_or("Type a factor.")?;
        }
        if uniform && values.len() == 1 {
            for i in 0..3 {
                if extent[i] > EPS {
                    f[i] = values[0];
                }
            }
        }
        if self.state == State::Idle {
            self.correct_last(editor, Self::scale_command(elements.clone(), origin, f, basis))?;
            self.last =
                Last::Scale { elements, origin, grip, factors: f, basis, extent, revision: editor.revision(), uniform };
        } else {
            self.finish_scale(editor, ScaleCompletion { elements, origin, grip, factors: f, basis, extent, uniform });
        }
        self.update_status();
        Ok(())
    }

    // ----- Offset -----

    pub(crate) fn offset_click(&mut self, editor: &mut Editor, ctx: &Ctx, clicks: u8) {
        match self.state.clone() {
            State::Idle => {
                let face = editor.mesh().pick_face(&ctx.view.ray(ctx.cursor)).map(|(f, _)| f);
                if clicks >= 2 {
                    if let (Some(f), Some(d)) = (face, self.last_offset) {
                        self.finish_offset(editor, OffsetTarget::Face(f), Vec::new(), d);
                    }
                    return;
                }
                let edges: Vec<EdgeId> = editor
                    .selected()
                    .into_iter()
                    .filter_map(|e| if let Element::Edge(x) = e { Some(x) } else { None })
                    .collect();
                let mesh = editor.mesh();
                if !edges.is_empty() && editor.selected().len() == edges.len() {
                    let Some((chain, _)) = mesh.edge_chain(&edges) else {
                        self.message = Some("The selected edges do not form one chain.".into());
                        return;
                    };
                    let pts = mesh.loop_points(&chain);
                    let normal = rysmo_math::newell_normal(&pts)
                        .or_else(|| face.and_then(|f| mesh.face_normal(f)))
                        .unwrap_or(DVec3::Z);
                    self.state = State::Offsetting { target: OffsetTarget::Chain(normal), edges, distance: 0.0 };
                } else if let Some(f) = face {
                    self.state = State::Offsetting { target: OffsetTarget::Face(f), edges: Vec::new(), distance: 0.0 };
                }
            }
            State::Offsetting { target, edges, distance } => {
                if distance.abs() < EPS {
                    self.reset();
                    if clicks >= 2
                        && let (OffsetTarget::Face(f), Some(d)) = (target, self.last_offset)
                    {
                        self.finish_offset(editor, OffsetTarget::Face(f), Vec::new(), d);
                    }
                } else {
                    self.finish_offset(editor, target, edges, distance);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn offset_move(&mut self, editor: &Editor, ctx: &Ctx) {
        let State::Offsetting { target, edges, .. } = self.state.clone() else { return };
        let mesh = editor.mesh();
        let ray = ctx.view.ray(ctx.cursor);
        let d = match target {
            OffsetTarget::Face(f) => {
                let (Some(plane), Some(face)) = (mesh.face_plane(f), mesh.face(f)) else { return };
                let Some(t) = plane.intersect_ray(&ray).or_else(|| plane.flipped().intersect_ray(&ray)) else { return };
                let q = plane.to_2d(ray.at(t));
                let dist = face
                    .loops()
                    .map(|ring| {
                        let points: Vec<_> = ring.iter().map(|v| plane.to_2d(mesh.pos(*v))).collect();
                        (0..points.len())
                            .map(|i| distance_point_segment_2d(q, points[i], points[(i + 1) % points.len()]))
                            .fold(f64::MAX, f64::min)
                    })
                    .fold(f64::MAX, f64::min);
                if mesh.face_contains(f, ray.at(t)) { dist } else { -dist }
            }
            OffsetTarget::Chain(n) => {
                let Some((chain, closed)) = mesh.edge_chain(&edges) else { return };
                let pts = mesh.loop_points(&chain);
                let plane = Plane::new(n, pts[0]);
                let Some(t) = plane.intersect_ray(&ray).or_else(|| plane.flipped().intersect_ray(&ray)) else { return };
                let q = ray.at(t);
                let segs = if closed { pts.len() } else { pts.len() - 1 };
                let mut best = (f64::MAX, 0.0);
                for i in 0..segs {
                    let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                    let (c, _) = rysmo_math::closest_on_segment(a, b, q);
                    let dist = c.distance(q);
                    if dist < best.0 {
                        let left = n.cross((b - a).normalize());
                        best = (dist, if (q - c).dot(left) >= 0.0 { dist } else { -dist });
                    }
                }
                best.1
            }
        };
        if let State::Offsetting { distance, .. } = &mut self.state {
            *distance = editor.doc().measurement.snap_length(d);
        }
    }

    pub(crate) fn finish_offset(
        &mut self,
        editor: &mut Editor,
        target: OffsetTarget,
        edges: Vec<EdgeId>,
        distance: f64,
    ) {
        let cmd = match target {
            OffsetTarget::Face(face) => Command::OffsetFace { face, distance },
            OffsetTarget::Chain(normal) => Command::OffsetEdges { edges: edges.clone(), normal, distance },
        };
        if self.run(editor, cmd).is_some() {
            self.last = Last::Offset { target, edges, distance, revision: editor.revision() };
            self.last_offset = Some(distance);
        }
        self.reset();
    }

    // ----- Follow Me -----

    pub(crate) fn follow_me_click(&mut self, editor: &mut Editor, ctx: &Ctx) {
        if let State::Following { face, path, .. } = self.state.clone() {
            if path.is_empty() {
                self.message = Some("Touch a path edge first.".into());
                return;
            }
            if self.run(editor, Command::FollowMe { face, path }).is_some() {
                editor.clear_selection();
                self.reset();
            }
            return;
        }
        let Some(Element::Face(face)) = self.pick(editor, ctx) else {
            self.message = Some("Click the profile face to sweep.".into());
            return;
        };
        let mut path = Vec::new();
        for element in editor.selected() {
            match element {
                Element::Edge(edge) => path.push(edge),
                Element::Face(id) if id != face => {
                    if let Some(face) = editor.mesh().face(id) {
                        for i in 0..face.outer.len() {
                            if let Some(edge) =
                                editor.mesh().edge_between(face.outer[i], face.outer[(i + 1) % face.outer.len()])
                            {
                                path.push(edge);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        path.sort();
        path.dedup();
        if !path.is_empty() {
            if self.run(editor, Command::FollowMe { face, path }).is_some() {
                editor.clear_selection();
            }
        } else {
            self.state = State::Following {
                face,
                path,
                automatic: ctx.modifiers.alt,
                revision: editor.revision(),
                context: editor.context().to_vec(),
                preview: Vec::new(),
            };
        }
        self.last = Last::None;
    }

    pub(crate) fn follow_me_move(&mut self, editor: &Editor, ctx: &Ctx) {
        let State::Following { face, mut path, automatic, .. } = self.state.clone() else { return };
        let old = path.clone();
        match self.pick(editor, ctx) {
            Some(Element::Face(id)) if automatic && id != face => {
                if let Some(surface) = editor.mesh().face(id) {
                    path = (0..surface.outer.len())
                        .filter_map(|i| {
                            editor.mesh().edge_between(surface.outer[i], surface.outer[(i + 1) % surface.outer.len()])
                        })
                        .collect();
                }
            }
            Some(Element::Edge(edge)) if !automatic && !editor.mesh().face_edges(face).contains(&edge) => {
                if let Some(index) = path.iter().position(|id| *id == edge) {
                    if !editor.mesh().edge_chain(&path).is_some_and(|(_, closed)| closed) {
                        path.truncate(index + 1);
                    }
                } else {
                    let mut candidate = path.clone();
                    candidate.push(edge);
                    if editor.mesh().edge_chain(&candidate).is_some() {
                        path = candidate;
                    }
                }
            }
            _ => {}
        }
        if path == old {
            return;
        }
        let mut trial = editor.clone();
        let preview = match trial.apply(Command::FollowMe { face, path: path.clone() }) {
            Ok(out) => {
                let edges: BTreeSet<_> = out
                    .created
                    .iter()
                    .filter_map(|e| if let Element::Face(id) = e { Some(*id) } else { None })
                    .flat_map(|id| trial.mesh().face_edges(id))
                    .collect();
                self.message = None;
                edges.into_iter().map(|edge| trial.mesh().edge_points(edge)).collect()
            }
            Err(error) => {
                self.message = Some(error.to_string());
                Vec::new()
            }
        };
        if let State::Following { path: saved, preview: saved_preview, .. } = &mut self.state {
            *saved = path;
            *saved_preview = preview;
        }
    }

    // ----- Tape Measure -----

    pub(crate) fn tape_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        self.last = Last::None;
        match self.state.clone() {
            State::Idle => {
                // From an edge (not its ends) the tape makes a guide parallel to it.
                let along = match (inf.kind, inf.edge) {
                    (Kind::OnEdge, Some(e)) if editor.mesh().edge(e).is_some() => {
                        let (a, b) = editor.mesh().edge_points(e);
                        Some((b - a).normalize())
                    }
                    (Kind::OnGuide, _) => inf.construction.and_then(|g| match editor.active().guides.get(&g) {
                        Some(rysmo_model::Guide::Line { dir, .. }) => Some(*dir),
                        _ => None,
                    }),
                    _ => None,
                };
                self.state = State::Taping { start: inf.point, along };
            }
            State::Taping { start, along } => {
                let end = inf.point;
                let measured = match along.filter(|_| !ctx.modifiers.ctrl) {
                    Some(dir) => project_to_plane(end - start, dir).length(),
                    None => end.distance(start),
                };
                if !ctx.modifiers.ctrl {
                    match along {
                        Some(dir) => {
                            self.run(editor, Command::AddGuideLine { origin: end, dir });
                        }
                        None if end.distance(start) > EPS => {
                            self.run(editor, Command::AddGuideSegment { start, point: end });
                        }
                        None => {}
                    }
                }
                self.reset();
                if measured > EPS {
                    self.last = if let Some(dir) = along.filter(|_| !ctx.modifiers.ctrl) {
                        let direction = project_to_plane(end - start, dir).normalize_or(DVec3::X);
                        Last::TapeGuide { start, along, direction, length: measured, revision: editor.revision() }
                    } else {
                        Last::TapeMeasurement { length: measured, revision: editor.revision() }
                    };
                }
                if self.message.is_none() {
                    self.message = Some(format!("Length {}", editor.doc().format_length(measured)));
                }
            }
            _ => {}
        }
    }

    pub(crate) fn tape_value(
        &mut self,
        editor: &mut Editor,
        ctx: &Ctx,
        start: DVec3,
        along: Option<DVec3>,
        len: f64,
    ) -> Result<(), String> {
        if !len.is_finite() || len.abs() <= EPS {
            return Err("Enter a nonzero distance.".into());
        }
        let current = self.inference.map(|i| i.point).ok_or("Move the cursor to show the direction first.")?;
        let cmd = match along.filter(|_| !ctx.modifiers.ctrl) {
            Some(dir) => {
                let perp = project_to_plane(current - start, dir);
                if perp.length() < EPS {
                    return Err("Move the cursor off the edge to show the side.".into());
                }
                Command::AddGuideLine { origin: start + perp.normalize() * len, dir }
            }
            None => {
                let d = current - start;
                if d.length() < EPS {
                    return Err("Move the cursor to show the direction first.".into());
                }
                Command::AddGuideSegment { start, point: start + d.normalize() * len }
            }
        };
        if ctx.modifiers.ctrl {
            if !len.is_finite() || len.abs() <= EPS {
                return Err("Enter a nonzero measurement.".into());
            }
            self.reset();
            self.last = Last::TapeMeasurement { length: len.abs(), revision: editor.revision() };
            self.message = Some(format!("Length {}", editor.doc().format_length(len.abs())));
        } else {
            if self.run(editor, cmd).is_none() {
                return Err(self.message.clone().unwrap_or_else(|| "Cannot create guide.".into()));
            }
            let direction = match along {
                Some(dir) => project_to_plane(current - start, dir),
                None => current - start,
            }
            .normalize();
            self.reset();
            self.last = Last::TapeGuide { start, along, direction, length: len, revision: editor.revision() };
        }
        Ok(())
    }

    // ----- Paint Bucket -----

    pub(crate) fn paint_click(&mut self, editor: &mut Editor, ctx: &Ctx) {
        let Some(target) = self.pick(editor, ctx) else { return };
        let mesh = editor.mesh();
        let back_side = |f: FaceId| {
            let ray = ctx.view.ray(ctx.cursor);
            mesh.face_normal(f).is_some_and(|n| n.dot(ray.dir) > 0.0)
        };
        if ctx.modifiers.alt {
            self.paint_placement = if let Element::Face(f) = target {
                Some((
                    mesh.face(f).expect("picked").uv.unwrap_or_default(),
                    editor.context().to_vec(),
                    f,
                    editor.context_transform(),
                    mesh.face_plane(f).expect("picked planar face"),
                ))
            } else {
                None
            };
            // Sample.
            self.paint = match target {
                Element::Face(f) => {
                    let face = mesh.face(f).expect("picked");
                    if back_side(f) { face.back_material } else { face.material }
                }
                Element::Instance(i) => editor.active().instances.get(&i).and_then(|x| x.material),
                Element::Edge(id) => mesh.edge(id).and_then(|e| e.material),
                _ => self.paint,
            };
            let name = self.paint.and_then(|m| editor.doc().materials.get(&m)).map(|m| m.name.clone());
            self.message = Some(format!("Sampled {}.", name.unwrap_or_else(|| "the default material".into())));
            return;
        }
        let material = self.paint;
        if matches!(target, Element::Face(_) | Element::Instance(_) | Element::Edge(_)) && editor.is_selected(target) {
            let back = if let Element::Face(f) = target { back_side(f) } else { false };
            let elements = editor.selected().into_iter().filter(|e| !back || !matches!(e, Element::Edge(_))).collect();
            let command = self.positioned_paint(
                editor,
                elements,
                material,
                back,
                if let Element::Face(f) = target { Some(f) } else { None },
            );
            self.run(editor, command);
            return;
        }
        match target {
            Element::Face(f) => {
                let back = back_side(f);
                let current = |g: FaceId| mesh.face(g).map(|x| if back { x.back_material } else { x.material });
                let was = current(f);
                let faces: Vec<FaceId> = match (ctx.modifiers.ctrl, ctx.modifiers.shift) {
                    (true, true) => super::select::connected(mesh, Element::Face(f))
                        .into_iter()
                        .filter_map(|e| if let Element::Face(g) = e { Some(g) } else { None })
                        .filter(|g| current(*g) == was)
                        .collect(),
                    (false, true) => mesh.faces().map(|(g, _)| g).filter(|g| current(*g) == was).collect(),
                    (true, false) => {
                        let mut seen = BTreeSet::from([f]);
                        let mut pending = vec![f];
                        while let Some(face) = pending.pop() {
                            for edge in mesh.face_edges(face) {
                                for neighbour in mesh.faces_of_edge(edge) {
                                    if current(neighbour) == was && seen.insert(neighbour) {
                                        pending.push(neighbour);
                                    }
                                }
                            }
                        }
                        seen.into_iter().collect()
                    }
                    (false, false) if self.paint_surface => {
                        let mut seen = BTreeSet::from([f]);
                        let mut pending = vec![f];
                        while let Some(face) = pending.pop() {
                            for edge in mesh.face_edges(face) {
                                if !mesh.edge(edge).is_some_and(|e| e.soft) {
                                    continue;
                                }
                                for neighbour in mesh.faces_of_edge(edge) {
                                    if seen.insert(neighbour) {
                                        pending.push(neighbour);
                                    }
                                }
                            }
                        }
                        seen.into_iter().collect()
                    }
                    (false, false) => vec![f],
                };
                let command = self.positioned_paint(
                    editor,
                    faces.into_iter().map(Element::Face).collect(),
                    material,
                    back,
                    Some(f),
                );
                self.run(editor, command);
            }
            Element::Edge(id) => {
                let elements = mesh
                    .edge(id)
                    .and_then(|edge| edge.curve)
                    .map(|curve| mesh.curve_edges(curve))
                    .unwrap_or(vec![id])
                    .into_iter()
                    .map(Element::Edge)
                    .collect();
                if self.run(editor, Command::PaintElements { elements, material, back: false }).is_some()
                    && editor.doc().style.edge_color != rysmo_model::EdgeColor::ByMaterial
                {
                    self.message =
                        Some("Painted edges. Enable By material edge color in Settings to see their colors.".into());
                }
            }
            Element::Instance(i) => {
                self.run(editor, Command::PaintInstances { instances: vec![i], material });
            }
            _ => {}
        }
    }

    fn positioned_paint(
        &self,
        editor: &Editor,
        elements: Vec<Element>,
        material: Option<rysmo_model::MaterialId>,
        back: bool,
        clicked: Option<FaceId>,
    ) -> Command {
        if let Some(texture) = material.and_then(|m| editor.doc().materials.get(&m)).and_then(|m| m.texture) {
            if let Some((placement, context, source, frame, plane)) = &self.paint_placement {
                if context == editor.context() && editor.mesh().face(*source).is_some() {
                    return Command::PaintWrappedElements {
                        elements,
                        material,
                        back,
                        source: *source,
                        placement: *placement,
                    };
                }
                let transform = editor.context_transform().inverse() * *frame;
                if let Some(placement) = rysmo_model::transformed_placement(*plane, texture.size, *placement, transform)
                {
                    return Command::PaintPositionedElements { elements, material, back, placement };
                }
            } else if let Some(source) = clicked
                && elements.iter().filter(|e| matches!(e, Element::Face(_))).count() > 1
            {
                let placement = editor.mesh().face(source).unwrap().uv.unwrap_or_default();
                return Command::PaintWrappedElements { elements, material, back, source, placement };
            }
        }
        Command::PaintElements { elements, material, back }
    }

    // ----- Eraser -----

    pub(crate) fn finish_erase(&mut self, editor: &mut Editor, ctx: &Ctx, marked: BTreeSet<Element>) {
        if marked.is_empty() {
            return;
        }
        let edges: Vec<EdgeId> =
            marked.iter().filter_map(|e| if let Element::Edge(x) = e { Some(*x) } else { None }).collect();
        let m = ctx.modifiers;
        let cmd = match (m.ctrl, m.shift) {
            (true, true) => Command::SetEdges { edges, soft: Some(false), smooth: Some(false), hidden: None },
            (true, false) => Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None },
            (false, true) => Command::SetEdges { edges, soft: None, smooth: None, hidden: Some(true) },
            (false, false) => Command::Erase { elements: marked.into_iter().collect() },
        };
        self.run(editor, cmd);
    }

    // ----- previews -----

    pub(crate) fn preview_edit(&self, editor: &Editor, pv: &mut Preview) {
        let mesh = editor.mesh();
        let push = |pv: &mut Preview, a: DVec3, b: DVec3, color: [f32; 3], dashed: bool| {
            pv.lines.push(PreviewLine { a, b, color, dashed, thick: true });
        };
        let protractor = |pv: &mut Preview, centre: DVec3, axis: DVec3, r: f64, reference: Option<DVec3>| {
            let (u, v) = Plane::new(axis, centre).basis();
            let n = 36;
            let pts: Vec<DVec3> = (0..=n)
                .map(|k| {
                    let a = k as f64 / n as f64 * std::f64::consts::TAU;
                    centre + (u * a.cos() + v * a.sin()) * r
                })
                .collect();
            let color = if rysmo_math::parallel(axis.normalize(), DVec3::Z) {
                [0.0, 0.0, 0.95]
            } else if rysmo_math::parallel(axis.normalize(), DVec3::X) {
                [0.9, 0.0, 0.0]
            } else if rysmo_math::parallel(axis.normalize(), DVec3::Y) {
                [0.0, 0.7, 0.0]
            } else {
                BLACK
            };
            for w in pts.windows(2) {
                pv.lines.push(PreviewLine { a: w[0], b: w[1], color, dashed: false, thick: false });
            }
            super::protractor::append_ticks(pv, centre, axis, reference.unwrap_or(u), r, editor.doc().measurement);
        };
        match &self.state {
            State::PushPull { face, normal, distance, .. } => {
                if let Some(f) = mesh.face(*face) {
                    let off = *normal * *distance;
                    for l in f.loops() {
                        for k in 0..l.len() {
                            let a = mesh.pos(l[k]);
                            let b = mesh.pos(l[(k + 1) % l.len()]);
                            push(pv, a + off, b + off, BLACK, false);
                            push(pv, a, a + off, BLACK, false);
                        }
                    }
                    pv.faces.push(*face);
                }
            }
            State::Move { elements, start, offset } => {
                if let Some(view) = self.current_view {
                    let result = if self.preview_copy {
                        editor.preview_text_copy(elements, *offset, *start, view)
                    } else {
                        editor.preview_text_move(elements, *offset, *start, view)
                    };
                    match result {
                        Ok(texts) => pv.moving_texts = texts,
                        Err(error) => pv.label = Some(error.to_string()),
                    }
                }
                match editor.preview_instance_texts(elements, DMat4::from_translation(*offset)) {
                    Ok(texts) => pv.instanced_texts = texts,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                match editor.preview_instance_dimensions(elements, DMat4::from_translation(*offset)) {
                    Ok(dimensions) => pv.instanced_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                let dimension_elements =
                    elements.iter().copied().filter(|e| !matches!(e, Element::Text(_))).collect::<Vec<_>>();
                match editor.preview_dimension_move(&dimension_elements, *offset, self.preview_copy) {
                    Ok(dimensions) => pv.moving_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                for (a, b) in element_lines(editor, elements) {
                    push(pv, a + *offset, b + *offset, MAGENTA, false);
                }
                push(pv, *start, *start + *offset, line_color(self.inference.as_ref()), true);
            }
            State::RotatePivot { centre, axis, .. } => {
                protractor(pv, *centre, *axis, PROTRACTOR_PX * self.px, None);
                if let Some(i) = &self.inference {
                    push(pv, *centre, i.point, BLACK, true);
                }
            }
            State::Rotating { elements, centre, axis, from, angle } => {
                let r = PROTRACTOR_PX * self.px;
                protractor(pv, *centre, *axis, r, Some(*from));
                let m = rotate_about(*centre, *axis, *angle);
                {
                    match editor.preview_text_rotation(elements, *centre, *axis, *angle, self.preview_copy) {
                        Ok(texts) => pv.moving_texts = texts,
                        Err(error) => pv.label = Some(error.to_string()),
                    }
                }
                match editor.preview_instance_texts(elements, m) {
                    Ok(texts) => pv.instanced_texts = texts,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                match editor.preview_instance_dimensions(elements, m) {
                    Ok(dimensions) => pv.instanced_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                push(pv, *centre, *centre + *from * r * 1.6, BLACK, true);
                push(pv, *centre, m.transform_point3(*centre + *from * r * 1.6), BLACK, true);
                for (a, b) in element_lines(editor, elements) {
                    push(pv, m.transform_point3(a), m.transform_point3(b), MAGENTA, false);
                }
                pv.label = Some(editor.doc().measurement.format_angle(*angle));
                match editor.preview_dimension_rotate(elements, *centre, *axis, *angle, self.preview_copy) {
                    Ok(dimensions) => pv.moving_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
            }
            State::Scaling { elements, fixed, factors, basis, .. } => {
                let m = DMat4::from_translation(*fixed)
                    * *basis
                    * DMat4::from_scale(*factors)
                    * basis.inverse()
                    * DMat4::from_translation(-*fixed);
                match editor.preview_text_transform(elements, m) {
                    Ok(texts) => pv.moving_texts = texts,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                match editor.preview_instance_texts(elements, m) {
                    Ok(texts) => pv.instanced_texts = texts,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                match editor.preview_instance_dimensions(elements, m) {
                    Ok(dimensions) => pv.instanced_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                for (a, b) in element_lines(editor, elements) {
                    push(pv, m.transform_point3(a), m.transform_point3(b), MAGENTA, false);
                }
                match editor.preview_dimension_scale(elements, *fixed, *factors, *basis) {
                    Ok(dimensions) => pv.moving_dimensions = dimensions,
                    Err(error) => pv.label = Some(error.to_string()),
                }
                pv.markers.push((*fixed, BLACK));
                self.preview_hover(editor, pv);
            }
            State::Following { face, path, revision, context, preview, .. } => {
                if *revision != editor.revision() || context != editor.context() {
                    return;
                }
                pv.faces.push(*face);
                for (a, b) in preview {
                    push(pv, *a, *b, MAGENTA, false);
                }
                for edge in path {
                    if mesh.edge(*edge).is_some() {
                        let (a, b) = mesh.edge_points(*edge);
                        push(pv, a, b, GRIP, false);
                    }
                }
            }
            State::Offsetting { target, edges, distance } => {
                let pts = match target {
                    OffsetTarget::Face(f) => {
                        pv.faces.push(*f);
                        mesh.offset_face_loops(*f, *distance)
                            .map(|loops| loops.into_iter().map(|p| (p, true)).collect::<Vec<_>>())
                    }
                    OffsetTarget::Chain(n) => mesh.edge_chain(edges).and_then(|(chain, closed)| {
                        offset_polyline(&mesh.loop_points(&chain), *n, *distance, closed).map(|p| vec![(p, closed)])
                    }),
                };
                if let Some(loops) = pts {
                    for (pts, closed) in loops {
                        let segs = if closed { pts.len() } else { pts.len() - 1 };
                        for i in 0..segs {
                            push(pv, pts[i], pts[(i + 1) % pts.len()], BLACK, false);
                        }
                    }
                }
            }
            State::Taping { start, along } => {
                if let Some(i) = &self.inference {
                    push(pv, *start, i.point, BLACK, true);
                    if !self.tape_measure_only
                        && let Some(dir) = along
                    {
                        push(pv, i.point - *dir * 100.0, i.point + *dir * 100.0, BLACK, true);
                    }
                }
            }
            State::Erasing { marked } => {
                for e in marked {
                    match e {
                        Element::Edge(x) => pv.edges.push(*x),
                        Element::Instance(i) => pv.instances.push(*i),
                        _ => {}
                    }
                }
            }
            State::Idle => self.preview_hover(editor, pv),
            _ => {}
        }
    }

    /// What a tool would act on, before the first click.
    fn preview_hover(&self, editor: &Editor, pv: &mut Preview) {
        let inf = self.inference.as_ref();
        match self.active() {
            ToolKind::PushPull | ToolKind::Offset | ToolKind::FollowMe => pv.faces.extend(inf.and_then(|i| i.face)),
            ToolKind::PaintBucket => {
                if let Some(i) = inf {
                    pv.faces.extend(i.face);
                    pv.instances.extend(i.instance);
                }
            }
            ToolKind::Move | ToolKind::Rotate if editor.selection().is_empty() => match inf.and_then(hovered) {
                Some(Element::Edge(e)) => pv.edges.push(e),
                Some(Element::Face(f)) => pv.faces.push(f),
                Some(Element::Instance(i)) => pv.instances.push(i),
                _ => {}
            },
            ToolKind::Rotate if matches!(self.state, State::Idle) => {
                for (axis, point) in rotation_handles(editor, self.axes.basis()) {
                    let color = if axis.dot(self.axes.x).abs() > 0.9 {
                        [1.0, 0.35, 0.25]
                    } else if axis.dot(self.axes.y).abs() > 0.9 {
                        [0.35, 0.9, 0.4]
                    } else {
                        [0.35, 0.65, 1.0]
                    };
                    let u = axis.any_orthonormal_vector() * self.px * 9.0;
                    let v = axis.cross(u);
                    for i in 0..24 {
                        let a = i as f64 * std::f64::consts::TAU / 24.0;
                        let b = (i + 1) as f64 * std::f64::consts::TAU / 24.0;
                        pv.lines.push(PreviewLine {
                            a: point + u * a.cos() + v * a.sin(),
                            b: point + u * b.cos() + v * b.sin(),
                            color,
                            dashed: false,
                            thick: true,
                        });
                    }
                    pv.markers.push((point, color));
                }
            }
            ToolKind::Scale => {
                let basis = if let State::Scaling { basis, .. } = self.state { basis } else { self.axes.basis() };
                let bounds = if let State::Scaling { centre, extent, .. } = self.state {
                    let c = basis.inverse().transform_point3(centre);
                    Some((c - extent * 0.5, c + extent * 0.5))
                } else {
                    element_bounds_in_axes(editor, &editor.selected(), basis, self.current_view.as_ref())
                };
                if let Some((lo, hi)) = bounds {
                    let transform = if let State::Scaling { fixed, factors, .. } = self.state {
                        DMat4::from_translation(fixed)
                            * basis
                            * DMat4::from_scale(factors)
                            * basis.inverse()
                            * DMat4::from_translation(-fixed)
                            * basis
                    } else {
                        basis
                    };
                    for (a, b) in super::box_lines(lo, hi, &transform) {
                        pv.lines.push(PreviewLine { a, b, color: [0.9, 0.75, 0.0], dashed: false, thick: false });
                    }
                    for (_, p) in grips(lo, hi) {
                        pv.markers.push((transform.transform_point3(p), GRIP));
                    }
                }
            }
            _ => {}
        }
    }
}

fn project_to_plane(v: DVec3, normal: DVec3) -> DVec3 {
    let n = normal.normalize();
    v - n * v.dot(n)
}
