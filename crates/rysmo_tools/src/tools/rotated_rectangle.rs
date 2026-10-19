//! Three-point rectangles with a freely tilted width plane and exact dimensions.
use super::*;

pub(super) fn width_vector(corner: DVec3, u: DVec3, point: DVec3) -> DVec3 {
    let d = point - corner;
    d - u * (d.dot(u) / u.length_squared())
}

impl ToolBox {
    pub(super) fn rotated_stale(&mut self, editor: &Editor) -> bool {
        let stale = match &self.state {
            State::RotatedEdge { revision, context, .. } | State::RotatedWidth { revision, context, .. } => {
                *revision != editor.revision() || context != editor.context()
            }
            _ => false,
        };
        if stale {
            self.reset();
            self.last = Last::None;
            self.message = Some("The model or editing context changed; start a new rectangle.".into());
        }
        stale
    }

    pub(super) fn rotated_lock(&self) -> Option<Lock> {
        match self.state {
            State::RotatedEdge { corner, plane, .. } => {
                let direction = match self.shift_lock {
                    Some(Lock::Line { dir, .. }) => Some(dir),
                    _ => self.axis_lock.map(|a| self.axes.dir(a)),
                };
                if let Some(dir) = direction {
                    let dir = plane.map_or(dir, |p| dir - p.normal * dir.dot(p.normal));
                    if let Some(dir) = dir.try_normalize() {
                        return Some(Lock::Line { origin: corner, dir, axis: None });
                    }
                }
                plane.map(Lock::Plane)
            }
            State::RotatedWidth { corner, u, plane, angle_lock, .. } => {
                let direction = angle_lock.or_else(|| match self.shift_lock {
                    Some(Lock::Line { dir, .. }) => Some(dir),
                    _ => self.axis_lock.map(|a| self.axes.dir(a)),
                });
                let direction = if let Some(plane) = plane {
                    let side = plane.normal.cross(u);
                    Some(if direction.is_some_and(|d| d.dot(side) < 0.) { -side } else { side })
                } else {
                    direction
                };
                if let Some(dir) = direction.and_then(|d| width_vector(DVec3::ZERO, u, d).try_normalize()) {
                    Some(Lock::Line { origin: corner, dir, axis: None })
                } else {
                    Some(Lock::Plane(Plane::new(u.normalize(), corner)))
                }
            }
            _ => None,
        }
    }

    pub(super) fn rotated_snap(&self, editor: &Editor, ctx: &Ctx, mut inf: Inference) -> Inference {
        if let State::RotatedWidth { corner, u, baseline, plane: None, angle_lock: None, .. } = self.state
            && self.shift_lock.is_none()
            && self.axis_lock.is_none()
            && inf.guide.is_none()
        {
            let v = width_vector(corner, u, inf.point);
            if v.length() > EPS {
                let side = u.normalize().cross(baseline);
                let angle = v.dot(side).atan2(v.dot(baseline)).to_degrees();
                let tolerance = (ctx.view.pixel_size_at(corner) * 8. * ctx.scale / v.length()).atan().to_degrees();
                let snapped = editor.doc().measurement.snap_angle(angle, tolerance);
                if (snapped - angle).abs() > 1e-10 {
                    let dir = baseline * snapped.to_radians().cos() + side * snapped.to_radians().sin();
                    inf.point = corner + dir * v.length();
                }
            }
        }
        inf
    }

    pub(super) fn rotated_click(&mut self, editor: &mut Editor, ctx: &Ctx, inf: Inference) {
        match self.state.clone() {
            State::Idle => {
                let plane = self.axis_lock.map(|a| Plane::new(self.axes.dir(a), inf.point));
                self.state = State::RotatedEdge {
                    corner: inf.point,
                    plane,
                    revision: editor.revision(),
                    context: editor.context().to_vec(),
                };
                self.axis_lock = None;
                self.shift_lock = None;
                self.last = Last::None;
            }
            State::RotatedEdge { corner, plane, revision, context } => {
                if ctx.modifiers.alt {
                    let normal = super::draw::drawing_plane(&inf, editor.mesh(), self.axes).normal;
                    if let State::RotatedEdge { plane, .. } = &mut self.state {
                        *plane = Some(Plane::new(normal, corner));
                    }
                    self.axis_lock = None;
                    self.shift_lock = None;
                    self.message =
                        Some("Drawing plane locked. Select the first edge endpoint; Down clears the plane.".into());
                    return;
                }
                let p = plane.map_or(inf.point, |p| p.project(inf.point));
                let u = p - corner;
                if !u.length().is_finite() || u.length() <= EPS {
                    self.message = Some("Choose a nonzero first edge.".into());
                    return;
                }
                let normal = plane.map(|p| p.normal).or(inf.plane.map(|p| p.normal)).unwrap_or(self.axes.z());
                let baseline = normal.cross(u).try_normalize().unwrap_or_else(|| {
                    [self.axes.x, self.axes.y, self.axes.z()]
                        .into_iter()
                        .min_by(|a, b| a.dot(u).abs().total_cmp(&b.dot(u).abs()))
                        .unwrap()
                        .cross(u)
                        .normalize()
                });
                self.state = State::RotatedWidth { corner, u, baseline, plane, angle_lock: None, revision, context };
                self.axis_lock = None;
                self.shift_lock = None;
                // Re-sample the new perpendicular plane on the next pointer event.
                self.inference = None;
            }
            State::RotatedWidth { corner, u, .. } => {
                let v = width_vector(corner, u, inf.point);
                if ctx.modifiers.alt {
                    if let Some(dir) = v.try_normalize() {
                        if let State::RotatedWidth { baseline, angle_lock, .. } = &mut self.state {
                            *baseline = dir;
                            *angle_lock = None;
                        }
                        self.shift_lock = None;
                        self.axis_lock = None;
                        self.message = Some("Angle baseline set. Move to set the width or type an angle.".into());
                    }
                } else {
                    if let Err(error) = self.finish_rotated(editor, ctx, corner, u, v) {
                        self.message = Some(error);
                    }
                }
            }
            _ => {}
        }
    }

    fn finish_rotated(
        &mut self,
        editor: &mut Editor,
        ctx: &Ctx,
        corner: DVec3,
        u: DVec3,
        v: DVec3,
    ) -> Result<(), String> {
        if !u.is_finite()
            || !v.is_finite()
            || !u.length().is_finite()
            || !v.length().is_finite()
            || u.length() <= EPS
            || v.length() <= EPS
        {
            return Err("Enter positive, finite rectangle dimensions.".into());
        }
        let normal = u.cross(v).try_normalize().ok_or("The rectangle dimensions are too large or degenerate.")?;
        let facing = if rysmo_math::parallel(normal, DVec3::Z) { None } else { Some(Self::facing(ctx)) };
        let result = self
            .run(editor, Command::DrawRectangle { corner, u, v, facing })
            .ok_or_else(|| self.message.clone().unwrap_or_else(|| "Cannot create rectangle.".into()))?;
        self.message = None;
        self.reset();
        self.last = if result.changed {
            Last::RotatedRectangle { corner, u, v, facing, revision: editor.revision() }
        } else {
            Last::None
        };
        Ok(())
    }

    pub(super) fn rotated_vcb(&mut self, editor: &mut Editor, ctx: &Ctx, text: &str) -> Result<(), String> {
        if self.rotated_stale(editor) {
            return Err("The model changed; start a new rectangle.".into());
        }
        let text = text.trim();
        if let Some(angle) = text.strip_suffix("deg").or_else(|| text.strip_suffix('°')) {
            let angle = angle.trim().replace(',', ".").parse::<f64>().map_err(|_| "Enter an angle such as 30deg.")?;
            if !angle.is_finite() {
                return Err("Enter a finite angle.".into());
            }
            let State::RotatedWidth { corner, u, baseline, plane, angle_lock, .. } = &mut self.state else {
                return Err("Choose the first edge before entering a tilt angle.".into());
            };
            if plane.is_some() {
                return Err("Press Down to clear the plane constraint before entering a tilt.".into());
            }
            let dir =
                rysmo_math::DQuat::from_axis_angle(u.normalize(), angle.rem_euclid(360.).to_radians()) * *baseline;
            *angle_lock = Some(dir);
            let width = self.inference.map(|i| width_vector(*corner, *u, i.point).length()).unwrap_or(0.);
            if let Some(i) = &mut self.inference {
                i.point = *corner + dir * width;
            }
            self.message = Some(format!("Tilt set to {angle}°. Enter the width."));
            return Ok(());
        }
        let wanted = if matches!(self.state, State::RotatedEdge { .. }) { 1 } else { 2 };
        let VcbInput::Lengths(values) = parse_vcb(text, editor.doc().units, wanted)? else {
            return Err("Enter rectangle dimensions.".into());
        };
        if values.len() > wanted {
            return Err("Too many rectangle dimensions.".into());
        }
        if values.iter().any(|v| !v.is_finite() || *v <= EPS || !(v * v).is_finite()) {
            return Err("Enter positive rectangle dimensions.".into());
        }
        match self.state.clone() {
            State::RotatedEdge { corner, plane, .. } => {
                let len = values.first().copied().ok_or("Enter the first edge length.")?;
                let dir = self.current_dir(corner).ok_or("Move the pointer to choose the first edge direction.")?;
                let mut inf = self.inference.ok_or("Choose a direction first.")?;
                inf.point = corner + dir * len;
                if let Some(plane) = plane {
                    inf.point = plane.project(inf.point);
                }
                self.rotated_click(editor, ctx, inf);
            }
            State::RotatedWidth { corner, u, baseline, angle_lock, .. } => {
                let current = self.inference.map(|i| width_vector(corner, u, i.point)).unwrap_or(DVec3::ZERO);
                let dir = angle_lock.or_else(|| current.try_normalize()).unwrap_or(baseline);
                let (length, width) = if values.len() == 2 {
                    (values[0], values[1])
                } else {
                    (u.length(), *values.first().ok_or("Enter a width.")?)
                };
                self.finish_rotated(editor, ctx, corner, u.normalize() * length, dir * width)?;
            }
            State::Idle => {
                let Last::RotatedRectangle { corner, u, v, facing, revision } = self.last else {
                    return Err("Select the first corner.".into());
                };
                if revision != editor.revision() {
                    return Err("The model changed; draw a new rectangle.".into());
                }
                if values.len() != 2 {
                    return Err("Enter length,width to correct both dimensions.".into());
                }
                let u = u.normalize() * values[0];
                let v = v.normalize() * values[1];
                self.correct_last(editor, Command::DrawRectangle { corner, u, v, facing })?;
                self.last = Last::RotatedRectangle { corner, u, v, facing, revision: editor.revision() };
            }
            _ => return Err("Select the first corner.".into()),
        }
        self.message = None;
        self.update_status();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Rig;
    fn no_angle_snap(r: &mut Rig) {
        let mut settings = r.editor.doc().measurement;
        settings.angle_snap = None;
        r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
    }
    fn area(r: &Rig) -> f64 {
        r.editor.mesh().faces().map(|(id, _)| r.editor.mesh().face_area(id)).sum()
    }
    #[test]
    fn three_points_create_a_tilted_rectangle_and_corrections_share_one_undo() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        no_angle_snap(&mut r);
        let before = r.editor.doc().clone();
        let depth = r.editor.undo_depth();
        let u = DVec3::new(2., 1., 0.);
        let v = DVec3::new(-1., 2., 2.);
        r.click(DVec3::ZERO);
        r.click(u);
        r.hover(v);
        assert!(r.tools.busy());
        assert_eq!(r.editor.doc(), &before);
        assert!(r.tools.preview(&r.editor).lines.len() >= 4);
        r.click(v);
        assert!(!r.tools.busy());
        assert!((area(&r) - u.length() * v.length()).abs() < 1e-6);
        assert!(r.editor.mesh().find_vertex(u + v).is_some());
        assert_eq!(r.editor.undo_depth(), depth + 1);
        r.type_vcb("4000,2000").unwrap();
        assert!((area(&r) - 8.).abs() < 1e-7);
        assert_eq!(r.editor.undo_depth(), depth + 1);
        r.editor.undo();
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn exact_first_edge_tilt_and_decimal_comma_dimensions() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.click(DVec3::ZERO);
        r.hover(DVec3::new(2., 1., 0.));
        r.type_vcb("2.5m").unwrap();
        r.type_vcb("30deg").unwrap();
        r.type_vcb("2,5m;1,25m").unwrap();
        assert!((area(&r) - 3.125).abs() < 1e-8);
        let z = r.editor.mesh().vertices().map(|(_, v)| v.pos.z).fold(0., f64::max);
        assert!((z - 0.625).abs() < 1e-8);
    }
    #[test]
    fn escape_tool_change_and_stale_model_leave_no_rectangle() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.key(Key::Escape);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.mesh().face_count(), 0);
        r.click(DVec3::ZERO);
        r.tools.set_tool(ToolKind::Line);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.mesh().face_count(), 0);
        r.tools.set_tool(ToolKind::RotatedRectangle);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(10.), DVec3::splat(11.)]], facing: None })
            .unwrap();
        let doc = r.editor.doc().clone();
        assert!(r.type_vcb("1m").is_err());
        assert!(!r.tools.busy());
        assert_eq!(r.editor.doc(), &doc);
    }
    #[test]
    fn invalid_numeric_values_keep_the_draft_and_model_unchanged() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.hover(DVec3::Y);
        let doc = r.editor.doc().clone();
        for value in ["0", "-1m", "NaN", "1m,2m,3m", "infdeg", "1e300m"] {
            assert!(r.type_vcb(value).is_err(), "{value}");
            assert!(r.tools.busy());
            assert_eq!(r.editor.doc(), &doc);
        }
        r.type_vcb("1m").unwrap();
        assert!((area(&r) - 2.).abs() < 1e-8);
    }
    #[test]
    fn preselected_plane_is_respected_and_down_releases_it_for_tilt() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.key(Key::Up);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        assert!(r.type_vcb("45deg").is_err());
        r.key(Key::Down);
        r.type_vcb("45deg").unwrap();
        r.type_vcb("1m").unwrap();
        assert!((area(&r) - 2.).abs() < 1e-8);
        assert!(r.editor.mesh().vertices().any(|(_, v)| v.pos.z > 0.7));
    }
    #[test]
    fn alt_click_sets_baseline_without_committing_and_shift_holds_width_angle() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        no_angle_snap(&mut r);
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.hover(DVec3::new(0., 1., 1.));
        r.key(Key::Shift(true));
        r.hover(DVec3::new(0., 3., 0.));
        let p = r.tools.preview(&r.editor).inference.unwrap().point;
        assert!((p.y - p.z).abs() < 1e-7);
        r.key(Key::Shift(false));
        r.mods.alt = true;
        r.click(DVec3::new(0., 1., 1.));
        r.mods.alt = false;
        assert!(r.tools.busy());
        assert_eq!(r.editor.mesh().face_count(), 0);
        r.type_vcb("90deg").unwrap();
        r.type_vcb("1m").unwrap();
        assert!((area(&r) - 2.).abs() < 1e-8);
        assert!(r.editor.mesh().vertices().any(|(_, v)| v.pos.y < -0.7 && v.pos.z > 0.7));
    }
    #[test]
    fn exact_endpoint_projection_takes_priority_over_length_and_angle_snapping() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        let point = DVec3::new(0., 1.13, 0.37);
        r.editor.apply(Command::DrawLines { lines: vec![[point, point + DVec3::Z]], facing: None }).unwrap();
        let mut settings = r.editor.doc().measurement;
        settings.length_snap = Some(0.5);
        r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.click(point);
        assert!(r.editor.mesh().find_vertex(point + DVec3::X * 2.).is_some());
        assert!((area(&r) - point.length() * 2.).abs() < 1e-7);
    }
    #[test]
    fn changed_context_cancels_the_draft_and_later_edits_block_corrections() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        let made = r
            .editor
            .apply(Command::DrawRectangle { corner: DVec3::X * 10., u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let group = r.editor.apply(Command::MakeGroup { elements: made.created }).unwrap();
        let Element::Instance(id) = group.created[0] else { panic!("Expected group") };
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.editor.enter(id).unwrap();
        let before = r.editor.doc().clone();
        r.hover(DVec3::Y);
        assert!(!r.tools.busy());
        assert_eq!(r.editor.doc(), &before);
        r.editor.exit();
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.type_vcb("1m").unwrap();
        r.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(20.), DVec3::splat(21.)]], facing: None })
            .unwrap();
        let before = r.editor.doc().clone();
        assert!(r.type_vcb("3m,2m").is_err());
        assert_eq!(r.editor.doc(), &before);
    }
    #[test]
    fn looking_edge_on_at_the_width_plane_does_not_use_the_camera_position() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.tools.state = State::RotatedWidth {
            corner: DVec3::ZERO,
            u: DVec3::X,
            baseline: DVec3::Y,
            plane: None,
            angle_lock: None,
            revision: r.editor.revision(),
            context: vec![],
        };
        let camera = rysmo_math::OrbitCamera {
            target: DVec3::ZERO,
            parallel: true,
            yaw: -std::f64::consts::FRAC_PI_2,
            pitch: 0.,
            ..Default::default()
        };
        r.view = camera.view(rysmo_math::DVec2::new(1000., 800.));
        r.hover(DVec3::Y);
        assert!(r.tools.preview(&r.editor).inference.is_none());
        assert_eq!(r.editor.mesh().face_count(), 0);
    }
    #[test]
    fn alt_click_can_lock_the_first_edge_plane_without_advancing_the_draft() {
        let mut r = Rig::new(ToolKind::RotatedRectangle);
        r.click(DVec3::ZERO);
        r.mods.alt = true;
        r.click(DVec3::X);
        r.mods.alt = false;
        assert!(matches!(r.tools.state, State::RotatedEdge { plane: Some(_), .. }));
        r.click(DVec3::X * 2.);
        r.click(DVec3::new(0., 1., 2.));
        assert!(!r.tools.busy());
        assert!(r.editor.mesh().vertices().all(|(_, v)| v.pos.z.abs() < 1e-8));
    }
}
