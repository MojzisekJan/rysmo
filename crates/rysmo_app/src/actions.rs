//! Menu actions on the selection: groups and components, edge styles, faces, guides.
//! Edit and Model split the shared actions by purpose; the context menu combines them.

use bevy_egui::egui;
use rysmo_model::{Command, EdgeId, Element, FaceId, InstanceId};

use crate::input::{erase_selection, select_all, undo};
use crate::menu::{self, glyph as g};
use crate::state::App3d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SectionPlane,
    IntersectModel,
    IntersectSelection,
    SolidTools,
    SolidCheck,
    SelectSimilar,
    PasteOnFace,
    Fillet,
    DivideEdges,
    ArrayAlongPath,
    ComponentData,
    LiveOperations,
    EditCurvePoints,
    WeldEdges,
    ExplodeCurves,
    OrientFaces,
    Drape,
    Stamp,
    AddDetail,
    FlipEdge,
    Smoove,
    FromContours,
    TerrainGrid,
    Text3d,
    MakeGroup,
    MakeComponent,
    OpenGroup,
    CloseGroup,
    Explode,
    MakeUnique,
    Unglue,
    SoftenEdges,
    UnsoftenEdges,
    HideGeometry,
    UnhideSelectedGeometry,
    UnhideGeometry,
    HideEdges,
    HideInstances,
    UnhideInstances,
    UnhideEdges,
    ReverseFaces,
    DeleteGuides,
    Erase,
    SelectAll,
    SelectNone,
    ZoomExtents,
    Undo,
}

impl Action {
    fn menu_group(self) -> u8 {
        match self {
            Self::MakeGroup
            | Self::MakeComponent
            | Self::OpenGroup
            | Self::CloseGroup
            | Self::Explode
            | Self::MakeUnique
            | Self::Unglue
            | Self::ArrayAlongPath
            | Self::ComponentData
            | Self::LiveOperations => 1,
            Self::ReverseFaces
            | Self::IntersectModel
            | Self::IntersectSelection
            | Self::WeldEdges
            | Self::ExplodeCurves
            | Self::OrientFaces
            | Self::DivideEdges
            | Self::EditCurvePoints
            | Self::Fillet
            | Self::SolidCheck
            | Self::SolidTools
            | Self::SoftenEdges
            | Self::UnsoftenEdges => 2,
            Self::HideGeometry
            | Self::UnhideSelectedGeometry
            | Self::UnhideGeometry
            | Self::HideEdges
            | Self::HideInstances
            | Self::UnhideInstances
            | Self::UnhideEdges => 3,
            Self::Drape
            | Self::Stamp
            | Self::AddDetail
            | Self::FlipEdge
            | Self::Smoove
            | Self::FromContours
            | Self::TerrainGrid => 4,
            Self::SectionPlane | Self::Text3d | Self::DeleteGuides => 5,
            Self::SelectAll | Self::SelectNone | Self::SelectSimilar => 6,
            _ => 0,
        }
    }
    fn menu_icon(self) -> &'static str {
        match self {
            Self::MakeGroup | Self::OpenGroup | Self::CloseGroup => g::CUBE,
            Self::MakeComponent | Self::MakeUnique => g::CUBE_FOCUS,
            Self::Explode => g::ARROWS_OUT,
            Self::Unglue => g::LINK_BREAK,
            Self::HideGeometry | Self::HideEdges | Self::HideInstances => g::EYE_SLASH,
            Self::UnhideSelectedGeometry | Self::UnhideGeometry | Self::UnhideInstances | Self::UnhideEdges => g::EYE,
            Self::ReverseFaces => g::ARROWS_CLOCKWISE,
            Self::IntersectModel | Self::IntersectSelection | Self::SolidTools => g::INTERSECT,
            Self::Fillet => g::ARROWS_CLOCKWISE,
            Self::SolidCheck => g::CUBE,
            Self::SelectSimilar => g::SELECTION_ALL,
            Self::PasteOnFace => g::CLIPBOARD,
            Self::DivideEdges | Self::EditCurvePoints => g::LINE_SEGMENT,
            Self::ArrayAlongPath | Self::ComponentData | Self::LiveOperations => g::CUBE_FOCUS,
            Self::WeldEdges => g::LINK,
            Self::ExplodeCurves => g::LINK_BREAK,
            Self::OrientFaces => g::ARROWS_CLOCKWISE,
            Self::SoftenEdges | Self::UnsoftenEdges => g::LINE_SEGMENT,
            Self::SectionPlane => g::SQUARE_SPLIT_VERTICAL,
            Self::Text3d => g::TEXT_T,
            Self::DeleteGuides => g::RULER,
            Self::Erase => g::TRASH,
            Self::SelectAll => g::SELECTION_ALL,
            Self::SelectNone => g::SELECTION_SLASH,
            Self::ZoomExtents => g::CORNERS_OUT,
            Self::Undo => g::ARROW_U_UP_LEFT,
            Self::TerrainGrid | Self::AddDetail => g::GRID_FOUR,
            Self::FlipEdge => g::ARROWS_LEFT_RIGHT,
            _ => g::MOUNTAINS,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Action::SectionPlane => "Section Plane",
            Action::IntersectModel => "Intersect Faces with Model",
            Action::IntersectSelection => "Intersect Faces with Selection",
            Action::SolidTools => "Solid Tools...",
            Action::SolidCheck => "Solid Check...",
            Action::SelectSimilar => "Select Similar...",
            Action::PasteOnFace => "Paste on Face...",
            Action::Fillet => "Fillet...",
            Action::DivideEdges => "Divide Edges...",
            Action::ArrayAlongPath => "Array Along Path...",
            Action::ComponentData => "Component Data & Connections...",
            Action::LiveOperations => "Live Operations...",
            Action::EditCurvePoints => "Edit Curve Points...",
            Action::WeldEdges => "Weld Edges",
            Action::ExplodeCurves => "Explode Curve",
            Action::OrientFaces => "Orient Faces",
            Action::Drape => "Drape",
            Action::Stamp => "Stamp",
            Action::AddDetail => "Add Detail",
            Action::FlipEdge => "Flip Edge",
            Action::Smoove => "Smoove",
            Action::FromContours => "From Contours",
            Action::TerrainGrid => "From Scratch",
            Action::Text3d => "3D Text",
            Action::MakeGroup => "Make Group",
            Action::MakeComponent => "Make Component (G)",
            Action::OpenGroup => "Edit Group",
            Action::CloseGroup => "Close Group",
            Action::Explode => "Explode",
            Action::MakeUnique => "Make Unique",
            Action::Unglue => "Unglue",
            Action::SoftenEdges => "Soften/Smooth Edges",
            Action::UnsoftenEdges => "Unsoften Edges",
            Action::HideGeometry => "Hide Selected Geometry",
            Action::UnhideSelectedGeometry => "Unhide Selected Geometry",
            Action::UnhideGeometry => "Unhide All Geometry",
            Action::HideEdges => "Hide Edges",
            Action::HideInstances => "Hide Groups/Components",
            Action::UnhideInstances => "Unhide All Groups/Components",
            Action::UnhideEdges => "Unhide All Edges",
            Action::ReverseFaces => "Reverse Faces",
            Action::DeleteGuides => "Delete Guides",
            Action::Erase => "Erase",
            Action::SelectAll => "Select All",
            Action::SelectNone => "Select None",
            Action::ZoomExtents => "Zoom Extents",
            Action::Undo => "Undo",
        }
    }
}

fn selected_instances(app: &App3d) -> Vec<InstanceId> {
    app.editor
        .selected()
        .into_iter()
        .filter_map(|e| if let Element::Instance(i) = e { Some(i) } else { None })
        .collect()
}

fn selected_faces(app: &App3d) -> Vec<FaceId> {
    app.editor.selected().into_iter().filter_map(|e| if let Element::Face(f) = e { Some(f) } else { None }).collect()
}

/// Selected edges, and the edges of selected faces.
fn selected_edges(app: &App3d) -> Vec<EdgeId> {
    let mesh = app.editor.mesh();
    let mut out: Vec<EdgeId> = Vec::new();
    for e in app.editor.selected() {
        match e {
            Element::Edge(x) => out.push(x),
            Element::Face(f) if mesh.face(f).is_some() => out.extend(mesh.face_edges(f)),
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

fn only_editable_edges(app: &App3d) -> bool {
    !app.editor.selection().is_empty()
        && app.editor.selection().len() <= 8192
        && app.editor.selection().iter().all(|element| {
            let Element::Edge(id) = element else { return false };
            app.editor.mesh().edge(*id).is_some_and(|edge| !edge.procedural)
        })
}

fn apply(app: &mut App3d, cmd: Command) {
    match app.editor.apply(cmd) {
        Ok(out) => {
            if matches!(out.created.first(), Some(Element::Instance(_))) {
                app.editor.set_selection(out.created);
            }
        }
        Err(e) => app.message = Some(format!("{e}.")),
    }
}

pub fn run(app: &mut App3d, action: Action) {
    let sel = app.editor.selected();
    match action {
        Action::SolidTools => crate::solid_tools::begin(app),
        Action::SolidCheck => crate::solid_check::begin(app),
        Action::SelectSimilar => crate::similar::begin(app),
        Action::PasteOnFace => crate::object_clipboard::paste_on_face(app),
        Action::Fillet => crate::fillet::begin(app),
        Action::DivideEdges => crate::divide::begin(app),
        Action::ArrayAlongPath => crate::path_array::begin(app),
        Action::ComponentData => app.features.component_data.open = true,
        Action::LiveOperations => app.features.procedures.open = true,
        Action::EditCurvePoints => crate::curve_points::begin(app),
        Action::WeldEdges | Action::ExplodeCurves => {
            let selected = app.editor.selected();
            if selected.iter().any(|element| !matches!(element, Element::Edge(_))) {
                app.message = Some("Select only edges or curves for this operation.".into());
                return;
            }
            let edges = selected_edges(app);
            let command = if action == Action::WeldEdges {
                Command::WeldEdges { edges }
            } else {
                Command::ExplodeCurves { edges }
            };
            match app.editor.apply(command) {
                Ok(out) => {
                    app.editor.set_selection(out.created);
                    app.message = Some(
                        if out.changed {
                            "Curve grouping updated. Undo restores the previous grouping."
                        } else {
                            "The selection is already one curve."
                        }
                        .into(),
                    );
                }
                Err(error) => app.message = Some(error.to_string()),
            }
        }
        Action::OrientFaces => {
            let [Element::Face(seed)] = sel.as_slice() else {
                app.message = Some("Select exactly one reference face to orient its neighbours.".into());
                return;
            };
            match app.editor.apply(Command::OrientFaces { seed: *seed }) {
                Ok(out) => {
                    app.message = Some(if out.changed {
                        format!("Reoriented {} faces. The reference face is unchanged.", out.created.len())
                    } else {
                        "Connected face orientations are already consistent.".into()
                    })
                }
                Err(error) => app.message = Some(error.to_string()),
            }
        }
        Action::IntersectModel | Action::IntersectSelection => {
            let context = app.editor.context().to_vec();
            apply(
                app,
                Command::IntersectFaces {
                    elements: sel.clone(),
                    with_model: action == Action::IntersectModel,
                    context,
                },
            );
        }

        Action::Drape => app.message = crate::drape_tool::begin(app).err(),
        Action::Stamp => app.message = crate::stamp_tool::begin(app).err(),
        Action::AddDetail => crate::detail_tool::begin(app),
        Action::FlipEdge => crate::flip_tool::begin(app),
        Action::Smoove => crate::smoove_tool::begin(app),
        Action::FromContours => {
            let edges = selected_edges(app);
            app.set_tool(rysmo_tools::ToolKind::Select);
            apply(app, Command::TerrainFromContours { edges });
        }
        Action::TerrainGrid => crate::terrain_tool::begin(app),
        Action::Text3d => crate::text3d_tool::begin(app),
        Action::SectionPlane => crate::section_panel::begin(app),
        Action::MakeGroup => apply(app, Command::MakeGroup { elements: sel }),
        Action::MakeComponent => {
            if sel.is_empty() {
                app.message = Some("Select geometry to make a component of.".into());
            } else {
                apply(app, Command::MakeComponent { elements: sel, name: None });
            }
        }
        Action::OpenGroup => {
            if let Some(i) = selected_instances(app).first().copied()
                && let Err(e) = app.editor.enter(i)
            {
                app.message = Some(e.to_string());
            }
        }
        Action::CloseGroup => {
            app.editor.exit();
        }
        Action::Explode => {
            for i in selected_instances(app) {
                apply(app, Command::Explode { instance: i });
            }
        }
        Action::MakeUnique => {
            for i in selected_instances(app) {
                apply(app, Command::MakeUnique { instance: i });
            }
        }
        Action::SoftenEdges => {
            let edges = selected_edges(app);
            let angle = app.soften_angle;
            apply(app, Command::SoftenEdges { edges, angle });
        }
        Action::UnsoftenEdges => {
            let edges = selected_edges(app);
            apply(app, Command::SetEdges { edges, soft: Some(false), smooth: Some(false), hidden: None });
        }
        Action::HideInstances => {
            let instances = selected_instances(app);
            apply(app, Command::SetInstancesHidden { instances, hidden: true });
        }
        Action::UnhideInstances => {
            let instances = app.editor.active().instances.iter().filter(|(_, i)| i.hidden).map(|(id, _)| *id).collect();
            apply(app, Command::SetInstancesHidden { instances, hidden: false });
        }
        Action::Unglue => {
            apply(app, Command::UnglueInstances { instances: selected_instances(app) });
        }
        Action::HideGeometry | Action::UnhideSelectedGeometry => {
            let mut elements: Vec<_> = sel
                .iter()
                .copied()
                .filter(|e| matches!(e, Element::Face(_) | Element::Edge(_) | Element::Instance(_) | Element::Guide(_)))
                .collect();
            elements.extend(selected_edges(app).into_iter().map(Element::Edge));
            elements.sort();
            elements.dedup();
            let hidden = action == Action::HideGeometry;
            apply(app, Command::SetGeometryHidden { elements, hidden });
            if hidden {
                app.editor.clear_selection();
            }
        }
        Action::UnhideGeometry => {
            let owner = app.editor.active();
            let elements = owner
                .mesh
                .faces()
                .filter(|(_, f)| f.hidden)
                .map(|(id, _)| Element::Face(id))
                .chain(owner.mesh.edges().filter(|(_, e)| e.hidden).map(|(id, _)| Element::Edge(id)))
                .chain(owner.instances.iter().filter(|(_, i)| i.hidden).map(|(id, _)| Element::Instance(*id)))
                .chain(owner.guides.iter().filter(|(_, g)| g.hidden()).map(|(id, _)| Element::Guide(*id)))
                .collect();
            apply(app, Command::SetGeometryHidden { elements, hidden: false });
        }
        Action::HideEdges => {
            let edges: Vec<EdgeId> =
                sel.iter().filter_map(|e| if let Element::Edge(x) = e { Some(*x) } else { None }).collect();
            apply(app, Command::SetEdges { edges, soft: None, smooth: None, hidden: Some(true) });
        }
        Action::UnhideEdges => {
            let edges: Vec<EdgeId> = app.editor.mesh().edges().filter(|(_, e)| e.hidden).map(|(id, _)| id).collect();
            apply(app, Command::SetEdges { edges, soft: None, smooth: None, hidden: Some(false) });
        }
        Action::ReverseFaces => {
            let faces = selected_faces(app);
            if !faces.is_empty() {
                apply(app, Command::ReverseFaces { faces });
            }
        }
        Action::DeleteGuides => apply(app, Command::DeleteGuides),
        Action::Erase => erase_selection(app),
        Action::SelectAll => select_all(app),
        Action::SelectNone => app.editor.clear_selection(),
        Action::ZoomExtents => app.zoom_extents(),
        Action::Undo => undo(app),
    }
}

/// Shared availability for menus and command search.
pub fn available(app: &App3d) -> Vec<(Action, bool)> {
    let has_sel = !app.editor.selection().is_empty();
    let has_inst = !selected_instances(app).is_empty();
    let has_faces = !selected_faces(app).is_empty();
    let has_edges = !selected_edges(app).is_empty();
    let in_group = !app.editor.context().is_empty();
    vec![
        (Action::Drape, true),
        (Action::Stamp, true),
        (Action::AddDetail, true),
        (Action::FlipEdge, true),
        (Action::Smoove, true),
        (Action::FromContours, has_edges),
        (Action::TerrainGrid, true),
        (Action::Text3d, true),
        (Action::SectionPlane, true),
        (Action::Erase, has_sel),
        (Action::OpenGroup, has_inst),
        (Action::CloseGroup, in_group),
        (Action::MakeGroup, has_sel),
        (Action::MakeComponent, has_sel),
        (Action::Explode, has_inst),
        (Action::MakeUnique, has_inst),
        (Action::Unglue, selected_instances(app).iter().any(|id| app.editor.active().instances[id].glued_to.is_some())),
        (Action::ReverseFaces, has_faces),
        (Action::IntersectModel, has_faces || has_inst),
        (Action::IntersectSelection, has_faces || has_inst),
        (Action::SolidCheck, has_inst || app.editor.mesh().edge_count() > 0 || app.editor.mesh().face_count() > 0),
        (Action::SelectSimilar, crate::similar::available(app)),
        (Action::PasteOnFace, app.object_clipboard.is_some()),
        (Action::SolidTools, app.editor.selection().len() == 2 && selected_instances(app).len() == 2),
        (Action::Fillet, app.editor.selection().len() == 2 && selected_edges(app).len() == 2),
        (Action::DivideEdges, crate::divide::available(app)),
        (Action::ArrayAlongPath, crate::path_array::available(app)),
        (Action::LiveOperations,true),
        (Action::ComponentData, app.editor.selection().iter().any(|e|matches!(e,Element::Instance(_)))),
        (Action::EditCurvePoints, crate::curve_points::available(app)),
        (Action::WeldEdges, app.editor.selection().len() >= 2 && only_editable_edges(app)),
        (Action::ExplodeCurves, only_editable_edges(app) && app.editor.selection().iter().any(|element| {
            matches!(element, Element::Edge(id) if app.editor.mesh().edge(*id).is_some_and(|edge| edge.curve.is_some()))
        })),
        (Action::OrientFaces, app.editor.selection().len() == 1 && has_faces),
        (Action::SoftenEdges, has_edges),
        (Action::UnsoftenEdges, has_edges),
        (
            Action::HideGeometry,
            has_edges || has_faces || has_inst || app.editor.selection().iter().any(|e| matches!(e, Element::Guide(_))),
        ),
        (
            Action::UnhideSelectedGeometry,
            has_edges || has_faces || has_inst || app.editor.selection().iter().any(|e| matches!(e, Element::Guide(_))),
        ),
        (Action::UnhideGeometry, true),
        (Action::HideEdges, has_edges),
        (Action::HideInstances, has_inst),
        (Action::UnhideInstances, app.editor.active().instances.values().any(|i| i.hidden)),
        (Action::UnhideEdges, true),
        (Action::DeleteGuides, !app.editor.active().guides.is_empty()),
        (Action::SelectAll, true),
        (Action::SelectNone, has_sel),
        (Action::ZoomExtents, true),
        (Action::Undo, app.editor.undo_label().is_some()),
    ]
}

/// The context menu's entries. Returns whether one was chosen.
pub fn menu(ui: &mut egui::Ui, app: &mut App3d) -> bool {
    selection_menu(ui, app, MenuScope::Context)
}

pub fn edit_menu(ui: &mut egui::Ui, app: &mut App3d) -> bool {
    selection_menu(ui, app, MenuScope::Edit)
}

pub fn model_menu(ui: &mut egui::Ui, app: &mut App3d) -> bool {
    selection_menu(ui, app, MenuScope::Model)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuScope {
    Edit,
    Model,
    Context,
}
impl MenuScope {
    fn includes(self, group: u8) -> bool {
        match self {
            Self::Context => true,
            Self::Edit => matches!(group, 0 | 1 | 3 | 6),
            Self::Model => matches!(group, 2 | 4 | 5),
        }
    }
}

fn selection_menu(ui: &mut egui::Ui, app: &mut App3d, scope: MenuScope) -> bool {
    if scope != MenuScope::Model && app.texture_session.is_some() {
        if menu::button(ui, g::ARROW_COUNTER_CLOCKWISE, "Reset texture position", "").clicked() {
            crate::texture_view::reset(app);
            return true;
        }
        if menu::button(ui, g::CHECK, "Done positioning texture", "").clicked() {
            crate::texture_view::finish(app, false);
            return true;
        }
        if menu::button(ui, g::X, "Cancel texture position", "").clicked() {
            crate::texture_view::finish(app, true);
            return true;
        }
        return false;
    }
    for (icon, label, shortcut, enabled, action) in [
        (g::COPY, "Copy", "Ctrl+C", !app.editor.selection().is_empty(), 0),
        (g::SCISSORS, "Cut", "Ctrl+X", !app.editor.selection().is_empty(), 1),
        (g::CLIPBOARD, "Paste", "Ctrl+V", app.object_clipboard.is_some(), 2),
        (g::CLIPBOARD_TEXT, "Paste in Place", "Ctrl+Shift+V", app.object_clipboard.is_some(), 3),
        (g::COPY_SIMPLE, "Duplicate", "Ctrl+D", !app.editor.selection().is_empty(), 4),
        (g::ARROWS_OUT, "Flip...", "", !app.editor.selection().is_empty(), 5),
    ] {
        if scope == MenuScope::Context && !enabled {
            continue;
        }
        if (action == 5 && scope == MenuScope::Edit) || (action != 5 && scope == MenuScope::Model) {
            continue;
        }
        if menu::enabled(ui, enabled, icon, label, shortcut).clicked() {
            match action {
                0 => crate::object_clipboard::copy(app, false),
                1 => crate::object_clipboard::copy(app, true),
                2 => crate::object_clipboard::paste(app, false),
                3 => crate::object_clipboard::paste(app, true),
                4 => crate::object_clipboard::duplicate(app),
                _ => crate::mirror::begin(app),
            };
            return true;
        }
    }
    ui.separator();
    if scope != MenuScope::Edit
        && !selected_faces(app).is_empty()
        && menu::button(ui, g::FRAME_CORNERS, "Position texture...", "").clicked()
    {
        crate::texture_view::begin(app);
        return true;
    }

    let entries = available(app);
    let has_inst = !selected_instances(app).is_empty();
    let in_group = !app.editor.context().is_empty();
    // The interface says "Component" for components.
    let doc = app.editor.doc();
    let first_is_group = selected_instances(app)
        .first()
        .and_then(|i| app.editor.active().instances.get(i))
        .is_some_and(|x| doc.definitions.get(&x.definition).is_some_and(|d| d.group));
    let editing_group = app.editor.active_definition().and_then(|d| doc.definitions.get(&d)).is_some_and(|d| d.group);
    let mut chosen = false;
    let mut entry = |ui: &mut egui::Ui, action: Action, enabled: bool| {
        let label = match action {
            Action::OpenGroup if has_inst && !first_is_group => "Edit Component",
            Action::CloseGroup if in_group && !editing_group => "Close Component",
            Action::MakeComponent => "Make Component",
            _ => action.label(),
        };
        let shortcut = match action {
            Action::MakeComponent => "G",
            Action::Erase => "Delete",
            Action::SelectAll => "Ctrl+A",
            Action::Undo => "Ctrl+Z",
            _ => "",
        };
        if menu::enabled(ui, enabled, action.menu_icon(), label, shortcut).clicked() {
            run(app, action);
            chosen = true;
        }
    };
    for (action, enabled) in entries.iter().filter(|(action, _)| {
        action.menu_group() == 0
            && scope.includes(0)
            && (scope != MenuScope::Context || entries.iter().any(|(a, enabled)| a == action && *enabled))
            && (*action != Action::ZoomExtents || scope == MenuScope::Context)
    }) {
        // Undo already has its own dynamic label at the top of Edit.
        if *action != Action::Undo || !egui::containers::menu::is_in_menu(ui) {
            entry(ui, *action, *enabled);
        }
    }
    if scope != MenuScope::Model {
        ui.separator();
    }
    for (group, icon, label) in [
        (1, g::CUBE, "Groups & Components"),
        (2, g::POLYGON, "Faces & Edges"),
        (3, g::EYE, "Visibility"),
        (4, g::MOUNTAINS, "Terrain"),
        (5, g::RULER, "Construction"),
        (6, g::SELECTION_ALL, "Selection"),
    ] {
        if !scope.includes(group)
            || (scope == MenuScope::Context
                && !entries.iter().any(|(action, enabled)| action.menu_group() == group && *enabled))
        {
            continue;
        }
        menu::submenu(ui, icon, label, |ui| {
            for (action, enabled) in entries
                .iter()
                .filter(|(action, enabled)| action.menu_group() == group && (scope != MenuScope::Context || *enabled))
            {
                entry(ui, *action, *enabled);
            }
        });
    }
    if has_inst && scope != MenuScope::Model {
        ui.separator();
        menu::submenu(ui, g::TAG, "Tag", |ui| {
            let tags: Vec<(Option<rysmo_model::TagId>, String)> = std::iter::once((None, "Untagged".to_string()))
                .chain(app.editor.doc().tags.iter().map(|(id, t)| (Some(*id), t.name.clone())))
                .collect();
            for (tag, name) in tags {
                if menu::button(ui, g::TAG, &name, "").clicked() {
                    let instances = selected_instances(app);
                    apply(app, Command::AssignTag { instances, tag });
                    chosen = true;
                }
            }
        });
    }
    chosen
}

#[cfg(test)]
mod contour_tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::Editor;
    #[test]
    fn from_contours_action_selects_new_group_and_keeps_sources() {
        let mut app = App3d::new(Editor::new());
        let lines = (0..3)
            .map(|row| [DVec3::new(0., row as f64 * 2., row as f64), DVec3::new(4., row as f64 * 2., row as f64)])
            .collect();
        app.editor.apply(Command::DrawLines { lines, facing: None }).unwrap();
        let selected = app.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect::<Vec<_>>();
        app.editor.set_selection(selected);
        let before = app.editor.doc().clone();
        let source = app.editor.mesh().clone();
        run(&mut app, Action::FromContours);
        assert!(app.message.is_none());
        assert_eq!(app.editor.mesh(), &source);
        assert_eq!(app.editor.undo_label(), Some("From Contours"));
        assert!(matches!(app.editor.selected().as_slice(), [Element::Instance(_)]));
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
}

#[cfg(test)]
mod geometry_visibility_tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn selective_unhide_restores_one_face_and_leaves_other_geometry_hidden() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        for x in [0., 3.] {
            app.editor
                .apply(Command::DrawRectangle { corner: DVec3::X * x, u: DVec3::X, v: DVec3::Y, facing: None })
                .unwrap();
        }
        let faces: Vec<_> = app.editor.mesh().faces().map(|(id, _)| id).collect();
        app.editor.set_selection(faces.iter().copied().map(Element::Face));
        run(&mut app, Action::HideGeometry);
        assert!(app.editor.mesh().faces().all(|(_, f)| f.hidden));
        app.editor.set_selection([Element::Face(faces[0])]);
        run(&mut app, Action::UnhideSelectedGeometry);
        assert!(!app.editor.mesh().face(faces[0]).unwrap().hidden);
        assert!(app.editor.mesh().face(faces[1]).unwrap().hidden);
        app.editor.undo();
        assert!(app.editor.mesh().faces().all(|(_, f)| f.hidden));
    }
}

#[cfg(test)]
mod topology_tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn curve_actions_preserve_selection_and_have_one_undo() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        let out = app
            .editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::X + DVec3::Y]],
                facing: None,
            })
            .unwrap();
        app.editor.set_selection(out.created);
        assert!(available(&app).contains(&(Action::WeldEdges, true)));
        assert!(available(&app).contains(&(Action::ExplodeCurves, false)));
        run(&mut app, Action::WeldEdges);
        assert_eq!(app.editor.mesh().curves().count(), 1);
        assert_eq!(app.editor.selection().len(), 2);
        assert!(available(&app).contains(&(Action::ExplodeCurves, true)));
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        run(&mut app, Action::ExplodeCurves);
        assert_eq!(app.editor.mesh().curves().count(), 0);
        assert_eq!(app.editor.selection().len(), 2);
        assert_eq!(app.editor.undo_depth(), depth + 1);
        assert!(app.editor.undo());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn orient_requires_one_face_and_retains_its_selection() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
        let face = app.editor.mesh().faces().next().unwrap().0;
        app.editor.apply(Command::PushPull { face, distance: 2., keep_base: false }).unwrap();
        let faces: Vec<_> = app.editor.mesh().faces().map(|(id, _)| id).collect();
        app.editor.apply(Command::ReverseFaces { faces: vec![faces[1]] }).unwrap();
        app.editor.set_selection([Element::Face(faces[0])]);
        assert!(available(&app).contains(&(Action::OrientFaces, true)));
        assert!(available(&app).contains(&(Action::WeldEdges, false)));
        run(&mut app, Action::OrientFaces);
        assert_eq!(app.editor.selected(), vec![Element::Face(faces[0])]);
        assert!(app.editor.mesh().orientation_plan(faces[0]).unwrap().is_empty());
        app.editor.set_selection(faces.into_iter().map(Element::Face));
        assert!(available(&app).contains(&(Action::OrientFaces, false)));
    }
}
