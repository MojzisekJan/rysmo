//! Input-preserving arrays, profile sweeps and Boolean components.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{
    Command, DefinitionId, Element,
    clipboard::Clipboard,
    path_array::{Distribution, Options},
    procedures::{Edit, Operation},
    solids::BooleanOp,
};
#[derive(Default)]
pub struct State {
    pub open: bool,
    revision: u64,
    mode: u8,
    count: u32,
    spacing: f64,
    by_spacing: bool,
    align: bool,
    reverse: bool,
    offset: f64,
    operation: Option<BooleanOp>,
    loaded: Option<(u64, DefinitionId)>,
    error: Option<String>,
}
fn selected(app: &App3d) -> Option<DefinitionId> {
    if let Some(def) = app.editor.active_definition().filter(|id| app.editor.doc().definitions[id].procedure.is_some())
    {
        return Some(def);
    }
    let selection = app.editor.selected();
    let [Element::Instance(id)] = selection.as_slice() else {
        return None;
    };
    let def = app.editor.active().instances.get(id)?.definition;
    app.editor.doc().definitions.get(&def)?.procedure.as_ref()?;
    Some(def)
}
fn load(state: &mut State, operation: &Operation) {
    match operation {
        Operation::Array { options } => {
            state.mode = 0;
            state.align = options.align_tangent;
            state.reverse = options.reverse;
            state.offset = options.start_offset;
            match options.distribution {
                Distribution::Count(n) => {
                    state.count = n;
                    state.by_spacing = false;
                }
                Distribution::Spacing(n) => {
                    state.spacing = n;
                    state.by_spacing = true;
                }
            }
        }
        Operation::Sweep => state.mode = 1,
        Operation::Solid { operation, reverse } => {
            state.mode = 2;
            state.operation = Some(*operation);
            state.reverse = *reverse;
        }
    }
}
fn operation(state: &State) -> Operation {
    match state.mode {
        0 => Operation::Array {
            options: Options {
                distribution: if state.by_spacing {
                    Distribution::Spacing(state.spacing)
                } else {
                    Distribution::Count(state.count)
                },
                align_tangent: state.align,
                reverse: state.reverse,
                start_offset: state.offset,
            },
        },
        1 => Operation::Sweep,
        _ => Operation::Solid { operation: state.operation.unwrap_or(BooleanOp::Union), reverse: state.reverse },
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.procedures.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.procedures);
    let selected = selected(app);
    if state.count == 0 {
        state.count = 10;
        state.spacing = 1.;
        state.align = true;
    }
    if let Some(id) = selected
        && (state.loaded != Some((app.document_generation, id)) || state.revision != app.editor.revision())
    {
        load(&mut state, &app.editor.doc().definitions[&id].procedure.as_ref().unwrap().operation);
        state.loaded = Some((app.document_generation, id));
        state.revision = app.editor.revision();
        state.error = None;
    }
    let editing = selected.is_some_and(|id| app.editor.doc().definitions[&id].procedure.as_ref().unwrap().editing);
    let mut create = false;
    let mut action = None;
    let mut enter = false;
    let window =
        crate::theme::window("Live Operations").vscroll(true).beside_model().open(&mut state.open).default_width(430.);
    let response = window.show(ctx, |ui| {
        ui.label("Preserve source geometry and parameters for later changes.");
        ui.horizontal_wrapped(|ui| {
            for (id, label) in [(0, "Path array"), (1, "Profile sweep"), (2, "Solid operation")] {
                ui.selectable_value(&mut state.mode, id, label);
            }
        });
        match state.mode {
            0 => {
                ui.checkbox(&mut state.by_spacing, "Use measured spacing");
                if state.by_spacing {
                    ui.add(
                        crate::widgets::Number::new(&mut state.spacing)
                            .range(0.00001..=1e6)
                            .prefix("Spacing ")
                            .suffix(" m"),
                    );
                } else {
                    ui.add(crate::widgets::Number::new(&mut state.count).range(1..=1024).prefix("New occurrences "));
                }
                ui.add(
                    crate::widgets::Number::new(&mut state.offset)
                        .range(0. ..=1e6)
                        .prefix("Start offset ")
                        .suffix(" m"),
                );
                ui.checkbox(&mut state.align, "Align X axis to path tangent");
                ui.checkbox(&mut state.reverse, "Reverse direction");
                ui.small("Select one component and a connected wire path.");
                ui.small("Distances use local metres. Scaling the live component also scales the array.");
            }
            1 => {
                ui.small("Select one profile face and separate connected path edges.");
                ui.small("Input editing retains ordinary editable profile and path geometry.");
            }
            _ => {
                let mut op = state.operation.unwrap_or(BooleanOp::Union);
                egui::ComboBox::from_label("Operation").selected_text(op.name()).show_ui(ui, |ui| {
                    for value in [
                        BooleanOp::Union,
                        BooleanOp::Subtract,
                        BooleanOp::Intersect,
                        BooleanOp::Trim,
                        BooleanOp::Split,
                        BooleanOp::OuterShell,
                    ] {
                        ui.selectable_value(&mut op, value, value.name());
                    }
                });
                state.operation = Some(op);
                ui.checkbox(&mut state.reverse, "Swap operands");
                ui.small("Select two solid groups/components. Stored inputs retain hierarchy and materials.");
            }
        }
        ui.separator();
        create = ui
            .add_enabled(!app.editor.selection().is_empty(), egui::Button::new("Create live component from selection"))
            .clicked();
        if selected.is_some() {
            if editing {
                ui.colored_label(egui::Color32::LIGHT_GREEN, "Editing source inputs");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Finish inputs and regenerate").clicked() {
                        action = Some(Edit::FinishInputs);
                    }
                    if ui.button("Cancel input changes").clicked() {
                        action = Some(Edit::CancelInputs);
                    }
                });
            } else {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Update parameters").clicked() {
                        action = Some(Edit::Regenerate);
                    }
                    if ui.button("Edit source inputs").clicked() {
                        action = Some(Edit::BeginInputs);
                        enter = true;
                    }
                });
            }
            if ui.button("Convert visible geometry to ordinary component").clicked() {
                action = Some(Edit::Detach);
            }
            ui.small("Shared copies update together. Make Unique first to isolate one.");
            ui.small(
                "Regeneration replaces output edits. Keep lasting edits in source inputs. Undo reverts each operation.",
            );
        }
        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    });
    super::remember(app, &response);
    if create {
        let selection = app.editor.selected();
        match Clipboard::capture(&app.editor, selection.clone()).and_then(|input| {
            let transform = app.editor.context_transform().inverse() * input.source_transform;
            app.editor.apply(Command::CreateProcedure {
                input: Box::new(input),
                operation: operation(&state),
                transform,
                replace: selection,
            })
        }) {
            Ok(out) => {
                app.editor.set_selection(out.created);
                state.loaded = None;
                state.error = None;
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    if let Some(edit) = action
        && let Some(definition) = selected
    {
        let selected_instance =
            app.editor.selected().iter().find_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None });
        match app.editor.apply(Command::UpdateProcedure {
            definition,
            operation: matches!(edit, Edit::Regenerate | Edit::FinishInputs).then(|| operation(&state)),
            edit,
        }) {
            Ok(_) => {
                state.error = None;
                if enter
                    && app.editor.active_definition() != Some(definition)
                    && let Some(instance) = selected_instance
                    && let Err(error) = app.editor.enter(instance)
                {
                    state.error = Some(error.to_string());
                }
            }
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    app.features.procedures = state;
}
