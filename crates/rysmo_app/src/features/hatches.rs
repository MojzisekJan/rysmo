//! Shared section hatches cached by document identity and revision.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, Element, MaterialId, section_hatching::Hatch};
#[derive(Default)]
pub struct State {
    pub lines: Vec<[rysmo_math::DVec3; 2]>,
    key: Option<(u64, u64)>,
    pub error: Option<String>,
    scope: u8,
    revision: u64,
    material: Option<MaterialId>,
    draft: Hatch,
    enabled: bool,
    loaded: Option<(u64, u8, Option<rysmo_model::DefinitionId>, Option<MaterialId>)>,
}
pub fn refresh(app: &mut App3d) {
    let key = (app.document_generation, app.editor.revision());
    if app.features.hatches.key == Some(key) {
        return;
    }
    let state = &mut app.features.hatches;
    state.key = Some(key);
    match rysmo_model::section_hatching::build(app.editor.doc()) {
        Ok(lines) => {
            state.lines = lines;
            state.error = None;
        }
        Err(error) => {
            state.lines.clear();
            state.error = Some(error.to_string());
        }
    }
}
pub fn settings(ui: &mut egui::Ui, app: &mut App3d) {
    let mut state = std::mem::take(&mut app.features.hatches);
    ui.collapsing("Section hatching", |ui| {
        let scopes = ["Model default", "Selected component", "Material"];
        egui::ComboBox::from_label("Apply to").selected_text(scopes[state.scope as usize]).show_ui(ui, |ui| {
            for (index, name) in scopes.into_iter().enumerate() {
                ui.selectable_value(&mut state.scope, index as u8, name);
            }
        });
        if state.scope == 2 {
            let name = state
                .material
                .and_then(|id| app.editor.doc().materials.get(&id))
                .map(|m| m.name.as_str())
                .unwrap_or("Choose material");
            egui::ComboBox::from_label("Material").selected_text(name).show_ui(ui, |ui| {
                for (id, material) in &app.editor.doc().materials {
                    ui.selectable_value(&mut state.material, Some(*id), &material.name);
                }
            });
        }
        let definition = if state.scope == 1 {
            app.editor.selected().iter().find_map(|element| match element {
                Element::Instance(id) => app.editor.active().instances.get(id).map(|i| i.definition),
                _ => None,
            })
        } else {
            None
        };
        let material = (state.scope == 2).then_some(state.material).flatten();
        let valid = match state.scope {
            1 => definition.is_some(),
            2 => material.is_some(),
            _ => true,
        };
        if !valid {
            ui.label("Choose the target component or material.");
            return;
        }
        let identity = (app.document_generation, state.scope, definition, material);
        if state.loaded != Some(identity) || state.revision != app.editor.revision() {
            let hatch = if let Some(id) = material {
                app.editor.doc().materials.get(&id).and_then(|m| m.section_hatch)
            } else {
                app.editor.doc().def(definition).and_then(|d| d.section_hatch)
            };
            state.revision = app.editor.revision();
            state.enabled = hatch.is_some();
            state.draft = hatch.unwrap_or_default();
            state.loaded = Some(identity);
        }
        ui.checkbox(&mut state.enabled, "Enable hatch override");
        ui.add(crate::widgets::Number::new(&mut state.draft.angle).prefix("Angle ").suffix("°"));
        ui.add(
            crate::widgets::Number::new(&mut state.draft.spacing)
                .range(0.0001..=1000.)
                .speed(0.005)
                .prefix("Spacing ")
                .suffix(" m"),
        );
        ui.checkbox(&mut state.draft.cross, "Crosshatch");
        if ui.button("Apply hatching").clicked() {
            match app.editor.apply(Command::SetSectionHatch {
                definition,
                material,
                hatch: state.enabled.then_some(state.draft),
            }) {
                Ok(_) => state.key = None,
                Err(error) => state.error = Some(error.to_string()),
            }
        }
        ui.small("Component overrides precede whole-occurrence material hatches, then the model default.");
        ui.small("Spacing uses world metres. Face-only paint does not assign a section hatch.");
        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    });
    app.features.hatches = state;
}
