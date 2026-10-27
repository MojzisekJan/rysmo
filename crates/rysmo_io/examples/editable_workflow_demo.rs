//! Reproducible fixture for live components, metadata, cuts and drawing sheets.
use rysmo_math::{DMat4, DVec3, DrawingAxes};
use rysmo_model::{
    Command, Editor, Element,
    clipboard::Clipboard,
    component_data::{Attribute, Value},
    parametric::Spec,
    path_array::{Distribution, Options},
    procedures::Operation,
    section_hatching::Hatch,
    sections::SectionPlane,
    sheets::{Note, Projection, Sheet, Viewport},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).unwrap_or_else(|| "target/editable-workflow-demo".into());
    std::fs::create_dir_all(&output)?;
    let mut input = Editor::with_document(Spec::default().document()?);
    input.apply(Command::DrawLines { lines: vec![[DVec3::new(0., 3., 0.), DVec3::new(5., 3., 0.)]], facing: None })?;
    let mut editor = Editor::new();
    let out = editor.apply(Command::CreateProcedure {
        input: Box::new(Clipboard { document: input.doc().clone(), source_transform: DMat4::IDENTITY }),
        operation: Operation::Array { options: Options { distribution: Distribution::Count(3), ..Default::default() } },
        transform: DMat4::IDENTITY,
        replace: vec![],
    })?;
    let Element::Instance(id) = out.created[0] else { unreachable!() };
    let mut doc = editor.doc().clone();
    let definition = doc.root.instances[&id].definition;
    let def = doc.def_mut(Some(definition)).unwrap();
    def.component.connections.insert("Base".into(), DrawingAxes::default());
    def.component.attributes.insert("Cost".into(), Attribute { value: Value::Number(125.), unit: "EUR".into() });
    doc.root_mut()
        .instances
        .get_mut(&id)
        .unwrap()
        .attributes
        .insert("Description".into(), Attribute { value: Value::Text("Window assembly".into()), unit: String::new() });
    let section = doc.reserve_id();
    doc.root_mut()
        .sections
        .planes
        .insert(section, SectionPlane { name: "Horizontal cut".into(), origin: DVec3::Z * 0.7, normal: DVec3::Z });
    doc.root_mut().sections.active = Some(section);
    doc.root_mut().section_hatch = Some(Hatch { spacing: 0.03, ..Default::default() });
    let first = Sheet {
        name: "Plans and elevations".into(),
        title_block: "Rysmo | Window study | Revision A\nCafé / přízemí".into(),
        viewports: vec![
            Viewport { scale: 100., ..Default::default() },
            Viewport {
                name: "Front".into(),
                projection: Projection::Front,
                rect_mm: [155., 12., 130., 150.],
                scale: 100.,
                ..Default::default()
            },
        ],
        notes: vec![Note { text: "Scaled vector views\nPrint at 100%".into(), position_mm: [15., 170.], size_mm: 3. }],
        ..Default::default()
    };
    let second = Sheet {
        name: "Assembly detail".into(),
        viewports: vec![Viewport {
            name: "Isometric".into(),
            projection: Projection::Isometric,
            rect_mm: [12., 12., 270., 150.],
            scale: 50.,
            ..Default::default()
        }],
        ..first.clone()
    };
    doc.sheets = vec![first, second];
    let path = std::path::Path::new(&output);
    std::fs::write(path.join("editable-workflows.rysmo"), rysmo_io::save(&doc))?;
    std::fs::write(path.join("sheets.pdf"), rysmo_io::sheets::pdf(&doc, &doc.sheets)?)?;
    std::fs::write(path.join("sheet.svg"), rysmo_io::sheets::build(&doc, &doc.sheets[0])?.svg())?;
    Ok(())
}
