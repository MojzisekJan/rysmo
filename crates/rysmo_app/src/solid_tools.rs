//! Ordered solid operands and transactional Boolean operation controls.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, Element, InstanceId, solids::BooleanOp};
pub struct Draft {
    left: InstanceId,
    right: InstanceId,
    pub(crate) operation: BooleanOp,
    keep_inputs: bool,
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.generation == app.document_generation
            && self.context == app.editor.context()
    }
}
pub fn begin(app: &mut App3d) {
    begin_operation(app, BooleanOp::Union);
}
pub fn begin_operation(app: &mut App3d, operation: BooleanOp) {
    let selection = app.editor.selected();
    let [Element::Instance(left), Element::Instance(right)] = selection.as_slice() else {
        app.message = Some("Select exactly two groups or components for Solid Tools.".into());
        return;
    };
    let (left, right) = (*left, *right);
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.solid_tools = Some(Draft {
        left,
        right,
        operation,
        keep_inputs: false,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
    });
}
fn name(app: &App3d, id: InstanceId) -> String {
    app.editor
        .active()
        .instances
        .get(&id)
        .map(|i| {
            if !i.name.is_empty() {
                i.name.clone()
            } else {
                app.editor
                    .doc()
                    .definitions
                    .get(&i.definition)
                    .map(|d| d.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("Object {}", id.0))
            }
        })
        .unwrap_or_else(|| "Missing object".into())
}
fn apply(app: &mut App3d, draft: Draft) {
    if !draft.valid(app) {
        return;
    }
    match app.editor.apply(Command::SolidBoolean {
        left: draft.left,
        right: draft.right,
        operation: draft.operation,
        keep_inputs: draft.keep_inputs,
    }) {
        Ok(out) => {
            let empty = out.created.is_empty();
            app.editor.set_selection(out.created);
            app.message = Some(if empty {
                if draft.operation == BooleanOp::Trim {
                    "No remainder in A. Cutter B was retained.".into()
                } else {
                    "The solid result is empty.".into()
                }
            } else {
                format!("{} completed. Undo restores the operands.", draft.operation.name())
            });
        }
        Err(error) => app.message = Some(format!("Solid operation failed: {error}. The model was not changed.")),
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.solid_tools.take() else {
        return;
    };
    if !draft.valid(app) {
        return;
    }
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    crate::theme::window("Solid Tools").beside_model().open(&mut open).default_width(370.).show(ctx,|ui| {
        ui.label("Combine two closed groups or components.");
        ui.label(format!("A: {}",name(app,draft.left)));
        ui.label(format!("B: {}",name(app,draft.right)));
        if ui.button("Swap A and B").clicked(){std::mem::swap(&mut draft.left,&mut draft.right);}
        ui.separator();
        crate::widgets::combo(egui::ComboBox::from_label("Operation")).selected_text(draft.operation.name())
            .show_ui(ui,|ui| {
                for op in [BooleanOp::Union,BooleanOp::Subtract,BooleanOp::Intersect,BooleanOp::Trim,BooleanOp::Split,BooleanOp::OuterShell] {
                    ui.selectable_value(&mut draft.operation,op,op.name());
                }
            });
        ui.label(match draft.operation {
            BooleanOp::Union=>"Union combines the volumes of A and B.",
            BooleanOp::Subtract=>"Subtract removes B from A.",
            BooleanOp::Intersect=>"Intersect keeps only the shared volume.",
            BooleanOp::Trim=>"Trim removes B from A and always preserves cutter B.",
            BooleanOp::Split=>"Split creates A minus B, B minus A and their intersection as separate groups. Empty pieces are omitted.",
            BooleanOp::OuterShell=>"Outer Shell combines A and B and removes enclosed internal shells. Through-holes remain.",
        });
        crate::widgets::checkbox(ui, &mut draft.keep_inputs,if draft.operation==BooleanOp::Trim {"Keep A as well (B is always retained)"} else {"Keep original objects"});
        ui.label("Results are new groups. Other copies of shared components stay unchanged.");
        ui.small("Closed shells and nested solid groups/components are supported. Annotations and section planes inside operands are not supported. Limit: 4096 triangles per operand; complex or invalid geometry is rejected.");
        ui.horizontal(|ui| {commit=ui.button("Apply").clicked();cancel=ui.button("Cancel").clicked();});
    });
    if commit {
        apply(app, draft);
    } else if open && !cancel {
        app.solid_tools = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.solid_tools.as_ref().filter(|d| d.valid(app)) else {
        return;
    };
    for (id, label, colour) in [
        (draft.left, "A", egui::Color32::from_rgb(65, 125, 220)),
        (draft.right, "B", egui::Color32::from_rgb(205, 105, 25)),
    ] {
        let Some(instance) = app.editor.active().instances.get(&id) else {
            continue;
        };
        let Some((lo, hi)) = app.editor.doc().definition_bounds(Some(instance.definition)) else {
            continue;
        };
        let world = app.editor.context_transform() * instance.transform;
        if let Some(point) = app.view().project(world.transform_point3((lo + hi) * 0.5)) {
            let at = egui::pos2(point.x as f32, point.y as f32);
            painter.circle_filled(at, 13., colour);
            painter.text(at, egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(18.), egui::Color32::WHITE);
        }
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn preview_split(app: &mut App3d) {
    if let Some(draft) = app.solid_tools.as_mut() {
        draft.operation = BooleanOp::Split;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_selection_is_rejected() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        begin(&mut app);
        assert!(app.solid_tools.is_none());
        assert!(app.message.as_ref().unwrap().contains("exactly two"));
    }
    fn fixture() -> App3d {
        use rysmo_math::DVec3;
        let mut editor = rysmo_model::Editor::new();
        let mut instances = Vec::new();
        for corner in [DVec3::ZERO, DVec3::ONE] {
            editor
                .apply(Command::DrawRectangle { corner, u: DVec3::X * 2., v: DVec3::Y * 2., facing: Some(DVec3::Z) })
                .unwrap();
            let face = editor.mesh().faces().next().unwrap().0;
            editor
                .apply(Command::PushPull {
                    face,
                    distance: 2. * editor.mesh().face_normal(face).unwrap().z,
                    keep_base: false,
                })
                .unwrap();
            let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
            instances.extend(editor.apply(Command::MakeGroup { elements }).unwrap().created);
        }
        editor.set_selection(instances);
        App3d::new(editor)
    }
    #[test]
    fn editor_action_subtracts_and_native_result_round_trips() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        let mut draft = app.solid_tools.take().unwrap();
        draft.operation = BooleanOp::Subtract;
        apply(&mut app, draft);
        assert_eq!(app.editor.undo_depth(), depth + 1);
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let instance = &app.editor.active().instances[&id];
        assert!((app.editor.doc().instance_volume(instance, rysmo_math::DMat4::IDENTITY).unwrap() - 7.).abs() < 1e-7);
        let saved = rysmo_io::save(app.editor.doc());
        let loaded = rysmo_io::load(&saved).unwrap();
        assert_eq!(rysmo_io::save(&loaded), saved);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn escape_and_replacement_document_reject_stale_drafts() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        begin(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.solid_tools.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        let draft = app.solid_tools.take().unwrap();
        app.document_generation += 1;
        apply(&mut app, draft);
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn split_selects_and_saves_all_pieces_in_one_undo_step() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        let mut draft = app.solid_tools.take().unwrap();
        draft.operation = BooleanOp::Split;
        apply(&mut app, draft);
        assert_eq!(app.editor.selected().len(), 3);
        assert_eq!(app.editor.undo_depth(), depth + 1);
        let saved = rysmo_io::save(app.editor.doc());
        let loaded = rysmo_io::load(&saved).unwrap();
        assert_eq!(loaded.root.instances.len(), 3);
        assert_eq!(rysmo_io::save(&loaded), saved);
        let total = loaded
            .root
            .instances
            .values()
            .map(|i| loaded.instance_volume(i, rysmo_math::DMat4::IDENTITY).unwrap())
            .sum::<f64>();
        assert!((total - 15.).abs() < 1e-7);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
}
