//! A circumcircle through start, pivot and endpoint, with a preserved pivot vertex.
use super::*;
use rysmo_model::arcs::{ArcParameters, three_point_parameters, three_point_points};

impl ToolBox {
    pub(super) fn three_stale(&mut self, editor: &Editor) -> bool {
        let stale = match &self.state {
            State::ThreeMiddle { revision, context, .. } | State::ThreeEnd { revision, context, .. } => {
                *revision != editor.revision() || context != editor.context()
            }
            _ => false,
        };
        if stale {
            self.reset();
            self.last = Last::None;
            self.message = Some("The model or context changed; start a new three-point arc.".into());
        }
        stale
    }
    pub(super) fn three_lock(&self) -> Option<Lock> {
        let (origin, plane) = match self.state {
            State::ThreeMiddle { start, plane, .. } => (start, plane),
            State::ThreeEnd { through, plane, .. } => (through, plane),
            _ => return None,
        };
        let direction = match self.shift_lock {
            Some(Lock::Line { dir, .. }) => Some(dir),
            _ => self.axis_lock.map(|a| self.axes.dir(a)),
        };
        if let Some(dir) = direction.and_then(|d| (d - plane.normal * d.dot(plane.normal)).try_normalize()) {
            Some(Lock::Line { origin, dir, axis: None })
        } else {
            Some(Lock::Plane(plane))
        }
    }
    pub(super) fn three_click(&mut self, editor: &mut Editor, _ctx: &Ctx, inf: Inference) {
        match self.state.clone() {
            State::Idle => {
                let plane = self
                    .axis_lock
                    .map(|a| Plane::new(self.axes.dir(a), inf.point))
                    .unwrap_or_else(|| super::draw::drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::ThreeMiddle {
                    start: inf.point,
                    plane: Plane::new(plane.normal, inf.point),
                    revision: editor.revision(),
                    context: editor.context().to_vec(),
                };
                self.axis_lock = None;
                self.shift_lock = None;
                self.last = Last::None;
            }
            State::ThreeMiddle { start, plane, revision, context } => {
                let through = plane.project(inf.point);
                if start.distance(through) <= EPS {
                    self.message = Some("Choose a distinct point on the arc.".into());
                    return;
                }
                self.state = State::ThreeEnd { start, through, plane, revision, context };
                self.axis_lock = None;
                self.shift_lock = None;
            }
            State::ThreeEnd { start, through, plane, .. } => {
                if let Err(error) = self.finish_three(editor, start, through, plane.project(inf.point), plane.normal) {
                    self.message = Some(error);
                }
            }
            _ => {}
        }
    }
    fn finish_three(
        &mut self,
        editor: &mut Editor,
        start: DVec3,
        through: DVec3,
        end: DVec3,
        normal: DVec3,
    ) -> Result<(), String> {
        if three_point_parameters(start, through, end, normal).is_none() {
            return Err("Choose three distinct, non-collinear points.".into());
        }
        let segments = self.arc_segments;
        let result = self
            .run(editor, Command::DrawThreePointArc { start, through, end, normal, segments })
            .ok_or_else(|| self.message.clone().unwrap_or_else(|| "Cannot create arc.".into()))?;
        self.message = None;
        self.reset();
        self.last = if result.changed {
            Last::ThreePointArc { start, through, end, normal, segments, revision: editor.revision() }
        } else {
            Last::None
        };
        Ok(())
    }
    pub(super) fn three_value(&self, editor: &Editor) -> String {
        match self.state {
            State::ThreeMiddle { start, .. } => {
                self.inference.map(|i| editor.doc().format_length(i.point.distance(start))).unwrap_or_default()
            }
            State::ThreeEnd { start, through, plane, .. } => self
                .inference
                .and_then(|i| three_point_parameters(start, through, plane.project(i.point), plane.normal))
                .map(|arc| editor.doc().measurement.format_angle(arc.sweep))
                .unwrap_or_default(),
            State::Idle => {
                if let Last::ThreePointArc { start, through, end, normal, .. } = self.last {
                    three_point_parameters(start, through, end, normal)
                        .map(|a| editor.doc().measurement.format_angle(a.sweep))
                        .unwrap_or_default()
                } else {
                    self.arc_segments.to_string()
                }
            }
            _ => String::new(),
        }
    }
    pub(super) fn three_preview(&self, editor: &Editor, pv: &mut Preview) {
        let Some(inf) = self.inference else { return };
        let mut line = |a, b, dashed| {
            pv.lines.push(PreviewLine { a, b, color: if dashed { MAGENTA } else { BLACK }, dashed, thick: false })
        };
        match self.state {
            State::ThreeMiddle { start, plane, .. } => line(start, plane.project(inf.point), false),
            State::ThreeEnd { start, through, plane, .. } => {
                let end = plane.project(inf.point);
                pv.markers.push((through, [0.2, 0.55, 1.]));
                line(start, through, true);
                line(through, end, true);
                if let Some((arc, points)) = three_point_points(start, through, end, plane.normal, self.arc_segments) {
                    for p in points.windows(2) {
                        line(p[0], p[1], false);
                    }
                    pv.label = Some(editor.doc().measurement.format_angle(arc.sweep));
                }
            }
            _ => {}
        }
    }
    pub(super) fn three_vcb(&mut self, editor: &mut Editor, ctx: &Ctx, text: &str) -> Result<(), String> {
        if self.three_stale(editor) {
            return Err("The model changed; start a new three-point arc.".into());
        }
        let text = text.trim().to_ascii_lowercase();
        if text.ends_with('s') || text.ends_with('c') {
            let circle = text.ends_with('c');
            let raw = &text[..text.len() - 1];
            let count = raw.trim().parse::<u32>().map_err(|_| "Enter a count such as 24s or 20c.")?;
            if count < if circle { 3 } else { 2 } || count > 4096 {
                return Err("Use 2-4096 arc segments or 3-4096 circle segments.".into());
            }
            let segments = if circle {
                let Last::ThreePointArc { start, through, end, normal, .. } = self.last else {
                    return Err("Draw the arc before circle-based conversion.".into());
                };
                if self.state != State::Idle {
                    return Err("Finish the arc before circle-based conversion.".into());
                }
                let arc = three_point_parameters(start, through, end, normal).ok_or("Invalid arc.")?;
                (count as f64 * arc.sweep.abs() / 360. - 1e-10).ceil().max(2.) as u32
            } else {
                count
            };
            if self.state == State::Idle && matches!(self.last, Last::ThreePointArc { .. }) {
                self.correct_three(editor, None, None, Some(segments))?;
            }
            self.arc_segments = segments;
            self.message = None;
            self.update_status();
            return Ok(());
        }
        if self.state == State::Idle {
            let radius = text
                .strip_suffix('r')
                .map(|r| rysmo_model::parse_length(r, editor.doc().units).map_err(|e| e.to_string()))
                .transpose()?;
            let angle = if radius.is_none() { Some(parse_angle(&text)?) } else { None };
            self.correct_three(editor, radius, angle, None)?;
            return Ok(());
        }
        match self.state.clone() {
            State::ThreeMiddle { start, plane, .. } => {
                let length = rysmo_model::parse_length(&text, editor.doc().units).map_err(|e| e.to_string())?;
                if !length.is_finite() || length <= EPS || !(length * length).is_finite() {
                    return Err("Enter a positive finite length.".into());
                }
                let mut inf = self.inference.ok_or("Move the pointer to choose the through-point direction.")?;
                let dir = (plane.project(inf.point) - start)
                    .try_normalize()
                    .ok_or("Move the pointer to choose a direction.")?;
                inf.point = start + dir * length;
                self.three_click(editor, ctx, inf);
            }
            State::ThreeEnd { start, through, plane, .. } => {
                let candidate =
                    self.inference.map(|i| plane.project(i.point)).ok_or("Move the pointer to show an arc first.")?;
                let arc = three_point_parameters(start, through, candidate, plane.normal)
                    .ok_or("Show a non-collinear arc preview before entering its total angle.")?;
                let end = end_for_angle(start, through, arc, parse_angle(&text)?)?;
                self.finish_three(editor, start, through, end, plane.normal)?;
            }
            _ => return Err("Select the start first.".into()),
        }
        self.message = None;
        self.update_status();
        Ok(())
    }
    fn correct_three(
        &mut self,
        editor: &mut Editor,
        radius: Option<f64>,
        angle: Option<f64>,
        count: Option<u32>,
    ) -> Result<(), String> {
        let Last::ThreePointArc { mut start, mut through, mut end, normal, segments, revision } = self.last else {
            return Err("Select the start first.".into());
        };
        if revision != editor.revision() {
            return Err("The model changed; draw a new arc.".into());
        }
        let arc = three_point_parameters(start, through, end, normal).ok_or("Invalid arc.")?;
        if let Some(radius) = radius {
            if !radius.is_finite() || radius <= EPS || !(radius * radius).is_finite() {
                return Err("Enter a positive finite radius.".into());
            }
            let scale = radius / arc.radius;
            start = arc.centre + (start - arc.centre) * scale;
            through = arc.centre + (through - arc.centre) * scale;
            end = arc.centre + (end - arc.centre) * scale;
        }
        if let Some(angle) = angle {
            end = end_for_angle(start, through, arc, angle)?;
        }
        let segments = count.unwrap_or(segments);
        self.correct_last(editor, Command::DrawThreePointArc { start, through, end, normal, segments })?;
        self.last = Last::ThreePointArc { start, through, end, normal, segments, revision: editor.revision() };
        self.message = None;
        self.update_status();
        Ok(())
    }
}
fn parse_angle(text: &str) -> Result<f64, String> {
    let value = text
        .strip_suffix("deg")
        .or_else(|| text.strip_suffix('°'))
        .unwrap_or(text)
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .map_err(|_| "Enter signed total degrees, such as -180deg.")?;
    if !value.is_finite() || value.abs() < 1e-6 || value.abs() >= 360. {
        return Err("Use a nonzero sweep below 360 degrees.".into());
    }
    Ok(value)
}
fn end_for_angle(start: DVec3, through: DVec3, arc: ArcParameters, angle: f64) -> Result<DVec3, String> {
    let end = arc.centre + rysmo_math::DQuat::from_axis_angle(arc.normal, angle.to_radians()) * arc.start * arc.radius;
    let candidate =
        three_point_parameters(start, through, end, arc.normal).ok_or("The requested angle degenerates the arc.")?;
    if (candidate.sweep - angle).abs() > 1e-5 {
        return Err("The total angle must still pass through the second point.".into());
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Rig;
    #[test]
    fn pointer_creation_passes_through_all_three_points_and_commits_only_once() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        let before = r.editor.doc().clone();
        let b = DVec3::new(0.7, 1.4, 0.);
        r.click(DVec3::ZERO);
        r.click(b);
        r.hover(DVec3::X * 2.);
        assert_eq!(r.editor.doc(), &before);
        assert!(r.tools.preview(&r.editor).lines.len() > 2);
        r.click(DVec3::X * 2.);
        assert!(!r.tools.busy());
        assert!(r.editor.mesh().find_vertex(b).is_some());
        assert_eq!(r.editor.mesh().curves().count(), 1);
        assert_eq!(r.editor.undo_depth(), 1);
        r.editor.undo();
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn exact_pivot_distance_and_signed_sweep_then_immediate_corrections() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        r.click(DVec3::ZERO);
        r.hover(DVec3::new(1., 1., 0.));
        r.type_vcb("1.4142135623730951m").unwrap();
        r.hover(DVec3::X * 2.);
        assert!(r.type_vcb("90deg").is_err());
        assert!(r.tools.busy());
        r.type_vcb("-270deg").unwrap();
        assert!((r.editor.mesh().curves().next().unwrap().1.sweep + 270.).abs() < 1e-7);
        r.type_vcb("-180deg").unwrap();
        r.type_vcb("2mr").unwrap();
        r.type_vcb("20s").unwrap();
        let (_, c) = r.editor.mesh().curves().next().unwrap();
        assert!((c.radius - 2.).abs() < 1e-7);
        assert_eq!(c.segments, 20);
        assert_eq!(r.editor.undo_depth(), 1);
        r.type_vcb("20c").unwrap();
        assert_eq!(r.editor.mesh().curves().next().unwrap().1.segments, 10);
        assert_eq!(r.editor.undo_depth(), 1);
    }
    #[test]
    fn repeated_collinear_and_invalid_values_preserve_the_draft() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        r.click(DVec3::ZERO);
        r.click(DVec3::ZERO);
        assert!(matches!(r.tools.state, State::ThreeMiddle { .. }));
        assert!(r.type_vcb("-1m").is_err());
        r.click(DVec3::X);
        r.click(DVec3::X * 2.);
        assert!(r.tools.busy());
        assert_eq!(r.editor.mesh().edge_count(), 0);
        assert!(r.type_vcb("180").is_err());
        assert!(r.type_vcb("360").is_err());
        r.key(Key::Escape);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.undo_depth(), 0);
    }
    #[test]
    fn vertical_plane_and_shift_direction_locks() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        r.key(Key::Left);
        r.click(DVec3::ZERO);
        r.click(DVec3::new(1., 0., 1.));
        r.click(DVec3::X * 2.);
        assert!(r.editor.mesh().vertices().all(|(_, v)| v.pos.y.abs() < 1e-8));
        assert!((r.editor.mesh().curves().next().unwrap().1.radius - 1.).abs() < 1e-7);
        let mut r = Rig::new(ToolKind::ThreePointArc);
        r.click(DVec3::ZERO);
        r.click(DVec3::new(1., 1., 0.));
        r.hover(DVec3::X * 2.);
        r.key(Key::Shift(true));
        r.hover(DVec3::X * 3.);
        let point = r.tools.preview(&r.editor).inference.unwrap().point;
        assert!((point.x + point.y - 2.).abs() < 1e-7);
    }
    #[test]
    fn endpoint_inferences_and_correction_segmentation_preserve_attached_pivot() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        let b = DVec3::new(0.37_f64.cos(), 0.37_f64.sin(), 0.);
        r.editor.apply(Command::DrawLines { lines: vec![[b, b + DVec3::Z]], facing: None }).unwrap();
        let mut settings = r.editor.doc().measurement;
        settings.length_snap = Some(0.5);
        r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
        r.click(DVec3::X);
        r.click(b);
        r.click(-DVec3::X);
        r.type_vcb("20s").unwrap();
        let v = r.editor.mesh().find_vertex(b).unwrap();
        let (id, _) = r.editor.mesh().curves().next().unwrap();
        assert_eq!(r.editor.mesh().curve_edges(id).len(), 20);
        assert!(r.editor.mesh().curve_edges(id).iter().any(|id| r.editor.mesh().edge(*id).unwrap().v.contains(&v)));
    }
    #[test]
    fn model_changes_and_tool_changes_cancel_uncommitted_geometry() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        r.click(DVec3::ZERO);
        r.click(DVec3::new(1., 1., 0.));
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(10.), DVec3::splat(11.)]], facing: None })
            .unwrap();
        let before = r.editor.doc().clone();
        assert!(r.type_vcb("-180").is_err());
        assert_eq!(r.editor.doc(), &before);
        assert!(!r.tools.busy());
        r.click(DVec3::ZERO);
        r.tools.set_tool(ToolKind::Select);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn changing_context_or_editing_after_creation_blocks_stale_operations() {
        let mut r = Rig::new(ToolKind::ThreePointArc);
        let made = r
            .editor
            .apply(Command::DrawRectangle { corner: DVec3::X * 10., u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let group = r.editor.apply(Command::MakeGroup { elements: made.created }).unwrap();
        let Element::Instance(id) = group.created[0] else { panic!("Expected group") };
        r.click(DVec3::ZERO);
        r.click(DVec3::new(1., 1., 0.));
        r.editor.enter(id).unwrap();
        let before = r.editor.doc().clone();
        r.hover(DVec3::X * 2.);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.doc(), &before);
        r.editor.exit();
        r.click(DVec3::ZERO);
        r.click(DVec3::new(1., 1., 0.));
        r.click(DVec3::X * 2.);
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(20.), DVec3::splat(21.)]], facing: None })
            .unwrap();
        let before = r.editor.doc().clone();
        assert!(r.type_vcb("24s").is_err());
        assert_eq!(r.editor.doc(), &before);
    }
}
