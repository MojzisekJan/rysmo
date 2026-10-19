//! Non-destructive mirror setup with a live wire preview.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DMat4, DVec2, DVec3, Plane, Ray};
use rysmo_model::{Command, Element, InstanceId, clipboard::Clipboard};

pub struct Draft {
    snapshot: Clipboard,
    elements: Vec<Element>,
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
    basis: DMat4,
    centre: DVec3,
    radius: f64,
    axis: usize,
    offset: f64,
    copy: bool,
    object_axes: bool,
    drag: Option<(f64, f64)>,
    custom: Option<Frame>,
    pick_face: bool,
    candidate: Option<Frame>,
}
/// An orthonormal frame in editing-context coordinates; blue is its normal.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: DVec3,
    basis: DMat4,
}
impl Frame {
    fn from_surface(point: DVec3, normal: DVec3, centre: DVec3) -> Option<Self> {
        if !point.is_finite() || !normal.is_finite() || normal.length_squared() < 1e-20 {
            return None;
        }
        let normal = normal.normalize();
        let seed = [DVec3::X, DVec3::Y, DVec3::Z]
            .into_iter()
            .min_by(|a, b| a.dot(normal).abs().total_cmp(&b.dot(normal).abs()))?;
        let x = (seed - normal * seed.dot(normal)).normalize();
        let axes = rysmo_math::DrawingAxes::new(DVec3::ZERO, x, normal.cross(x))?;
        Some(Self { origin: Plane::new(normal, point).project(centre), basis: axes.basis() })
    }
    fn normal(self, axis: usize) -> DVec3 {
        self.basis.col(axis).truncate()
    }
    fn corners(self, axis: usize, radius: f64) -> [DVec3; 4] {
        let u = self.normal((axis + 1) % 3) * radius;
        let v = self.normal((axis + 2) % 3) * radius;
        [self.origin - u - v, self.origin + u - v, self.origin + u + v, self.origin - u + v]
    }
    fn hit(self, axis: usize, radius: f64, ray: Ray) -> Option<f64> {
        let distance = Plane::new(self.normal(axis), self.origin).intersect_ray(&ray)?;
        let delta = ray.at(distance) - self.origin;
        let u = self.normal((axis + 1) % 3);
        let v = self.normal((axis + 2) % 3);
        (delta.dot(u).abs() <= radius && delta.dot(v).abs() <= radius).then_some(distance)
    }
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.generation == app.document_generation
            && self.revision == app.editor.revision()
            && self.context == app.editor.context()
            && self.elements == app.editor.selected()
    }
    pub fn status(&self) -> &'static str {
        if self.pick_face {
            "Flip: hover a visible model face, then click or drag. Escape returns to the previous plane."
        } else {
            "Flip: choose or drag a plane; arrows choose an axis. Confirm in the Flip window; Escape cancels."
        }
    }
    fn axes(&self, app: &App3d) -> DMat4 {
        if self.object_axes { self.basis } else { app.editor.active().axes.basis() }
    }
    fn frame(&self, app: &App3d) -> Frame {
        self.custom.unwrap_or(Frame { origin: self.centre, basis: self.axes(app) })
    }
    fn plane(&self, app: &App3d) -> Frame {
        let mut frame = self.frame(app);
        frame.origin += frame.normal(self.axis) * self.offset;
        frame
    }
    fn choose_axis(&mut self, axis: usize) {
        if self.custom.is_some() || self.axis != axis {
            self.offset = 0.;
        }
        self.axis = axis;
        self.custom = None;
        self.pick_face = false;
        self.candidate = None;
        self.drag = None;
    }
    fn set_custom(&mut self, frame: Frame) {
        self.custom = Some(frame);
        self.axis = 2;
        self.offset = 0.;
        self.pick_face = false;
        self.candidate = None;
        self.drag = None;
    }
    fn transform(&self, app: &App3d) -> (DMat4, DVec3, DMat4, DVec3) {
        let frame = self.plane(app);
        let basis = frame.basis;
        let origin = frame.origin;
        let mut factors = DVec3::ONE;
        factors[self.axis] = -1.;
        let reflection = DMat4::from_translation(origin)
            * basis
            * DMat4::from_scale(factors)
            * basis.inverse()
            * DMat4::from_translation(-origin);
        (reflection, origin, basis, factors)
    }
}
pub fn begin(app: &mut App3d) {
    let elements = app.editor.selected();
    let snapshot = match Clipboard::capture(&app.editor, elements.clone()) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            app.message = Some(error.to_string());
            return;
        }
    };
    let Some((lo, hi)) = snapshot.bounds() else {
        app.message = Some("Select objects with 3D bounds to flip.".into());
        return;
    };
    let source = app.editor.context_transform().inverse() * snapshot.source_transform;
    let centre = source.transform_point3((lo + hi) * 0.5);
    let radius = (source.transform_vector3(hi - lo).length() * 0.7).max(0.1);
    let mut basis = app.editor.active().axes.basis();
    if let [Element::Instance(id)] = elements.as_slice() {
        let t = app.editor.active().instances[id].transform;
        if let Some(axes) = rysmo_math::DrawingAxes::new(DVec3::ZERO, t.x_axis.truncate(), t.y_axis.truncate()) {
            basis = axes.basis();
        }
    }
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.mirror = Some(Draft {
        snapshot,
        elements,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
        basis,
        centre,
        radius,
        axis: 0,
        offset: 0.,
        copy: false,
        object_axes: true,
        drag: None,
        custom: None,
        pick_face: false,
        candidate: None,
    });
    app.message = None;
}
fn apply(app: &mut App3d, draft: Draft) {
    if !draft.valid(app) || draft.pick_face {
        return;
    }
    let (reflection, origin, basis, factors) = draft.transform(app);
    let command = if draft.copy {
        let source = app.editor.context_transform().inverse() * draft.snapshot.source_transform;
        Command::Paste { clipboard: Box::new(draft.snapshot), transform: reflection * source, reuse_definitions: true }
    } else {
        Command::ScaleInAxes {
            elements: draft.elements,
            origin,
            factors,
            x: basis.x_axis.truncate(),
            y: basis.y_axis.truncate(),
        }
    };
    match app.editor.apply(command) {
        Ok(out) => {
            app.editor.set_selection(out.created);
            app.message = None;
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}
pub fn key(app: &mut App3d, key: rysmo_tools::Key) {
    use rysmo_tools::Key;
    let Some(draft) = app.mirror.as_mut() else {
        return;
    };
    match key {
        Key::Escape if draft.pick_face => {
            draft.pick_face = false;
            draft.candidate = None;
            draft.drag = None;
        }
        Key::Escape => app.mirror = None,
        Key::Right => draft.choose_axis(0),
        Key::Left => draft.choose_axis(1),
        Key::Up => draft.choose_axis(2),
        _ => {}
    }
}
// Closest point on the normal axis to the cursor ray; reject an end-on view.
fn axis_parameter(ray: rysmo_math::Ray, centre: DVec3, normal: DVec3) -> Option<f64> {
    let cosine = normal.dot(ray.dir);
    let denominator = 1. - cosine * cosine;
    let w = ray.origin - centre;
    (denominator > 1e-6).then(|| (normal.dot(w) - cosine * ray.dir.dot(w)) / denominator)
}
fn drag_parameter(view: &rysmo_math::View, at: DVec2, centre: DVec3, normal: DVec3) -> Option<f64> {
    // Off-centre perspective rays diverge even in an end-on view. Reject the
    // camera's forward direction too, so a surface click cannot start a frozen drag.
    let forward = view.ray(view.offset + view.size * 0.5).dir;
    if forward.dot(normal).abs() > 0.9999 {
        return None;
    }
    axis_parameter(view.ray(at), centre, normal)
}
fn face_frame(app: &App3d, at: DVec2, centre: DVec3) -> Option<Frame> {
    let hit = app.editor.doc().ray_surface(app.view().ray(at))?;
    let context = app.editor.context_transform();
    let point = context.inverse().transform_point3(hit.point);
    // A world-space plane normal is a covector, including under nonuniform scale.
    let normal = context.transpose().transform_vector3(hit.normal);
    Frame::from_surface(point, normal, centre)
}
pub fn pointer(app: &mut App3d, at: DVec2, event: rysmo_tools::Pointer) {
    use rysmo_tools::Pointer;
    let Some(mut draft) = app.mirror.take() else {
        return;
    };
    if !draft.valid(app) {
        return;
    }
    let view = app.local_view();
    if draft.pick_face {
        draft.candidate = face_frame(app, at, draft.centre);
        if matches!(event, Pointer::Down { .. }) {
            if let Some(frame) = draft.candidate {
                draft.set_custom(frame);
                let normal = frame.normal(2);
                draft.drag = drag_parameter(&view, at, frame.origin, normal).map(|p| (p, 0.));
            }
        }
    } else {
        let frame = draft.plane(app);
        let normal = frame.normal(draft.axis);
        let base = draft.frame(app).origin;
        match event {
            Pointer::Down { .. } => {
                let handle = view.project(frame.origin).is_some_and(|p| p.distance(at) <= 14. * app.pick_scale());
                if handle {
                    draft.drag = drag_parameter(&view, at, base, normal).map(|p| (p, draft.offset));
                } else if draft.custom.is_none() {
                    // All three planes are drawn. The nearest intersected square wins;
                    // retain the active plane on an exact tie at their intersection.
                    let ray = view.ray(at);
                    let chosen = (0..3)
                        .filter_map(|axis| {
                            let plane = if axis == draft.axis {
                                frame
                            } else {
                                Frame { origin: draft.centre, basis: draft.axes(app) }
                            };
                            plane.hit(axis, draft.radius, ray).map(|depth| (depth, axis))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| (a.1 != draft.axis).cmp(&(b.1 != draft.axis))));
                    if let Some((_, axis)) = chosen {
                        if axis != draft.axis {
                            draft.choose_axis(axis);
                        }
                        let plane = draft.plane(app);
                        draft.drag = drag_parameter(&view, at, draft.frame(app).origin, plane.normal(axis))
                            .map(|p| (p, draft.offset));
                    }
                } else if frame.hit(draft.axis, draft.radius, view.ray(at)).is_some() {
                    draft.drag = drag_parameter(&view, at, base, normal).map(|p| (p, draft.offset));
                }
            }
            Pointer::Move => {
                if let (Some((start, offset)), Some(current)) = (draft.drag, drag_parameter(&view, at, base, normal)) {
                    draft.offset = offset + current - start;
                }
            }
            Pointer::Up => draft.drag = None,
        }
    }
    app.mirror = Some(draft);
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.mirror.take() else {
        return;
    };
    if !draft.valid(app) {
        return;
    }
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    crate::theme::window("Flip").beside_model().open(&mut open).default_width(320.).show(ctx, |ui| {
        ui.label("Click a coloured plane to choose it; drag to move it. Arrow keys also choose an axis.");
        ui.horizontal(|ui| {
            for (i, label) in ["Red", "Green", "Blue"].into_iter().enumerate() {
                let selected = !draft.pick_face && draft.custom.is_none() && draft.axis == i;
                if ui.selectable_label(selected, label).clicked() {
                    draft.choose_axis(i);
                }
            }
        });
        ui.add_enabled_ui(draft.custom.is_none() && !draft.pick_face, |ui| {
            if crate::widgets::checkbox(ui, &mut draft.object_axes, "Use selection axes").changed() {
                draft.drag = None;
            }
        });
        ui.label("A single instance uses its own axes; other selections use drawing axes.");
        if ui.selectable_label(draft.pick_face, "Pick model face...").clicked() {
            draft.pick_face = !draft.pick_face;
            draft.candidate = None;
            draft.drag = None;
        }
        if draft.pick_face {
            ui.label("Hover a visible face, then click or drag. Escape returns to the previous plane.");
        } else if draft.custom.is_some() {
            ui.label("Custom face plane (magenta). Arrow keys return to axis planes.");
        }
        crate::theme::field(ui, "Plane offset (m)", |ui| {
            ui.add(crate::widgets::Number::new(&mut draft.offset).speed(draft.radius * 0.01));
        });
        ui.horizontal(|ui| {
            if ui.button("Reset Offset").clicked() {
                draft.offset = 0.;
            }
        });
        crate::widgets::checkbox(ui, &mut draft.copy, "Create mirrored copy");
        ui.separator();
        ui.horizontal(|ui| {
            commit = ui
                .add_enabled(
                    !draft.pick_face && draft.offset.is_finite(),
                    egui::Button::new(if draft.copy { "Flip Copy" } else { "Flip Selection" }),
                )
                .clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if commit && draft.offset.is_finite() {
        apply(app, draft);
    } else if open && !cancel {
        app.mirror = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.mirror.as_ref().filter(|d| d.valid(app)) else {
        return;
    };
    let (mut reflection, _, _, _) = draft.transform(app);
    if let Some(candidate) = draft.candidate.filter(|_| draft.pick_face) {
        reflection = DMat4::from_translation(candidate.origin)
            * candidate.basis
            * DMat4::from_scale(DVec3::new(1., 1., -1.))
            * candidate.basis.inverse()
            * DMat4::from_translation(-candidate.origin);
    }
    let world = app.editor.context_transform();
    let colours = [egui::Color32::LIGHT_RED, egui::Color32::LIGHT_GREEN, egui::Color32::LIGHT_BLUE];
    let magenta = egui::Color32::from_rgb(220, 110, 245);
    let draw_plane = |frame: Frame, axis: usize, colour: egui::Color32, active: bool| {
        let corners = frame.corners(axis, draft.radius);
        let view = app.view();
        let projected: Option<Vec<_>> = corners
            .iter()
            .map(|p| view.project(world.transform_point3(*p)).map(|p| egui::pos2(p.x as f32, p.y as f32)))
            .collect();
        if let Some(points) = projected {
            painter.add(egui::Shape::convex_polygon(
                points,
                colour.gamma_multiply(if active { 0.12 } else { 0.04 }),
                egui::Stroke::NONE,
            ));
        }
        let stroke =
            egui::Stroke::new(if active { 2.0 } else { 1.0 }, colour.gamma_multiply(if active { 1.0 } else { 0.55 }));
        for i in 0..4 {
            if let Some((a, b)) =
                view.project_segment(world.transform_point3(corners[i]), world.transform_point3(corners[(i + 1) % 4]))
            {
                painter.line_segment([egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)], stroke);
            }
        }
        if active && let Some(at) = view.project(world.transform_point3(frame.origin)) {
            painter.circle(
                egui::pos2(at.x as f32, at.y as f32),
                7.,
                colour,
                egui::Stroke::new(1.5, egui::Color32::from_rgb(25, 30, 38)),
            );
        }
    };
    if draft.custom.is_none() {
        for (axis, colour) in colours.into_iter().enumerate() {
            if axis != draft.axis {
                draw_plane(Frame { origin: draft.centre, basis: draft.axes(app) }, axis, colour, false);
            }
        }
    }
    draw_plane(draft.plane(app), draft.axis, if draft.custom.is_some() { magenta } else { colours[draft.axis] }, true);
    if let Some(candidate) = draft.candidate.filter(|_| draft.pick_face) {
        draw_plane(candidate, 2, magenta, true);
    }
    let mut line = |a: DVec3, b: DVec3, colour: egui::Color32| {
        if let Some((a, b)) = app.view().project_segment(world.transform_point3(a), world.transform_point3(b)) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.8, colour),
            );
        }
    };
    fn visit(
        def: &rysmo_model::Definition,
        doc: &rysmo_model::Document,
        t: DMat4,
        depth: usize,
        line: &mut impl FnMut(DVec3, DVec3, egui::Color32),
    ) {
        if depth > 64 {
            return;
        }
        for (_, edge) in def.mesh.edges() {
            line(
                t.transform_point3(def.mesh.pos(edge.v[0])),
                t.transform_point3(def.mesh.pos(edge.v[1])),
                egui::Color32::from_rgb(255, 180, 70),
            );
        }
        for instance in def.instances.values() {
            if let Some(child) = doc.definitions.get(&instance.definition) {
                visit(child, doc, t * instance.transform, depth + 1, line);
            }
        }
    }
    let source = world.inverse() * draft.snapshot.source_transform;
    visit(&draft.snapshot.document.root, &draft.snapshot.document, reflection * source, 0, &mut line);
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn preview_offset(app: &mut App3d) {
    if let Some(draft) = app.mirror.as_mut() {
        draft.offset = 1.6;
        draft.copy = true;
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn preview_face(app: &mut App3d) {
    if let Some(draft) = app.mirror.as_mut() {
        let frame = Frame::from_surface(DVec3::new(5., -1., 0.), DVec3::new(0.8, 0., 0.6), draft.centre).unwrap();
        draft.set_custom(frame);
        draft.copy = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::Editor;
    fn fixture() -> App3d {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 3., 1.),
                u: DVec3::X * 2.,
                v: DVec3::Y,
                facing: None,
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.set_selection([Element::Face(face)]);
        App3d::new(editor)
    }
    #[test]
    fn moved_plane_reflects_faces_and_undo_restores_original() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        let mut draft = app.mirror.take().unwrap();
        draft.offset = 2.; // plane x = 5
        let t = draft.transform(&app).0;
        assert!(t.transform_point3(DVec3::new(2., 3., 1.)).distance(DVec3::new(8., 3., 1.)) < 1e-10);
        apply(&mut app, draft);
        let (lo, hi) = app.editor.mesh().bounds().unwrap();
        assert!((lo.x - 6.).abs() < 1e-10 && (hi.x - 8.).abs() < 1e-10);
        let face = app.editor.mesh().faces().next().unwrap().0;
        assert!(app.editor.mesh().face_normal(face).unwrap().dot(DVec3::Z) > 0.99);
        assert_eq!(app.editor.undo_depth(), depth + 1);
        assert!(app.editor.undo());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn mirrored_copy_keeps_original_and_face_orientation() {
        let mut app = fixture();
        begin(&mut app);
        let mut draft = app.mirror.take().unwrap();
        draft.copy = true;
        draft.offset = 2.;
        apply(&mut app, draft);
        assert_eq!(app.editor.mesh().faces().count(), 2);
        for (face, _) in app.editor.mesh().faces() {
            assert!(app.editor.mesh().face_normal(face).unwrap().dot(DVec3::Z) > 0.99);
        }
        let (lo, hi) = app.editor.mesh().bounds().unwrap();
        assert!((lo.x - 2.).abs() < 1e-10 && (hi.x - 8.).abs() < 1e-10);
        app.editor.undo();
        assert_eq!(app.editor.mesh().faces().count(), 1);
    }
    #[test]
    fn stale_draft_and_escape_do_not_modify_document() {
        let mut app = fixture();
        begin(&mut app);
        let draft = app.mirror.take().unwrap();
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::Z]], facing: None }).unwrap();
        let before = app.editor.doc().clone();
        apply(&mut app, draft);
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.mirror.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        let draft = app.mirror.take().unwrap();
        app.document_generation += 1;
        apply(&mut app, draft);
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn nested_context_copy_uses_local_coordinates() {
        let mut app = fixture();
        let out = app.editor.apply(Command::MakeGroup { elements: app.editor.selected() }).unwrap();
        app.editor.set_selection(out.created);
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        app.editor.enter(id).unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.set_selection([Element::Face(face)]);
        begin(&mut app);
        let mut draft = app.mirror.take().unwrap();
        draft.copy = true;
        draft.offset = 2.;
        apply(&mut app, draft);
        let (lo, hi) = app.editor.mesh().bounds().unwrap();
        assert!(lo.x.abs() < 1e-10 && (hi.x - 6.).abs() < 1e-10);
        assert_eq!(app.editor.context(), &[id]);
        app.editor.undo();
        assert_eq!(app.editor.mesh().faces().count(), 1);
    }
    #[test]
    fn component_mirrored_copy_reuses_definition() {
        let mut app = fixture();
        let out = app
            .editor
            .apply(Command::MakeComponent { elements: app.editor.selected(), name: Some("Panel".into()) })
            .unwrap();
        app.editor.set_selection(out.created);
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let definition = app.editor.active().instances[&id].definition;
        begin(&mut app);
        let mut draft = app.mirror.take().unwrap();
        draft.copy = true;
        draft.offset = 2.;
        apply(&mut app, draft);
        assert_eq!(app.editor.active().instances.len(), 2);
        assert!(app.editor.active().instances.values().all(|i| i.definition == definition));
        assert_eq!(app.editor.active().instances.values().filter(|i| i.transform.determinant() < 0.).count(), 1);
        app.editor.undo();
        assert_eq!(app.editor.active().instances.len(), 1);
    }
    #[test]
    fn handle_drag_moves_plane_without_changing_model() {
        let mut app = fixture();
        begin(&mut app);
        let before = app.editor.doc().clone();
        let centre = app.mirror.as_ref().unwrap().centre;
        let start = app.local_view().project(centre).unwrap();
        let end = app.local_view().project(centre + DVec3::X).unwrap();
        pointer(&mut app, start, rysmo_tools::Pointer::Down { clicks: 1 });
        assert!(app.mirror.as_ref().unwrap().drag.is_some());
        pointer(&mut app, end, rysmo_tools::Pointer::Move);
        assert!((app.mirror.as_ref().unwrap().offset - 1.).abs() < 1e-8);
        pointer(&mut app, end, rysmo_tools::Pointer::Up);
        assert!(app.mirror.as_ref().unwrap().drag.is_none());
        assert_eq!(app.editor.doc(), &before);
        assert!(axis_parameter(rysmo_math::Ray::new(DVec3::ZERO, DVec3::X), centre, DVec3::X).is_none());
    }
    #[test]
    fn inclined_plane_reflection_and_copy_keep_facing_and_undo() {
        for copy in [false, true] {
            let mut app = fixture();
            let before = app.editor.doc().clone();
            let original = app.editor.mesh().vertices().map(|(id, _)| app.editor.mesh().pos(id)).collect::<Vec<_>>();
            begin(&mut app);
            let mut draft = app.mirror.take().unwrap();
            let frame = Frame::from_surface(DVec3::new(8., 0., 0.), DVec3::new(1., 1., 1.), draft.centre).unwrap();
            draft.set_custom(frame);
            draft.copy = copy;
            draft.offset = 0.75;
            let t = draft.transform(&app).0;
            let expected = original.iter().map(|p| t.transform_point3(*p)).collect::<Vec<_>>();
            apply(&mut app, draft);
            for point in expected {
                assert!(app.editor.mesh().vertices().any(|(id, _)| app.editor.mesh().pos(id).distance(point) < 1e-8));
            }
            let mirrored_normal = t.transform_vector3(DVec3::Z);
            assert!(
                app.editor
                    .selected()
                    .iter()
                    .filter_map(|e| if let Element::Face(id) = e { Some(*id) } else { None })
                    .all(|id| app.editor.mesh().face_normal(id).unwrap().dot(mirrored_normal) > 0.99)
            );
            assert_eq!(app.editor.mesh().faces().count(), if copy { 2 } else { 1 });
            app.editor.undo();
            assert_eq!(app.editor.doc(), &before);
        }
    }
    fn reference_fixture() -> (App3d, rysmo_model::FaceId, DVec3, DVec3) {
        let mut app = fixture();
        let selection = app.editor.selected();
        let corner = DVec3::new(6., -1., 2.);
        let u = DVec3::new(2., 0., 1.);
        let v = DVec3::new(0., 2., 0.);
        let out = app.editor.apply(Command::DrawRectangle { corner, u, v, facing: None }).unwrap();
        let face = out.created.iter().find_map(|e| if let Element::Face(id) = e { Some(*id) } else { None }).unwrap();
        let centre = corner + (u + v) * 0.5;
        app.camera.target = centre;
        app.camera.distance = 12.;
        app.editor.set_selection(selection);
        (app, face, centre, u.cross(v).normalize())
    }
    #[test]
    fn visible_face_hover_lock_drag_escape_and_axis_return() {
        let (mut app, _, point, normal) = reference_fixture();
        begin(&mut app);
        let before = app.editor.doc().clone();
        app.mirror.as_mut().unwrap().pick_face = true;
        let at = app.view().project(point).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Move);
        let candidate = app.mirror.as_ref().unwrap().candidate.unwrap();
        assert!(candidate.normal(2).dot(normal).abs() > 0.9999);
        assert!(Plane::new(normal, point).distance(candidate.origin).abs() < 1e-9);
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        pointer(&mut app, at, rysmo_tools::Pointer::Up);
        let frame = app.mirror.as_ref().unwrap().custom.unwrap();
        let handle = app.local_view().project(frame.origin).unwrap();
        let end = app.local_view().project(frame.origin + frame.normal(2) * 0.6).unwrap();
        pointer(&mut app, handle, rysmo_tools::Pointer::Down { clicks: 1 });
        pointer(&mut app, end, rysmo_tools::Pointer::Move);
        pointer(&mut app, end, rysmo_tools::Pointer::Up);
        assert!((app.mirror.as_ref().unwrap().offset - 0.6).abs() < 1e-8);
        app.mirror.as_mut().unwrap().pick_face = true;
        key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.mirror.as_ref().unwrap().custom.is_some());
        assert!(!app.mirror.as_ref().unwrap().pick_face);
        key(&mut app, rysmo_tools::Key::Right);
        let draft = app.mirror.as_ref().unwrap();
        assert!(draft.custom.is_none());
        assert_eq!(draft.axis, 0);
        assert_eq!(draft.offset, 0.);
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn face_pick_ignores_hidden_and_section_clipped_faces() {
        let (mut app, face, point, normal) = reference_fixture();
        let at = app.view().project(point).unwrap();
        assert!(face_frame(&app, at, DVec3::ZERO).is_some());
        let selected = app.editor.selected();
        app.editor.apply(Command::SetGeometryHidden { elements: vec![Element::Face(face)], hidden: true }).unwrap();
        assert!(face_frame(&app, at, DVec3::ZERO).is_none());
        app.editor.undo();
        app.editor.set_selection(selected);
        let mut doc = app.editor.doc().clone();
        doc.root.sections.planes.insert(
            999,
            rysmo_model::sections::SectionPlane { name: "Cut".into(), origin: point - normal * 0.1, normal },
        );
        doc.root.sections.active = Some(999);
        app.editor.replace_document(doc);
        assert!(face_frame(&app, at, DVec3::ZERO).is_none());
    }
    #[test]
    fn face_pick_converts_world_plane_into_scaled_editing_context() {
        let (mut app, _, point, normal) = reference_fixture();
        let selection = app.editor.selected();
        let group = app.editor.apply(Command::MakeGroup { elements: selection }).unwrap().created[0];
        let Element::Instance(id) = group else { panic!() };
        app.editor
            .apply(Command::ScaleInAxes {
                elements: vec![group],
                origin: DVec3::ZERO,
                factors: DVec3::new(1.5, 0.7, 2.5),
                x: DVec3::new(1., 1., 0.).normalize(),
                y: DVec3::new(-1., 1., 0.).normalize(),
            })
            .unwrap();
        app.editor.enter(id).unwrap();
        let context = app.editor.context_transform();
        let at = app.view().project(point).unwrap();
        let frame = face_frame(&app, at, DVec3::ZERO).unwrap();
        let world_plane = Plane::new(normal, point);
        for p in frame.corners(2, 2.) {
            assert!(world_plane.distance(context.transform_point3(p)).abs() < 1e-8);
        }
        assert!((frame.basis.determinant() - 1.).abs() < 1e-10);
    }
    #[test]
    fn coloured_plane_surfaces_can_be_selected_directly() {
        let mut app = fixture();
        begin(&mut app);
        let centre = app.mirror.as_ref().unwrap().centre;
        // Look from +X: only the red plane is front-facing.
        app.camera.target = centre;
        app.camera.yaw = 0.;
        app.camera.pitch = 0.;
        key(&mut app, rysmo_tools::Key::Up);
        let at = app.local_view().project(centre + DVec3::Y * 0.5 + DVec3::Z * 0.5).unwrap();
        pointer(&mut app, at, rysmo_tools::Pointer::Down { clicks: 1 });
        assert_eq!(app.mirror.as_ref().unwrap().axis, 0);
        assert!(app.mirror.as_ref().unwrap().drag.is_none()); // end-on axis, use numeric offset
        let frame = app.mirror.as_ref().unwrap().plane(&app);
        assert!(frame.hit(0, 1., Ray::new(centre + DVec3::X * 5. + DVec3::Y * 3., -DVec3::X)).is_none());
    }
    #[test]
    fn arbitrary_basis_reflection_is_an_involution_and_keys_choose_axis() {
        let mut app = fixture();
        begin(&mut app);
        key(&mut app, rysmo_tools::Key::Up);
        assert_eq!(app.mirror.as_ref().unwrap().axis, 2);
        key(&mut app, rysmo_tools::Key::Left);
        assert_eq!(app.mirror.as_ref().unwrap().axis, 1);
        let mut draft = app.mirror.take().unwrap();
        draft.basis = rysmo_math::DrawingAxes::new(DVec3::ZERO, DVec3::new(1., 2., 3.), DVec3::Y).unwrap().basis();
        draft.offset = 1.5;
        let t = draft.transform(&app).0;
        let p = DVec3::new(5., -7., 2.);
        assert!(t.transform_point3(t.transform_point3(p)).distance(p) < 1e-10);
        assert!((t.determinant() + 1.).abs() < 1e-10);
    }
}
