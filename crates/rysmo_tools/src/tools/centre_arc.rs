//! Centre/radius/sweep drawing, shared by Center Arc and Pie.
use super::*;
use rysmo_model::command::centre_arc_points;

impl ToolBox {
    pub(super) fn centre_stale(&mut self, editor: &Editor) -> bool {
        let stale = match &self.state {
            State::CentreRadius { revision, context, .. } | State::CentreSweep { revision, context, .. } => {
                *revision != editor.revision() || context != editor.context()
            }
            _ => false,
        };
        if stale {
            self.reset();
            self.last = Last::None;
            self.message = Some("The model or editing context changed; start a new arc.".into());
        }
        stale
    }
    pub(super) fn centre_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        if let State::CentreSweep { centre, .. } = self.state {
            if inf.point.distance(centre) <= EPS {
                self.message = Some("Select an angle point away from the centre.".into());
                return;
            }
        }
        if matches!(self.state, State::CentreSweep { .. }) {
            self.centre_move(editor, ctx);
        }
        match self.state.clone() {
            State::Idle => {
                let plane = self
                    .axis_lock
                    .map(|a| Plane::new(self.axes.dir(a), inf.point))
                    .unwrap_or_else(|| super::draw::drawing_plane(&inf, editor.mesh(), self.axes));
                self.state = State::CentreRadius {
                    centre: inf.point,
                    plane: Plane::new(plane.normal, inf.point),
                    revision: editor.revision(),
                    context: editor.context().to_vec(),
                };
                self.last = Last::None;
            }
            State::CentreRadius { centre, plane, revision, context } => {
                let d = plane.project(inf.point) - centre;
                if d.length() <= EPS || !d.length().is_finite() {
                    self.message = Some("Choose a nonzero radius.".into());
                    return;
                }
                self.state = State::CentreSweep {
                    centre,
                    plane,
                    start: d.normalize(),
                    radius: d.length(),
                    sweep: 0.,
                    revision,
                    context,
                };
                self.shift_lock = None;
                self.axis_lock = None;
            }
            State::CentreSweep { centre, plane, start, radius, sweep, .. } => {
                if let Err(error) = self.finish_centre(editor, centre, plane.normal, start, radius, sweep) {
                    self.message = Some(error);
                }
            }
            _ => {}
        }
    }
    pub(super) fn centre_move(&mut self, editor: &Editor, ctx: &Ctx) {
        let Some(inf) = self.inference else { return };
        if let State::CentreSweep { centre, plane, start, radius, sweep, .. } = &mut self.state {
            let d = plane.project(inf.point) - *centre;
            if d.length() <= EPS {
                return;
            }
            let side = plane.normal.cross(*start);
            let mut angle = d.dot(side).atan2(d.dot(*start)).to_degrees();
            while angle - *sweep > 180. {
                angle -= 360.;
            }
            while angle - *sweep < -180. {
                angle += 360.;
            }
            if inf.guide.is_none() {
                let tolerance = (ctx.view.pixel_size_at(*centre) * 8. * ctx.scale / *radius).atan().to_degrees();
                angle = editor.doc().measurement.snap_angle(angle, tolerance);
            }
            *sweep = angle.clamp(-359.9, 359.9);
        }
    }
    fn finish_centre(
        &mut self,
        editor: &mut Editor,
        centre: DVec3,
        normal: DVec3,
        start: DVec3,
        radius: f64,
        sweep: f64,
    ) -> Result<(), String> {
        let pie = self.active == ToolKind::Pie;
        let segments = self.arc_segments;
        let cmd = Command::DrawCentreArc { centre, normal, start, radius, sweep, segments, pie };
        let result =
            self.run(editor, cmd).ok_or_else(|| self.message.clone().unwrap_or_else(|| "Cannot create arc.".into()))?;
        self.message = None;
        self.reset();
        self.last = if result.changed {
            Last::CentreArc { centre, normal, start, radius, sweep, segments, pie, revision: editor.revision() }
        } else {
            Last::None
        };
        Ok(())
    }
    pub(super) fn centre_vcb(&mut self, editor: &mut Editor, _ctx: &Ctx, text: &str) -> Result<(), String> {
        if self.centre_stale(editor) {
            return Err("The model changed; start a new arc.".into());
        }
        let text = text.trim().to_ascii_lowercase();
        if let Some(n) = text.strip_suffix('c') {
            let count = n.trim().parse::<u32>().map_err(|_| "Enter a circle segment count such as 24c.")?;
            if !(3..=4096).contains(&count) {
                return Err("Use 3-4096 circle segments.".into());
            }
            let Last::CentreArc { sweep, .. } = self.last else {
                return Err("Draw the arc before converting circle-based segments.".into());
            };
            if self.state != State::Idle {
                return Err("Finish the arc before converting circle-based segments.".into());
            }
            let segments = (count as f64 * sweep.abs() / 360.).ceil().max(2.) as u32;
            self.correct_centre(editor, None, None, Some(segments))?;
            self.arc_segments = segments;
            self.message = None;
            self.update_status();
            return Ok(());
        }
        if let Some(n) = text.strip_suffix('s') {
            let segments = n.trim().parse::<u32>().map_err(|_| "Enter a segment count such as 24s.")?;
            if !(2..=4096).contains(&segments) {
                return Err("Use 2-4096 arc segments.".into());
            }
            if self.state == State::Idle && matches!(self.last, Last::CentreArc { .. }) {
                self.correct_centre(editor, None, None, Some(segments))?;
            }
            self.arc_segments = segments;
            self.message = None;
            self.update_status();
            return Ok(());
        }
        if self.state == State::Idle {
            if text.ends_with('r') {
                let radius = rysmo_model::parse_length(text.trim_end_matches('r'), editor.doc().units)
                    .map_err(|e| e.to_string())?;
                self.correct_centre(editor, Some(radius), None, None)?;
            } else {
                self.correct_centre(editor, None, Some(parse_angle(&text)?), None)?;
            }
            return Ok(());
        }
        match self.state.clone() {
            State::CentreRadius { centre, plane, revision, context } => {
                let radius = rysmo_model::parse_length(text.trim_end_matches('r'), editor.doc().units)
                    .map_err(|e| e.to_string())?;
                if !radius.is_finite() || radius <= EPS || !(radius * radius).is_finite() {
                    return Err("Enter a positive finite radius.".into());
                }
                let start = self
                    .inference
                    .map(|i| plane.project(i.point) - centre)
                    .and_then(|d| d.try_normalize())
                    .ok_or("Move the pointer to choose the start direction.")?;
                self.state = State::CentreSweep { centre, plane, start, radius, sweep: 0., revision, context };
                self.axis_lock = None;
                self.shift_lock = None;
            }
            State::CentreSweep { centre, plane, start, radius, .. } => {
                if let Some(r) = text.strip_suffix('r') {
                    let radius = rysmo_model::parse_length(r, editor.doc().units).map_err(|e| e.to_string())?;
                    if !radius.is_finite() || radius <= EPS || !(radius * radius).is_finite() {
                        return Err("Enter a positive finite radius.".into());
                    }
                    if let State::CentreSweep { radius: r, .. } = &mut self.state {
                        *r = radius;
                    }
                } else {
                    self.finish_centre(editor, centre, plane.normal, start, radius, parse_angle(&text)?)?;
                }
            }
            _ => return Err("Select the centre first.".into()),
        }
        self.message = None;
        self.update_status();
        Ok(())
    }
    fn correct_centre(
        &mut self,
        editor: &mut Editor,
        r: Option<f64>,
        a: Option<f64>,
        s: Option<u32>,
    ) -> Result<(), String> {
        let Last::CentreArc { centre, normal, start, radius, sweep, segments, pie, revision } = self.last else {
            return Err("Select the centre first.".into());
        };
        if revision != editor.revision() {
            return Err("The model changed; draw a new arc.".into());
        }
        let radius = r.unwrap_or(radius);
        let sweep = a.unwrap_or(sweep);
        let segments = s.unwrap_or(segments);
        self.correct_last(editor, Command::DrawCentreArc { centre, normal, start, radius, sweep, segments, pie })?;
        self.last =
            Last::CentreArc { centre, normal, start, radius, sweep, segments, pie, revision: editor.revision() };
        self.message = None;
        self.update_status();
        Ok(())
    }
    pub(super) fn centre_preview(&self, editor: &Editor, pv: &mut Preview) {
        if let State::CentreSweep { centre, plane, start, radius, sweep, .. } = self.state {
            let end = centre + rysmo_math::DQuat::from_axis_angle(plane.normal, sweep.to_radians()) * start * radius;
            let mut line = |a, b, dashed| {
                pv.lines.push(PreviewLine { a, b, color: if dashed { MAGENTA } else { BLACK }, dashed, thick: false })
            };
            line(centre, centre + start * radius, self.active != ToolKind::Pie);
            line(centre, end, self.active != ToolKind::Pie);
            if let Some(points) = centre_arc_points(centre, plane.normal, start, radius, sweep, self.arc_segments) {
                for p in points.windows(2) {
                    line(p[0], p[1], false);
                }
            }
            pv.label = Some(editor.doc().measurement.format_angle(sweep));
        }
    }
}
fn parse_angle(text: &str) -> Result<f64, String> {
    let angle = text
        .strip_suffix("deg")
        .or_else(|| text.strip_suffix('°'))
        .unwrap_or(text)
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .map_err(|_| "Enter signed degrees, such as 90 or -45deg.")?;
    if !angle.is_finite() || angle.abs() < 1e-6 || angle.abs() >= 360. {
        return Err("Use a nonzero sweep below 360 degrees; use Circle for a full turn.".into());
    }
    Ok(angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Rig;
    #[test]
    fn numeric_radius_signed_sweep_and_corrections_share_one_undo() {
        let mut r = Rig::new(ToolKind::CenterArc);
        let doc = r.editor.doc().clone();
        r.click(DVec3::ZERO);
        r.hover(DVec3::X);
        r.type_vcb("1m").unwrap();
        r.type_vcb("270deg").unwrap();
        assert!(!r.tools.busy());
        assert_eq!(r.editor.undo_depth(), 1);
        r.type_vcb("-90").unwrap();
        r.type_vcb("2mr").unwrap();
        r.type_vcb("32s").unwrap();
        let (_, curve) = r.editor.mesh().curves().next().unwrap();
        assert_eq!((curve.radius, curve.sweep, curve.segments), (2., -90., 32));
        assert_eq!(r.editor.undo_depth(), 1);
        r.editor.undo();
        assert_eq!(r.editor.doc(), &doc);
    }
    #[test]
    fn pointer_sweep_can_cross_180_and_pie_closes_a_face() {
        let mut r = Rig::new(ToolKind::Pie);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.hover(DVec3::Y * 2.);
        r.hover(-DVec3::X * 2.);
        r.click(-DVec3::Y * 2.);
        assert_eq!(r.editor.mesh().face_count(), 1);
        assert!((r.editor.mesh().curves().next().unwrap().1.sweep - 270.).abs() < 1e-6);
        assert_eq!(r.editor.undo_depth(), 1);
    }
    #[test]
    fn plane_lock_radius_changes_and_segments_do_not_commit_until_the_angle() {
        let mut r = Rig::new(ToolKind::Pie);
        r.key(Key::Left);
        r.click(DVec3::ZERO);
        r.hover(DVec3::X);
        r.type_vcb("1m").unwrap();
        r.type_vcb("500mmr").unwrap();
        r.type_vcb("12s").unwrap();
        assert_eq!(r.editor.mesh().face_count(), 0);
        r.type_vcb("90").unwrap();
        assert!(r.editor.mesh().vertices().all(|(_, v)| v.pos.y.abs() < 1e-8));
        let (_, curve) = r.editor.mesh().curves().next().unwrap();
        assert!((curve.radius - 0.5).abs() < 1e-8);
        assert_eq!(curve.segments, 12);
    }
    #[test]
    fn invalid_values_escape_and_external_edits_preserve_geometry() {
        let mut r = Rig::new(ToolKind::CenterArc);
        r.click(DVec3::ZERO);
        r.click(DVec3::X);
        for value in ["0", "360", "NaN", "-2mr", "5000s"] {
            assert!(r.type_vcb(value).is_err(), "{value}");
            assert!(r.tools.busy());
        }
        r.key(Key::Escape);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.mesh().edge_count(), 0);
        r.click(DVec3::ZERO);
        r.click(DVec3::X);
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(10.), DVec3::splat(11.)]], facing: None })
            .unwrap();
        let before = r.editor.doc().clone();
        assert!(r.type_vcb("90").is_err());
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn circle_based_segments_and_later_edits_keep_corrections_guarded() {
        let mut r = Rig::new(ToolKind::Pie);
        r.click(DVec3::ZERO);
        r.hover(DVec3::X);
        r.type_vcb("1m").unwrap();
        r.type_vcb("90").unwrap();
        r.type_vcb("20c").unwrap();
        assert_eq!(r.editor.mesh().curves().next().unwrap().1.segments, 5);
        assert_eq!(r.editor.undo_depth(), 1);
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(10.), DVec3::splat(11.)]], facing: None })
            .unwrap();
        let before = r.editor.doc().clone();
        assert!(r.type_vcb("30c").is_err());
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn exact_radius_endpoints_override_optional_length_snapping() {
        let mut r = Rig::new(ToolKind::CenterArc);
        let point = DVec3::new(1.13, 0., 0.);
        r.editor.apply(Command::DrawLines { lines: vec![[point, point + DVec3::Y]], facing: None }).unwrap();
        let mut settings = r.editor.doc().measurement;
        settings.length_snap = Some(0.5);
        r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
        r.click(DVec3::ZERO);
        r.click(point);
        r.type_vcb("90").unwrap();
        assert!((r.editor.mesh().curves().next().unwrap().1.radius - 1.13).abs() < 1e-8);
    }
}
