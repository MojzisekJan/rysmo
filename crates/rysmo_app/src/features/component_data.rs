//! Shared component metadata, occurrence overrides and named connection placement.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec3, DrawingAxes};
use rysmo_model::{
    Command, DefinitionId, Element, InstanceId,
    component_data::{Attribute, Attributes, Connections, Value},
};
#[derive(Default)]
pub struct State {
    pub open: bool,
    revision: u64,
    loaded: Option<(u64, DefinitionId, InstanceId)>,
    connections: Connections,
    attributes: Attributes,
    overrides: Attributes,
    new_connection: String,
    new_attribute: String,
    new_override: String,
    source_name: String,
    target_name: String,
    reverse: bool,
    error: Option<String>,
}
fn vector(ui: &mut egui::Ui, label: &str, value: &mut DVec3) {
    ui.label(label);
    ui.horizontal(|ui| {
        for n in [&mut value.x, &mut value.y, &mut value.z] {
            ui.add(crate::widgets::Number::new(n).speed(0.01));
        }
    });
}
fn attributes(ui: &mut egui::Ui, values: &mut Attributes, name: &mut String) {
    let mut remove = None;
    for (key, attribute) in values.iter_mut() {
        ui.push_id(key, |ui| {
            ui.horizontal(|ui| {
                ui.strong(key);
                if ui.small_button("Remove").clicked() {
                    remove = Some(key.clone());
                }
            });
            let mut kind = match attribute.value {
                Value::Text(_) => 0,
                Value::Number(_) => 1,
                Value::Boolean(_) => 2,
            };
            egui::ComboBox::from_id_salt("type").selected_text(["Text", "Number", "Boolean"][kind]).show_ui(ui, |ui| {
                for (i, label) in ["Text", "Number", "Boolean"].into_iter().enumerate() {
                    ui.selectable_value(&mut kind, i, label);
                }
            });
            if kind
                != match attribute.value {
                    Value::Text(_) => 0,
                    Value::Number(_) => 1,
                    Value::Boolean(_) => 2,
                }
            {
                attribute.value = match kind {
                    1 => Value::Number(0.),
                    2 => Value::Boolean(false),
                    _ => Value::Text(String::new()),
                };
            }
            match &mut attribute.value {
                Value::Text(text) => {
                    ui.add(crate::theme::singleline(text).desired_width(f32::INFINITY));
                }
                Value::Number(value) => {
                    ui.add(crate::widgets::Number::new(value));
                }
                Value::Boolean(value) => {
                    ui.checkbox(value, "Value");
                }
            }
            ui.horizontal(|ui| {
                ui.label("Unit");
                ui.add(crate::theme::singleline(&mut attribute.unit).desired_width(110.));
            });
            ui.separator();
        });
    }
    if let Some(key) = remove {
        values.remove(&key);
    }
    ui.horizontal(|ui| {
        ui.add(crate::theme::singleline(name).hint_text("Attribute name").desired_width(180.));
        if ui.add_enabled(!name.trim().is_empty() && values.len() < 128, egui::Button::new("Add field")).clicked() {
            values
                .entry(name.trim().into())
                .or_insert(Attribute { value: Value::Text(String::new()), unit: String::new() });
            name.clear();
        }
    });
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.component_data.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.component_data);
    let selected: Vec<_> = app
        .editor
        .selected()
        .into_iter()
        .filter_map(|e| if let Element::Instance(id) = e { Some(id) } else { None })
        .collect();
    let first =
        selected.first().and_then(|id| app.editor.active().instances.get(id).map(|instance| (*id, instance.clone())));
    let mut save = false;
    let mut attach = false;
    let window = crate::theme::window("Component Data & Connections")
        .vscroll(true)
        .beside_model()
        .open(&mut state.open)
        .default_size(egui::vec2(440., 550.));
    let response = window.show(ctx, |ui| {
        let Some((id, instance)) = &first else {
            ui.label("Select a component to edit its data, or two occurrences to connect them.");
            return;
        };
        let identity = (app.document_generation, instance.definition, *id);
        if state.loaded != Some(identity) || state.revision != app.editor.revision() {
            let def = &app.editor.doc().definitions[&instance.definition];
            state.revision = app.editor.revision();
            state.connections = def.component.connections.clone();
            state.attributes = def.component.attributes.clone();
            state.overrides = instance.attributes.clone();
            state.loaded = Some(identity);
            state.error = None;
        }
        ui.heading(&app.editor.doc().definitions[&instance.definition].name);
        ui.small("Definition changes affect all copies. Overrides affect the first selected occurrence.");
        ui.collapsing("Connection points", |ui| {
            let mut remove = None;
            for (name, frame) in &mut state.connections {
                ui.push_id(name, |ui| {
                    ui.collapsing(name, |ui| {
                        if ui.button("Remove point").clicked() {
                            remove = Some(name.clone());
                        }
                        vector(ui, "Local position (m)", &mut frame.origin);
                        vector(ui, "Local X direction", &mut frame.x);
                        vector(ui, "Local Y direction", &mut frame.y);
                        if ui.button("Orthonormalize directions").clicked()
                            && let Some(valid) = DrawingAxes::new(frame.origin, frame.x, frame.y)
                        {
                            *frame = valid;
                        }
                        if !frame.valid() {
                            ui.colored_label(egui::Color32::LIGHT_RED, "Use perpendicular unit directions.");
                        }
                    });
                });
            }
            if let Some(name) = remove {
                state.connections.remove(&name);
            }
            ui.horizontal(|ui| {
                ui.add(
                    crate::theme::singleline(&mut state.new_connection)
                        .hint_text("Connection name")
                        .desired_width(180.),
                );
                let valid = !state.new_connection.trim().is_empty() && state.connections.len() < 64;
                if ui.add_enabled(valid, egui::Button::new("Add point")).clicked() {
                    state.connections.entry(state.new_connection.trim().into()).or_default();
                    state.new_connection.clear();
                }
            });
        });
        ui.collapsing("Definition attributes", |ui| attributes(ui, &mut state.attributes, &mut state.new_attribute));
        ui.collapsing("Occurrence overrides", |ui| attributes(ui, &mut state.overrides, &mut state.new_override));
        save = ui.button("Apply data").clicked();
        if selected.len() == 2 {
            ui.separator();
            ui.heading("Attach occurrences");
            ui.checkbox(&mut state.reverse, "Swap source and target");
            let (source, target) = if state.reverse { (selected[1], selected[0]) } else { (selected[0], selected[1]) };
            let choices = |id| {
                &app.editor.doc().definitions[&app.editor.active().instances[&id].definition].component.connections
            };
            egui::ComboBox::from_label("Source point").selected_text(&state.source_name).show_ui(ui, |ui| {
                for name in choices(source).keys() {
                    ui.selectable_value(&mut state.source_name, name.clone(), name);
                }
            });
            egui::ComboBox::from_label("Target point").selected_text(&state.target_name).show_ui(ui, |ui| {
                for name in choices(target).keys() {
                    ui.selectable_value(&mut state.target_name, name.clone(), name);
                }
            });
            let valid =
                choices(source).contains_key(&state.source_name) && choices(target).contains_key(&state.target_name);
            attach = ui.add_enabled(valid, egui::Button::new("Attach source to target")).clicked();
            ui.small("Points meet face-to-face and retain source world size. Attachment makes one placement.");
        }
        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    });
    super::remember(app, &response);
    if save && let Some((id, instance)) = &first {
        // A single transaction prevents half-applied definition and override edits.
        match app.editor.apply(Command::SetComponentOccurrenceData {
            definition: instance.definition,
            instance: *id,
            connections: state.connections.clone(),
            attributes: state.attributes.clone(),
            overrides: state.overrides.clone(),
        }) {
            Ok(_) => state.error = None,
            Err(e) => state.error = Some(e.to_string()),
        }
    }
    if attach && selected.len() == 2 {
        let (source, target) = if state.reverse { (selected[1], selected[0]) } else { (selected[0], selected[1]) };
        match app.editor.apply(Command::AttachConnections {
            source,
            source_name: state.source_name.clone(),
            target,
            target_name: state.target_name.clone(),
            context: app.editor.context_transform(),
        }) {
            Ok(_) => state.error = None,
            Err(e) => state.error = Some(e.to_string()),
        }
    }
    app.features.component_data = state;
}

/// Display named local frames while their editor is open.
pub fn markers(ctx: &egui::Context, app: &App3d) {
    if !app.features.component_data.open {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("connection-frames")));
    for element in app.editor.selected() {
        let Element::Instance(id) = element else {
            continue;
        };
        let Some(instance) = app.editor.active().instances.get(&id) else {
            continue;
        };
        let definition = &app.editor.doc().definitions[&instance.definition];
        let transform = app.editor.context_transform() * instance.transform;
        let frames = if app.features.component_data.loaded == Some((app.document_generation, instance.definition, id)) {
            &app.features.component_data.connections
        } else {
            &definition.component.connections
        };
        for (name, frame) in frames {
            let origin = transform.transform_point3(frame.origin);
            let Some(at) = app.view().project(origin) else {
                continue;
            };
            if app.over_panel(at) {
                continue;
            }
            let at = egui::pos2(at.x as f32, at.y as f32);
            let scale = app.view().pixel_size_at(origin) * 22.;
            for (axis, color) in [
                (frame.x, egui::Color32::from_rgb(230, 100, 90)),
                (frame.y, egui::Color32::from_rgb(90, 200, 130)),
                (frame.x.cross(frame.y), egui::Color32::from_rgb(100, 160, 240)),
            ] {
                let direction = if axis == frame.x.cross(frame.y) {
                    transform.inverse().transpose().transform_vector3(axis)
                } else {
                    transform.transform_vector3(axis)
                };
                let Some(end) = app.view().project(origin + direction.normalize_or_zero() * scale) else {
                    continue;
                };
                painter.line_segment([at, egui::pos2(end.x as f32, end.y as f32)], egui::Stroke::new(2., color));
            }
            painter.circle_filled(at, 3., egui::Color32::WHITE);
            painter.text(
                at + egui::vec2(6., -8.),
                egui::Align2::LEFT_BOTTOM,
                name,
                egui::FontId::proportional(12.),
                egui::Color32::WHITE,
            );
        }
    }
}
