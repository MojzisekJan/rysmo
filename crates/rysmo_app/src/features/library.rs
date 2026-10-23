use super::data_files::{self, Purpose};
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::collection::{Asset, Collection};
use rysmo_math::{DVec2, OrbitCamera};
use rysmo_model::{Document, clipboard::Clipboard};

#[derive(Default)]
pub struct State {
    pub open: bool,
    collection: Collection,
    documents: Vec<Document>,
    previews: Vec<Preview>,
    selected: Option<usize>,
    filter: String,
    name: String,
    description: String,
    pub modified: bool,
    error: Option<String>,
}
pub fn load(bytes: &[u8]) -> Result<State, String> {
    let collection = Collection::read(bytes)?;
    let documents = collection
        .assets
        .iter()
        .map(|asset| asset.document().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let previews = documents.iter().map(Preview::new).collect();
    Ok(State { open: true, collection, documents, previews, name: "Component".into(), ..Default::default() })
}
pub fn merge(app: &mut App3d, mut incoming: State) -> Result<(), String> {
    let state = &mut app.features.library;
    if state.collection.assets.is_empty() {
        *state = incoming;
        return Ok(());
    }
    let mut collection = state.collection.clone();
    for asset in &incoming.collection.assets {
        if !collection.assets.iter().any(|old| old.name == asset.name && old.model == asset.model) {
            collection.assets.push(asset.clone());
        }
    }
    collection.validate()?;
    let documents = collection
        .assets
        .iter()
        .map(|asset| asset.document().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    incoming.collection = collection;
    incoming.previews = documents.iter().map(Preview::new).collect();
    incoming.documents = documents;
    incoming.modified = true;
    *state = incoming;
    Ok(())
}
pub fn export_selection(app: &mut App3d) {
    match Clipboard::capture(&app.editor, app.editor.selected()) {
        Ok(clipboard) => {
            let bytes = rysmo_io::save(&clipboard.document);
            data_files::save(app, "component", "rysmo", bytes.as_bytes(), "application/json");
        }
        Err(error) => app.message = Some(error.to_string()),
    }
}
pub struct Preview {
    bounds: Option<(rysmo_math::DVec3, rysmo_math::DVec3)>,
    lines: Vec<[rysmo_math::DVec3; 2]>,
    pub occurrences: usize,
    pub faces: usize,
}
impl Preview {
    pub fn new(document: &Document) -> Self {
        let occurrences = document.occurrences(&[]);
        let mut lines = Vec::new();
        let mut faces = 0;
        for occurrence in &occurrences {
            let mesh = document.effective_mesh(occurrence.definition);
            faces += mesh.face_count();
            for (_, edge) in mesh.edges().filter(|(_, edge)| edge.visible()) {
                if lines.len() >= 3000 {
                    break;
                }
                lines.push(edge.v.map(|v| occurrence.transform.transform_point3(mesh.pos(v))));
            }
        }
        Self { bounds: document.definition_bounds(None), lines, occurrences: occurrences.len(), faces }
    }
}
pub fn thumbnail(ui: &mut egui::Ui, preview: &Preview) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width().clamp(120., 300.), 145.), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 4., egui::Color32::from_rgb(27, 28, 31));
    let Some((lo, hi)) = preview.bounds else {
        return;
    };
    let camera = OrbitCamera {
        target: (lo + hi) * 0.5,
        distance: ((hi - lo).length() * 2.2).max(0.1),
        parallel: true,
        ..Default::default()
    };
    let mut view = camera.view(DVec2::new(rect.width() as f64, rect.height() as f64));
    view.offset = DVec2::new(rect.min.x as f64, rect.min.y as f64);
    for [a, b] in &preview.lines {
        if let Some((a, b)) = view.project_segment(*a, *b) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1., egui::Color32::LIGHT_GRAY),
            );
        }
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.library.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.library);
    let mut open = true;
    let mut merge_file = false;
    let mut save = false;
    let mut place = None;
    let mut remove = None;
    let result=crate::theme::window("Component library").open(&mut open).default_width(350.).max_height((ctx.content_rect().height()-100.).max(220.)).vscroll(true).show(ctx,|ui| {
        ui.horizontal(|ui|{merge_file=ui.button("Merge collection...").clicked();save=ui.button("Save collection...").clicked();});
        state.modified|=ui.add(crate::theme::singleline(&mut state.collection.name).desired_width(f32::INFINITY).hint_text("Collection name")).changed();
        if state.modified {ui.weak("Collection modified. Save a .ryslib file to keep these changes.");}
        ui.separator();
        ui.add(crate::theme::singleline(&mut state.name).desired_width(f32::INFINITY).hint_text("New component name"));
        ui.add(crate::theme::singleline(&mut state.description).desired_width(f32::INFINITY).hint_text("Description"));
        if ui.add_enabled(!app.editor.selected().is_empty()&&!state.name.trim().is_empty(),egui::Button::new("Add selection to collection")).on_disabled_hover_text("Select model objects and enter a component name.").clicked() {
            let result=(||->Result<(),String>{
                let clipboard=Clipboard::capture(&app.editor,app.editor.selected()).map_err(|e|e.to_string())?;
                let asset=Asset::from_clipboard(state.name.trim().into(),state.description.clone(),&clipboard);
                let mut collection=state.collection.clone();collection.assets.push(asset);collection.validate()?;
                let document=collection.assets.last().ok_or("Missing new asset")?.document().map_err(|e|e.to_string())?;
                state.collection=collection;state.previews.push(Preview::new(&document));state.documents.push(document);state.modified=true;state.name.clear();state.description.clear();Ok(())
            })();state.error=result.err();
        }
        ui.separator();ui.add(crate::theme::singleline(&mut state.filter).desired_width(f32::INFINITY).hint_text("Search collection"));
        let filter=state.filter.to_lowercase();
        for (index,asset) in state.collection.assets.iter().enumerate().filter(|(_,a)|a.name.to_lowercase().contains(&filter)||a.description.to_lowercase().contains(&filter)) {
            if ui.selectable_label(state.selected==Some(index),&asset.name).clicked() {state.selected=Some(index);}
        }
        if let Some(index)=state.selected.filter(|i|*i<state.collection.assets.len()) {
            thumbnail(ui,&state.previews[index]);
            let asset=&mut state.collection.assets[index];
            state.modified|=ui.add(crate::theme::singleline(&mut asset.name).desired_width(f32::INFINITY)).changed();
            state.modified|=ui.add(crate::theme::singleline(&mut asset.description).desired_width(f32::INFINITY)).changed();
            ui.horizontal(|ui|{if ui.button("Place component").clicked(){place=Some(index);}
if ui.button("Remove from collection").clicked(){remove=Some(index);}});
        }
        if state.collection.assets.is_empty() {ui.weak("Select model objects and add them, or merge a saved collection. Collections remain available across New/Open in this session.");}
        if let Some(error)=&state.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
    });
    state.open = open;
    if let Some(index) = remove {
        place = None;
        state.collection.assets.remove(index);
        state.documents.remove(index);
        state.previews.remove(index);
        state.selected = None;
        state.modified = true;
    }
    if let Some(index) = place
        && let Err(error) = crate::object_clipboard::place_external(
            app,
            state.documents[index].clone(),
            state.collection.assets[index].name.clone(),
            false,
        )
    {
        state.error = Some(error);
    }
    if save {
        match state.collection.write() {
            Ok(bytes) => {
                if data_files::save(app, &state.collection.name, "ryslib", &bytes, "application/json") {
                    state.modified = false;
                }
            }
            Err(error) => state.error = Some(error),
        }
    }
    app.features.library = state;
    super::remember(app, &result);
    if merge_file {
        data_files::pick(app, Purpose::Collection);
    }
}
