//! Per-mesh diagnostics across a bounded component hierarchy, with explicit repairs.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::DMat4;
use rysmo_model::{
    Command, DefinitionId, Element, InstanceId,
    solid_check::{Repair, Report},
};
use std::collections::BTreeSet;
const MAX_MARKERS: usize = 10000;
struct Part {
    name: String,
    path: Vec<InstanceId>,
    definition: Option<DefinitionId>,
    transform: DMat4,
    hidden: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Open,
    NonManifold,
    Orientation,
    Loose,
}
pub struct Dialog {
    parts: Vec<Part>,
    selected: usize,
    category: Category,
    report: Report,
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
}
impl Dialog {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.generation == app.document_generation
            && self.context == app.editor.context()
    }
}
fn parts(app: &App3d) -> Result<Vec<Part>, String> {
    struct Walker<'a> {
        doc: &'a rysmo_model::Document,
        parts: Vec<Part>,
        remaining: usize,
        ancestors: BTreeSet<DefinitionId>,
    }
    impl Walker<'_> {
        fn visit(
            &mut self,
            definition: Option<DefinitionId>,
            path: Vec<InstanceId>,
            transform: DMat4,
            name: String,
            hidden: bool,
        ) -> Result<(), String> {
            self.remaining = self.remaining.checked_sub(1).ok_or("Solid check exceeds 4096 occurrences")?;
            if path.len() > 32 {
                return Err("Solid check exceeds 32 hierarchy levels".into());
            }
            let def = self.doc.def(definition).ok_or("Missing component definition")?;
            if !transform.is_finite() || !transform.inverse().is_finite() {
                return Err("Invalid component transform".into());
            }
            if def.mesh.edge_count() > 0 || def.mesh.face_count() > 0 {
                self.parts.push(Part { name, path: path.clone(), definition, transform, hidden });
            }
            for (id, instance) in &def.instances {
                if !self.ancestors.insert(instance.definition) {
                    return Err("Cyclic component hierarchy".into());
                }
                let mut child = path.clone();
                child.push(*id);
                let label = if instance.name.is_empty() {
                    self.doc.definitions.get(&instance.definition).map(|def| def.name.clone()).unwrap_or_default()
                } else {
                    instance.name.clone()
                };
                self.visit(
                    Some(instance.definition),
                    child,
                    transform * instance.transform,
                    label,
                    hidden || instance.hidden || !self.doc.tag_visible(instance.tag),
                )?;
                self.ancestors.remove(&instance.definition);
            }
            Ok(())
        }
    }
    let mut context_hidden = false;
    let mut owner = None;
    for id in app.editor.context() {
        let instance =
            app.editor.doc().def(owner).and_then(|def| def.instances.get(id)).ok_or("Invalid editing path")?;
        context_hidden |= instance.hidden || !app.editor.doc().tag_visible(instance.tag);
        owner = Some(instance.definition);
    }
    let mut walker = Walker { doc: app.editor.doc(), parts: Vec::new(), remaining: 4096, ancestors: BTreeSet::new() };
    if let [Element::Instance(id)] = app.editor.selected().as_slice() {
        let instance = &app.editor.active().instances[id];
        walker.visit(
            Some(instance.definition),
            vec![*id],
            instance.transform,
            instance.name.clone(),
            context_hidden || instance.hidden || !app.editor.doc().tag_visible(instance.tag),
        )?;
    } else {
        walker.visit(
            app.editor.active_definition(),
            vec![],
            DMat4::IDENTITY,
            "Current context".into(),
            context_hidden,
        )?;
    }
    if walker.parts.is_empty() {
        return Err("There is no geometry to inspect in this context.".into());
    }
    Ok(walker.parts)
}
pub fn begin(app: &mut App3d) {
    let result = (|| {
        let parts = parts(app)?;
        let mesh = &app.editor.doc().def(parts[0].definition).ok_or("Missing geometry")?.mesh;
        let report = rysmo_model::solid_check::inspect(mesh).map_err(|error| error.to_string())?;
        Ok::<_, String>((parts, report))
    })();
    match result {
        Ok((parts, report)) => {
            app.set_tool(rysmo_tools::ToolKind::Select);
            app.solid_check = Some(Dialog {
                parts,
                selected: 0,
                category: Category::Open,
                report,
                revision: app.editor.revision(),
                generation: app.document_generation,
                context: app.editor.context().to_vec(),
            });
        }
        Err(error) => app.message = Some(error),
    }
}
fn elements(dialog: &Dialog) -> Vec<Element> {
    let edges = match dialog.category {
        Category::Open => &dialog.report.open_edges,
        Category::NonManifold => &dialog.report.non_manifold_edges,
        Category::Orientation => &dialog.report.orientation_edges,
        Category::Loose => &dialog.report.loose_edges,
    };
    let mut selected: Vec<_> = edges.iter().copied().map(Element::Edge).collect();
    if dialog.category == Category::NonManifold {
        selected.extend(dialog.report.non_manifold_faces.iter().copied().map(Element::Face));
    }
    selected
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut dialog) = app.solid_check.take() else { return };
    if !dialog.valid(app) {
        return;
    }
    let mut open = true;
    let mut select = false;
    let mut enter = false;
    let mut repair = None;
    let mut close = false;
    let window = crate::theme::window("Solid Check")
        .vscroll(true)
        .default_size(egui::vec2(420., 560.))
        .beside_model()
        .open(&mut open)
        .default_width(420.);
    window.show(ctx, |ui| {
        let previous = dialog.selected;
        crate::widgets::combo(egui::ComboBox::from_id_salt("solid-check-part"))
            .selected_text(&dialog.parts[dialog.selected].name)
            .show_ui(ui, |ui| {
                for (index, part) in dialog.parts.iter().enumerate() {
                    ui.selectable_value(&mut dialog.selected, index, format!("{}: {}", index + 1, part.name));
                }
            });
        if previous != dialog.selected {
            let part = &dialog.parts[dialog.selected];
            if let Some(def) = app.editor.doc().def(part.definition) {
                match rysmo_model::solid_check::inspect(&def.mesh) {
                    Ok(report) => dialog.report = report,
                    Err(error) => {
                        app.message = Some(error.to_string());
                        dialog.selected = previous;
                    }
                }
            }
        }
        if dialog.parts[dialog.selected].hidden {
            ui.colored_label(egui::Color32::LIGHT_RED, "This occurrence is hidden. Unhide it before Solid Tools.");
        }
        if let Some(reason) = &dialog.report.reason {
            ui.colored_label(egui::Color32::LIGHT_RED, reason);
        } else {
            ui.label("This mesh is a valid closed solid.");
        }
        if dialog.report.hidden_faces > 0 || dialog.report.hidden_edges > 0 {
            ui.label(format!("Hidden: {} faces, {} edges.", dialog.report.hidden_faces, dialog.report.hidden_edges));
            ui.small("Unhide operand faces before Solid Tools.");
        }
        for (category, name, count) in [
            (Category::Open, "Open edges", dialog.report.open_edges.len()),
            (Category::NonManifold, "Non-manifold edges", dialog.report.non_manifold_edges.len()),
            (Category::Orientation, "Orientation conflicts", dialog.report.orientation_edges.len()),
            (Category::Loose, "Loose edges", dialog.report.loose_edges.len()),
        ] {
            ui.selectable_value(&mut dialog.category, category, format!("{name}: {count}"));
        }
        ui.label(format!("Non-manifold vertices: {}", dialog.report.non_manifold_vertices.len()));
        let marker_count = dialog.report.open_edges.len()
            + dialog.report.non_manifold_edges.len()
            + dialog.report.orientation_edges.len()
            + dialog.report.loose_edges.len()
            + dialog.report.non_manifold_vertices.len();
        if marker_count > MAX_MARKERS {
            ui.small(format!("Showing the first {MAX_MARKERS} markers. Counts and selection include all problems."));
        }
        let local = dialog.parts[dialog.selected].path.is_empty();
        if !local {
            ui.small("Enter this geometry before selecting or repairing it.");
            ui.small("Shared component edits affect all copies. Use Make Unique first to isolate one.");
            enter = ui.button("Edit this geometry").clicked();
        } else {
            select =
                ui.add_enabled(!elements(&dialog).is_empty(), egui::Button::new("Select problem geometry")).clicked();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!dialog.report.loose_edges.is_empty(), egui::Button::new("Remove loose edges"))
                    .clicked()
                {
                    repair = Some(Repair::RemoveLooseEdges);
                }
                if ui.button("Repair closed-shell orientation").clicked() {
                    repair = Some(Repair::OrientClosedShells);
                }
            });
        }
        ui.small(
            "Orange: open boundaries. Red: non-manifold joins. Magenta: orientation conflicts. Blue: loose edges.",
        );
        ui.small("Repairs do not fill holes or change ambiguous topology.");
        ui.small("Checks apply to the chosen mesh, not a Boolean union of nested objects.");
        ui.small("Solid Tools also rejects glued occurrences, guides, text, dimensions and section planes.");
        ui.small("Other intersection problems are described above even when no edge can be marked.");
        close = ui.button("Close").clicked();
    });
    if enter {
        let path = dialog.parts[dialog.selected].path.clone();
        for id in path {
            if let Err(error) = app.editor.enter(id) {
                app.message = Some(error.to_string());
                return;
            }
        }
        app.editor.clear_selection();
        begin(app);
        return;
    }
    if let Some(repair) = repair {
        match app.editor.apply(Command::RepairSolid { repair }) {
            Ok(_) => {
                begin(app);
                return;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
    if select {
        app.editor.set_selection(elements(&dialog));
    }
    if open && !close {
        app.solid_check = Some(dialog);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(dialog) = app.solid_check.as_ref().filter(|d| d.valid(app)) else { return };
    let part = &dialog.parts[dialog.selected];
    let Some(def) = app.editor.doc().def(part.definition) else { return };
    let view = app.local_view();
    let mut remaining = MAX_MARKERS;
    for (edges, color) in [
        (&dialog.report.open_edges, egui::Color32::from_rgb(255, 170, 60)),
        (&dialog.report.non_manifold_edges, egui::Color32::LIGHT_RED),
        (&dialog.report.orientation_edges, egui::Color32::from_rgb(240, 100, 230)),
        (&dialog.report.loose_edges, egui::Color32::LIGHT_BLUE),
    ] {
        for id in edges.iter().take(remaining) {
            let (a, b) = def.mesh.edge_points(*id);
            if let Some((a, b)) =
                view.project_segment(part.transform.transform_point3(a), part.transform.transform_point3(b))
            {
                painter.line_segment(
                    [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                    egui::Stroke::new(3., color),
                );
            }
        }
        remaining = remaining.saturating_sub(edges.len());
    }
    for id in dialog.report.non_manifold_vertices.iter().take(remaining) {
        if let Some(vertex) = def.mesh.vertex(*id)
            && let Some(p) = view.project(part.transform.transform_point3(vertex.pos))
        {
            painter.circle_filled(egui::pos2(p.x as f32, p.y as f32), 5., egui::Color32::LIGHT_RED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn nested_problem_geometry_has_an_editable_path_and_local_highlights() {
        let mut editor = rysmo_model::Editor::new();
        let out = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let inner = editor.apply(Command::MakeGroup { elements: out.created }).unwrap().created[0];
        let outer = editor.apply(Command::MakeGroup { elements: vec![inner] }).unwrap().created[0];
        editor.apply(Command::Move { elements: vec![outer], offset: DVec3::X * 5. }).unwrap();
        editor.set_selection([outer]);
        let mut app = App3d::new(editor);
        begin(&mut app);
        let dialog = app.solid_check.as_ref().unwrap();
        assert_eq!(dialog.parts.len(), 1);
        assert_eq!(dialog.parts[0].path.len(), 2);
        assert_eq!(dialog.report.open_edges.len(), 4);
        assert_eq!(elements(dialog).len(), 4);
        let path = dialog.parts[0].path.clone();
        let transform = dialog.parts[0].transform;
        assert!((transform.w_axis.x - 5.).abs() < 1e-9);
        for id in path {
            app.editor.enter(id).unwrap();
        }
        app.editor.clear_selection();
        begin(&mut app);
        assert!(app.solid_check.as_ref().unwrap().parts[0].path.is_empty());
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.solid_check.is_none());
    }
    #[test]
    fn invalidated_report_cannot_highlight_reused_ids() {
        let mut editor = rysmo_model::Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let mut app = App3d::new(editor);
        begin(&mut app);
        assert_eq!(app.solid_check.as_ref().unwrap().report.loose_edges.len(), 1);
        app.document_generation += 1;
        assert!(!app.solid_check.as_ref().unwrap().valid(&app));
    }
}
