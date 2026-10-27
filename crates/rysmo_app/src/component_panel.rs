//! In-model component browser, insertion preview, and hierarchy navigation.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DMat4, DVec2, DVec3};
use rysmo_model::{Command, DefinitionId, Element, InstanceId};
use rysmo_tools::{Pointer, ToolKind};

pub struct Placement {
    pub definition: DefinitionId,
    pub origin: DVec3,
    pub rotation: f64,
    pub scale: f64,
    pub host: Option<rysmo_model::FaceId>,
    revision: u64,
    context: Vec<InstanceId>,
}
impl Placement {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.context == app.editor.context()
            && app.editor.doc().definitions.contains_key(&self.definition)
    }
    fn transform(&self, app: &App3d) -> DMat4 {
        if let Some(face) = self.host
            && let Ok(matrix) = rysmo_model::components::glued_transform(
                app.editor.doc(),
                app.editor.active_definition(),
                self.definition,
                face,
                self.origin,
                self.rotation,
                self.scale,
            )
        {
            return matrix;
        }
        DMat4::from_translation(self.origin)
            * DMat4::from_rotation_z(self.rotation.to_radians())
            * DMat4::from_scale(DVec3::splat(self.scale))
            * app.editor.doc().definitions[&self.definition].component.frame().inverse()
    }
}
pub fn begin(app: &mut App3d, definition: DefinitionId) -> Result<(), String> {
    if !app.editor.doc().definitions.contains_key(&definition) {
        return Err("The component is missing.".into());
    }
    if app.editor.active_definition() == Some(definition)
        || app.editor.active_definition().is_some_and(|owner| app.editor.doc().contains_definition(definition, owner))
    {
        return Err("A component cannot contain itself.".into());
    }
    app.set_tool(ToolKind::Move);
    app.component_place = Some(Placement {
        definition,
        origin: app.editor.active().axes.origin,
        rotation: 0.,
        scale: 1.,
        host: None,
        revision: app.editor.revision(),
        context: app.editor.context().to_vec(),
    });
    app.message = Some("Place component: click an insertion point. Escape cancels.".into());
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.component_place.as_ref().is_some_and(|p| p.valid(app)) {
        app.component_place = None;
        return;
    }
    app.with_ctx(at, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
    if let Some(inference) = app.tools.preview(&app.editor).inference {
        let definition = app.component_place.as_ref().unwrap().definition;
        let gluing =
            app.editor.doc().definitions[&definition].component.glue != rysmo_model::components::GlueMode::None;
        let host = gluing
            .then_some(inference.face)
            .flatten()
            .filter(|id| inference.instance.is_none() && app.editor.mesh().face(*id).is_some());
        let placement = app.component_place.as_mut().unwrap();
        placement.origin = inference.point;
        placement.host = host;
    }
    if matches!(event, Pointer::Down { .. }) {
        finish(app);
    }
}
pub fn finish(app: &mut App3d) {
    let Some(p) = app.component_place.as_ref() else { return };
    if !p.valid(app) {
        app.component_place = None;
        return;
    }
    let glue = app.editor.doc().definitions[&p.definition].component.glue;
    let command = if glue != rysmo_model::components::GlueMode::None {
        let Some(face) = p.host else {
            app.message = Some("Pick a matching face in the current editing context.".into());
            return;
        };
        Command::PlaceGluedInstance {
            definition: p.definition,
            face,
            origin: p.origin,
            rotation: p.rotation,
            scale: p.scale,
        }
    } else {
        Command::PlaceInstance { definition: p.definition, origin: p.origin, rotation: p.rotation, scale: p.scale }
    };
    match app.editor.apply(command) {
        Ok(out) => {
            app.component_place = None;
            app.editor.set_selection(out.created);
            app.set_tool(ToolKind::Move);
            app.message = None;
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}

pub fn distance(app: &mut App3d, text: &str) -> Result<(), String> {
    let Some(p) = app.component_place.as_ref() else { return Err("No component is being placed.".into()) };
    if !p.valid(app) {
        app.component_place = None;
        return Err("The model changed; place the component again.".into());
    }
    let distance = rysmo_model::parse_length(text, app.editor.doc().units).map_err(|e| e.to_string())?;
    let axes = app.editor.active().axes;
    let plane = p.host.and_then(|face| app.editor.mesh().face_plane(face));
    let origin = plane.map_or(axes.origin, |plane| plane.project(axes.origin));
    let fallback = plane
        .and_then(|plane| (axes.x - plane.normal * axes.x.dot(plane.normal)).try_normalize())
        .unwrap_or_else(|| plane.map_or(axes.x, |plane| plane.basis().0));
    let p = app.component_place.as_mut().unwrap();
    p.origin = origin + (p.origin - origin).try_normalize().unwrap_or(fallback) * distance;
    finish(app);
    if app.component_place.is_some() {
        Err(app.message.clone().unwrap_or_else(|| "Cannot place the component here.".into()))
    } else {
        Ok(())
    }
}

pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(p) = app.component_place.as_ref().filter(|p| p.valid(app)) else { return };
    fn visit(app: &App3d, painter: &egui::Painter, id: DefinitionId, t: DMat4, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(d) = app.editor.doc().def(Some(id)) else { return };
        let view = app.view();
        for (_, edge) in d.mesh.edges().filter(|(_, e)| e.visible()) {
            if let Some((a, b)) = view
                .project_segment(t.transform_point3(d.mesh.pos(edge.v[0])), t.transform_point3(d.mesh.pos(edge.v[1])))
            {
                painter.line_segment(
                    [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                    egui::Stroke::new(1.3, egui::Color32::MAGENTA),
                );
            }
        }
        for instance in d.instances.values().filter(|i| !i.hidden && app.editor.doc().tag_visible(i.tag)) {
            visit(app, painter, instance.definition, t * instance.transform, depth + 1);
        }
    }
    visit(app, painter, p.definition, app.editor.context_transform() * p.transform(app), 0);
}

struct Row {
    path: Vec<InstanceId>,
    name: String,
    hidden: bool,
    tag_visible: bool,
    has_children: bool,
}
fn rows(app: &App3d) -> Vec<Row> {
    fn collect(app: &App3d, owner: Option<DefinitionId>, path: &mut Vec<InstanceId>, out: &mut Vec<Row>, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(d) = app.editor.doc().def(owner) else { return };
        let name = |instance: &rysmo_model::Instance| {
            if instance.name.is_empty() {
                app.editor.doc().definitions.get(&instance.definition).map(|d| d.name.clone()).unwrap_or_default()
            } else {
                instance.name.clone()
            }
        };
        let mut children = d.instances.iter().collect::<Vec<_>>();
        if app.outliner_sort {
            children.sort_by(|(ia, a), (ib, b)| {
                name(a).to_lowercase().cmp(&name(b).to_lowercase()).then_with(|| ia.cmp(ib))
            });
        }
        for (id, instance) in children {
            path.push(*id);
            let name = if instance.name.is_empty() {
                app.editor.doc().definitions.get(&instance.definition).map(|d| d.name.clone()).unwrap_or_default()
            } else {
                instance.name.clone()
            };
            out.push(Row {
                path: path.clone(),
                name,
                hidden: instance.hidden,
                tag_visible: app.editor.doc().tag_visible(instance.tag),
                has_children: app
                    .editor
                    .doc()
                    .definitions
                    .get(&instance.definition)
                    .is_some_and(|d| !d.instances.is_empty()),
            });
            collect(app, Some(instance.definition), path, out, depth + 1);
            path.pop();
        }
    }
    let mut out = Vec::new();
    collect(app, None, &mut Vec::new(), &mut out, 0);
    out
}
/// Use child ordinals while entering copied groups: Make Unique remaps their IDs.
fn focus(app: &mut App3d, path: &[InstanceId], open: bool) -> Result<InstanceId, String> {
    let mut ordinals = Vec::new();
    let mut owner = None;
    for (index, id) in path.iter().enumerate() {
        let d = app.editor.doc().def(owner).ok_or("The hierarchy changed.")?;
        if (index + 1 < path.len() || open)
            && !app.editor.doc().tag_visible(d.instances.get(id).ok_or("The hierarchy changed.")?.tag)
        {
            return Err("Show the instance's tag before entering this context.".into());
        }
        ordinals.push(d.instances.keys().position(|i| i == id).ok_or("The hierarchy changed.")?);
        owner = Some(d.instances[id].definition);
    }
    app.set_tool(ToolKind::Select);
    app.editor.exit_all();
    let mut selected = None;
    for (index, ordinal) in ordinals.iter().enumerate() {
        let id = *app.editor.active().instances.keys().nth(*ordinal).ok_or("The hierarchy changed.")?;
        selected = Some(id);
        if index + 1 < ordinals.len() || open {
            if app.editor.active().instances[&id].hidden {
                app.editor
                    .apply(Command::SetInstancesHidden { instances: vec![id], hidden: false })
                    .map_err(|e| e.to_string())?;
            }
            app.editor.enter(id).map_err(|e| e.to_string())?;
        } else {
            app.editor.set_selection([Element::Instance(id)]);
        }
    }
    let context = app.editor.context().to_vec();
    for n in 1..=context.len() {
        app.outliner_collapsed.remove(&context[..n]);
    }
    selected.ok_or("Select a group or component.".into())
}
pub fn outliner(ui: &mut egui::Ui, app: &mut App3d) {
    ui.heading("Outliner");
    ui.add(
        crate::theme::singleline(&mut app.outliner_filter)
            .desired_width(f32::INFINITY)
            .hint_text("Search groups/components"),
    );
    let filter = app.outliner_filter.to_lowercase();
    crate::widgets::checkbox(ui, &mut app.outliner_sort, "Sort by name");
    let rows = rows(app);
    ui.horizontal(|ui| {
        if ui.small_button("Expand all").clicked() {
            app.outliner_collapsed.clear();
        }
        if ui.small_button("Collapse all").clicked() {
            app.outliner_collapsed = rows.iter().filter(|r| r.has_children).map(|r| r.path.clone()).collect();
        }
    });
    let visible: Vec<_> = rows
        .iter()
        .filter(|r| {
            (filter.is_empty() || r.name.to_lowercase().contains(&filter))
                && (!filter.is_empty() || !(1..r.path.len()).any(|n| app.outliner_collapsed.contains(&r.path[..n])))
        })
        .collect();
    let mut action = None;
    for row in visible {
        ui.horizontal(|ui| {
            ui.add_space((row.path.len() - 1) as f32 * 12.);
            if row.has_children {
                let collapsed = app.outliner_collapsed.contains(&row.path);
                if ui.small_button(if collapsed { ">" } else { "v" }).clicked() {
                    if collapsed {
                        app.outliner_collapsed.remove(&row.path);
                    } else {
                        app.outliner_collapsed.insert(row.path.clone());
                    }
                }
            } else {
                ui.add_space(18.);
            }
            let id = *row.path.last().unwrap();
            let selected = app.editor.context() == &row.path[..row.path.len() - 1]
                && app.editor.is_selected(Element::Instance(id));
            let label = if row.hidden {
                format!("{} [hidden]", row.name)
            } else if !row.tag_visible {
                format!("{} [hidden tag]", row.name)
            } else {
                row.name.clone()
            };
            let response = ui.selectable_label(selected, label);
            response.context_menu(|ui| {
                if ui.button("Rename").clicked() {
                    app.outliner_rename = Some((row.path.clone(), row.name.clone()));
                    ui.close();
                }
            });
            if response.double_clicked() {
                action = Some((row.path.clone(), true, None));
            } else if response.clicked() {
                action = Some((row.path.clone(), false, None));
            }
            if ui.small_button(if row.hidden { "Show" } else { "Hide" }).clicked() {
                action = Some((row.path.clone(), false, Some(!row.hidden)));
            }
        });
    }
    if rows.is_empty() {
        ui.weak("No groups or components in the model.");
    }
    if let Some((path, open, hidden)) = action {
        match focus(app, &path, open) {
            Ok(id) => {
                if let Some(hidden) = hidden {
                    app.message = app
                        .editor
                        .apply(Command::SetInstancesHidden { instances: vec![id], hidden })
                        .err()
                        .map(|e| e.to_string());
                }
            }
            Err(error) => app.message = Some(error),
        }
    }
    if !app.editor.context().is_empty() && ui.button("Close editing context").clicked() {
        app.editor.exit();
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.show_components {
        return;
    }
    let mut open = true;
    let result = crate::theme::window("Components")
        .open(&mut open)
        .default_width(320.)
        .beside_model()
        .max_height((ctx.content_rect().height() - 170.).max(200.))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.label("In Model");
            ui.add(crate::theme::singleline(&mut app.component_filter).desired_width(f32::INFINITY).hint_text("Search components"));
            let filter = app.component_filter.to_lowercase();
            let definitions: Vec<_> = app
                .editor
                .doc()
                .definitions
                .iter()
                .filter(|(_, d)| {
                    !d.group
                        && (filter.is_empty()
                            || d.name.to_lowercase().contains(&filter)
                            || d.component.description.to_lowercase().contains(&filter))
                })
                .map(|(id, d)| (*id, d.clone()))
                .collect();
            for (id, d) in &definitions {
                ui.horizontal(|ui| {
                    if ui.selectable_label(app.component_selected == Some(*id), &d.name).clicked() {
                        app.component_selected = Some(*id);
                        app.component_edit = Some((*id, d.name.clone(), d.component.clone()));
                    }
                    if ui.button("Place").clicked()
                        && let Err(error) = begin(app, *id) {
                            app.message = Some(error);
                        }
                });
            }
            if definitions.is_empty() {
                ui.weak("Create a component from selected geometry first.");
            }
            if let Some((id, mut name, mut settings)) = app.component_edit.clone() {
                if !app.editor.doc().definitions.contains_key(&id) {
                    app.component_edit = None;
                    return;
                }
                ui.separator();
                ui.label("Name");
                ui.add(crate::theme::singleline(&mut name).desired_width(f32::INFINITY));
                ui.label("Description");
                ui.add(crate::theme::multiline(&mut settings.description));
                use rysmo_model::components::GlueMode;
                let glue_modes = [
                    (GlueMode::None, "None"),
                    (GlueMode::Any, "Any face"),
                    (GlueMode::Horizontal, "Horizontal"),
                    (GlueMode::Vertical, "Vertical"),
                    (GlueMode::Sloped, "Sloped"),
                ];
                let glue_label = glue_modes.iter().find(|(mode, _)| *mode == settings.glue).map_or("None", |(_, l)| *l);
                crate::widgets::combo(egui::ComboBox::from_label("Glue to")).selected_text(glue_label).show_ui(
                    ui,
                    |ui| {
                        for (value, label) in glue_modes {
                            ui.selectable_value(&mut settings.glue, value, label);
                        }
                    },
                );
                if settings.glue == GlueMode::None {
                    settings.cut_opening = false;
                }
                ui.add_enabled_ui(settings.glue != GlueMode::None, |ui| {
                    crate::widgets::checkbox(ui, &mut settings.cut_opening, "Cut opening");
                });
                if settings.cut_opening {
                    ui.small(
                        "Cuts one face using a closed outline in the insertion plane. The outline must fit inside the host without overlapping other openings.",
                    );
                }
                // Gluing applies at once, so Place uses it; name and axes wait for Apply.
                let definition = &app.editor.doc().definitions[&id];
                if settings.glue != definition.component.glue || settings.cut_opening != definition.component.cut_opening {
                    let applied = rysmo_model::components::ComponentSettings {
                        glue: settings.glue,
                        cut_opening: settings.cut_opening,
                        ..definition.component.clone()
                    };
                    let name = definition.name.clone();
                    app.message = app
                        .editor
                        .apply(Command::SetComponentProperties { definition: id, name, settings: applied })
                        .err()
                        .map(|e| e.to_string());
                }
                ui.small("Glue orientations use the editing context's blue axis.");
                ui.label(format!("{} references in model", app.editor.doc().instance_count(id)));
                thumbnail(ui, app, id);
                ui.label("Insertion origin (metres)");
                ui.horizontal(|ui| {
                    for value in [
                        &mut settings.insertion_axes.origin.x,
                        &mut settings.insertion_axes.origin.y,
                        &mut settings.insertion_axes.origin.z,
                    ] {
                        ui.add(crate::widgets::Number::new(value).speed(0.01));
                    }
                });
                let mut x = settings.insertion_axes.x.to_array();
                let mut y = settings.insertion_axes.y.to_array();
                ui.label("Red direction");
                ui.horizontal(|ui| {
                    for v in &mut x {
                        ui.add(crate::widgets::Number::new(v).speed(0.01));
                    }
                });
                ui.label("Green direction");
                ui.horizontal(|ui| {
                    for v in &mut y {
                        ui.add(crate::widgets::Number::new(v).speed(0.01));
                    }
                });
                let axes = rysmo_math::DrawingAxes::new(
                    settings.insertion_axes.origin,
                    DVec3::from_array(x),
                    DVec3::from_array(y),
                );
                settings.insertion_axes.x = DVec3::from_array(x);
                settings.insertion_axes.y = DVec3::from_array(y);
                if axes.is_none() {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Directions must be nonzero and independent.");
                }
                if ui.button("Reset insertion axes").clicked() {
                    settings.insertion_axes = Default::default();
                }
                if ui.add_enabled(axes.is_some(), egui::Button::new("Apply properties")).clicked() {
                    settings.insertion_axes = axes.unwrap();
                    app.message = app
                        .editor
                        .apply(Command::SetComponentProperties {
                            definition: id,
                            name: name.clone(),
                            settings: settings.clone(),
                        })
                        .err()
                        .map(|e| e.to_string());
                }
                if ui.button("Select instances in current context").clicked() {
                    let selected = app
                        .editor
                        .active()
                        .instances
                        .iter()
                        .filter(|(_, i)| i.definition == id)
                        .map(|(id, _)| Element::Instance(*id))
                        .collect::<Vec<_>>();
                    app.set_tool(ToolKind::Select);
                    app.editor.set_selection(selected);
                }
                if ui
                    .add_enabled(
                        app.editor.doc().instance_count(id) == 0,
                        egui::Button::new("Delete unused definition"),
                    )
                    .clicked()
                {
                    let _ = app.editor.apply(Command::DeleteDefinition { definition: id });
                    app.component_edit = None;
                } else {
                    app.component_edit = Some((id, name, settings));
                }
            }
            if let Some(p) = app.component_place.as_mut() {
                ui.separator();
                ui.label("Placement");
                ui.add(crate::widgets::Number::new(&mut p.rotation).speed(1.).suffix("°"));
                ui.add(crate::widgets::Number::new(&mut p.scale).speed(0.05).prefix("Scale "));
                ui.horizontal(|ui| {
                    for value in [&mut p.origin.x, &mut p.origin.y, &mut p.origin.z] {
                        ui.add(crate::widgets::Number::new(value).speed(0.01).suffix(" m"));
                    }
                });
                if ui.button("Place at these coordinates").clicked() {
                    finish(app);
                }
            }
        });
    app.show_components = open;
    if let Some(result) = result {
        let r = result.response.rect;
        app.ui_rects.push((DVec2::new(r.min.x as f64, r.min.y as f64), DVec2::new(r.max.x as f64, r.max.y as f64)));
    }
}

pub fn rename_panel(ctx: &egui::Context, app: &mut App3d) {
    let Some((path, mut name)) = app.outliner_rename.clone() else { return };
    let mut open = true;
    // Typing goes to the name, not to the Measurements box: keep asking for focus until the
    // click that opened the window has been released.
    let focused = egui::Id::unique("rename-instance-focused");
    let result = crate::theme::window("Rename instance").open(&mut open).show(ctx, |ui| {
        let field = ui.add(crate::theme::singleline(&mut name).desired_width(f32::INFINITY));
        if !ctx.data(|d| d.get_temp::<bool>(focused).unwrap_or(false)) {
            field.request_focus();
            if field.has_focus() && !ui.input(|i| i.pointer.any_down() || i.pointer.any_released()) {
                ctx.data_mut(|d| d.insert_temp(focused, true));
            }
        }
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() || entered {
                match focus(app, &path, false) {
                    Ok(id) => {
                        app.message = app
                            .editor
                            .apply(Command::RenameInstance { instance: id, name: name.clone() })
                            .err()
                            .map(|e| e.to_string());
                        app.outliner_rename = None;
                    }
                    Err(error) => app.message = Some(error),
                }
            }
            if ui.button("Cancel").clicked() {
                app.outliner_rename = None;
            }
        });
    });
    if !open {
        app.outliner_rename = None;
    } else if app.outliner_rename.is_some() {
        app.outliner_rename = Some((path, name));
    }
    if app.outliner_rename.is_none() {
        ctx.data_mut(|d| d.remove::<bool>(focused));
    }
    if let Some(result) = result {
        let r = result.response.rect;
        app.ui_rects.push((DVec2::new(r.min.x as f64, r.min.y as f64), DVec2::new(r.max.x as f64, r.max.y as f64)));
    }
}

fn thumbnail(ui: &mut egui::Ui, app: &App3d, id: DefinitionId) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(200., 140.), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4., egui::Color32::from_rgb(27, 28, 31));
    let Some((lo, hi)) = app.editor.doc().definition_bounds(Some(id)) else { return };
    let camera = rysmo_math::OrbitCamera {
        target: (lo + hi) * 0.5,
        distance: ((hi - lo).length() * 2.2).max(0.1),
        parallel: true,
        ..Default::default()
    };
    let mut view = camera.view(DVec2::new(rect.width() as f64, rect.height() as f64));
    view.offset = DVec2::new(rect.min.x as f64, rect.min.y as f64);
    fn visit(app: &App3d, painter: &egui::Painter, view: &rysmo_math::View, id: DefinitionId, t: DMat4, depth: usize) {
        if depth > 64 {
            return;
        }
        let Some(d) = app.editor.doc().def(Some(id)) else { return };
        for (_, edge) in d.mesh.edges().filter(|(_, e)| e.visible()) {
            if let Some((a, b)) = view
                .project_segment(t.transform_point3(d.mesh.pos(edge.v[0])), t.transform_point3(d.mesh.pos(edge.v[1])))
            {
                painter.line_segment(
                    [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                    egui::Stroke::new(1., egui::Color32::LIGHT_GRAY),
                );
            }
        }
        for instance in d.instances.values().filter(|i| !i.hidden && app.editor.doc().tag_visible(i.tag)) {
            visit(app, painter, view, instance.definition, t * instance.transform, depth + 1);
        }
    }
    visit(app, &painter, &view, id, DMat4::IDENTITY, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::Editor;
    pub(super) fn sample() -> (App3d, InstanceId, DefinitionId) {
        let mut app = App3d::new(Editor::new());
        let elements = app
            .editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = app.editor.apply(Command::MakeComponent { elements, name: Some("Panel".into()) }).unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let definition = app.editor.doc().root.instances[&id].definition;
        (app, id, definition)
    }
    #[test]
    fn component_placement_preview_matches_commit_and_supports_undo() {
        let (mut app, _, definition) = sample();
        app.editor
            .apply(Command::SetComponentProperties {
                definition,
                name: "Panel".into(),
                settings: rysmo_model::components::ComponentSettings {
                    connections: Default::default(),
                    attributes: Default::default(),
                    glue: Default::default(),
                    cut_opening: false,
                    description: String::new(),
                    insertion_axes: rysmo_math::DrawingAxes::new(DVec3::new(0.5, 0.5, 0.), DVec3::Y, -DVec3::X)
                        .unwrap(),
                },
            })
            .unwrap();
        app.canvas = Some((DVec2::new(120., 90.), DVec2::new(900., 650.)));
        let before = app.editor.doc().clone();
        begin(&mut app, definition).unwrap();
        let at = app.view().project(DVec3::new(3., 2., 0.)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert_eq!(app.editor.doc(), &before);
        let p = app.component_place.as_mut().unwrap();
        p.rotation = 90.;
        p.scale = 2.;
        let expected = app.component_place.as_ref().unwrap().transform(&app);
        finish(&mut app);
        assert!(app.component_place.is_none());
        assert_eq!(app.editor.doc().root.instances.len(), 2);
        let id = *app.editor.doc().root.instances.keys().last().unwrap();
        assert!(app.editor.doc().root.instances[&id].transform.abs_diff_eq(expected, 1e-9));
        let after = app.editor.doc().clone();
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        app.editor.redo();
        assert_eq!(app.editor.doc(), &after);
    }
    #[test]
    fn component_escape_and_stale_placement_do_not_change_the_model() {
        let (mut app, _, definition) = sample();
        let before = app.editor.doc().clone();
        begin(&mut app, definition).unwrap();
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.component_place.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app, definition).unwrap();
        app.editor.apply(Command::AddTag { name: "Later".into() }).unwrap();
        let changed = app.editor.doc().clone();
        finish(&mut app);
        assert_eq!(app.editor.doc(), &changed);
    }
    #[test]
    fn outliner_navigation_remaps_children_of_copied_groups() {
        let (mut app, child, definition) = sample();
        let out = app.editor.apply(Command::MakeGroup { elements: vec![Element::Instance(child)] }).unwrap();
        let Element::Instance(parent) = out.created[0] else { panic!() };
        let original_definition = app.editor.doc().root.instances[&parent].definition;
        let original = app.editor.doc().definitions[&original_definition].clone();
        let out = app
            .editor
            .apply(Command::Copy { elements: vec![Element::Instance(parent)], offset: DVec3::X * 3., copies: 1 })
            .unwrap();
        let copy =
            out.created.into_iter().find_map(|e| if let Element::Instance(id) = e { Some(id) } else { None }).unwrap();
        assert_eq!(rows(&app).len(), 4);
        let mapped = focus(&mut app, &[copy, child], false).unwrap();
        assert_ne!(mapped, child);
        assert_eq!(app.editor.context(), &[copy]);
        assert!(app.editor.is_selected(Element::Instance(mapped)));
        assert_eq!(app.editor.doc().definitions[&original_definition], original);
        focus(&mut app, &[copy, mapped], true).unwrap();
        assert_eq!(app.editor.active_definition(), Some(definition));
    }
    #[test]
    fn component_browser_renders_properties_and_hidden_outliner_rows() {
        let (mut app, instance, definition) = sample();
        app.editor.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: true }).unwrap();
        assert_eq!(rows(&app).len(), 1);
        assert!(rows(&app)[0].hidden);
        app.show_components = true;
        app.component_edit = Some((definition, "Panel".into(), Default::default()));
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            panel(ui.ctx(), &mut app);
            outliner(ui, &mut app);
        });
        output.textures_delta.clear();
        assert!(!app.ui_rects.is_empty());
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    #[test]
    fn hidden_outliner_group_opens_visibly_but_hidden_tags_do_not_change() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        let elements = app
            .editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = app.editor.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        app.editor.apply(Command::SetInstancesHidden { instances: vec![id], hidden: true }).unwrap();
        focus(&mut app, &[id], true).unwrap();
        assert!(!app.editor.doc().root.instances[&id].hidden);
        assert_eq!(app.editor.context(), &[id]);
        app.editor.exit_all();
        let tag = app.editor.apply(Command::AddTag { name: "Hidden".into() }).unwrap().tag.unwrap();
        app.editor.apply(Command::AssignTag { instances: vec![id], tag: Some(tag) }).unwrap();
        app.editor.apply(Command::SetTag { tag, name: None, visible: Some(false) }).unwrap();
        let before = app.editor.doc().clone();
        assert!(focus(&mut app, &[id], true).is_err());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn glued_preview_matches_placement_and_invalid_click_keeps_the_draft() {
        let (mut app, _, definition) = super::tests::sample();
        let mut settings = app.editor.doc().definitions[&definition].component.clone();
        settings.glue = rysmo_model::components::GlueMode::Any;
        settings.cut_opening = true;
        app.editor.apply(Command::SetComponentProperties { definition, name: "Window".into(), settings }).unwrap();
        app.editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 0.),
                u: DVec3::X * 8.,
                v: DVec3::Y * 8.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = app
            .editor
            .mesh()
            .faces()
            .find(|(id, _)| app.editor.mesh().face_contains(*id, DVec3::new(6., 6., 0.)))
            .unwrap()
            .0;
        app.zoom_extents();
        let before = app.editor.doc().clone();
        begin(&mut app, definition).unwrap();
        finish(&mut app);
        assert!(app.component_place.is_some());
        assert_eq!(app.editor.doc(), &before);
        let at = app.view().project(DVec3::new(6., 6., 0.)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert_eq!(app.component_place.as_ref().unwrap().host, Some(face));
        let expected = app.component_place.as_ref().unwrap().transform(&app);
        finish(&mut app);
        assert!(app.component_place.is_none());
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        assert_eq!(app.editor.active().instances[&id].glued_to, Some(face));
        assert_eq!(app.editor.active().instances[&id].transform, expected);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
}
