//! Self-contained editable operation inputs, regenerated as atomic components.
//! Input IDs belong to the stored input document, not the containing model.
use crate::{
    Command, CommandError, Definition, DefinitionId, Document, Element, Instance, InstanceId,
    clipboard::{self, Clipboard},
    path_array::Options,
    solids::BooleanOp,
};
use rysmo_math::DMat4;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    Array { options: Options },
    Sweep,
    Solid { operation: BooleanOp, reverse: bool },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Procedure {
    pub input: Clipboard,
    pub operation: Operation,
    #[serde(default)]
    pub editing: bool,
}
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
pub fn validate(input: &Document, depth: usize, budget: &mut usize) -> Result<(), CommandError> {
    if depth > 4 || input.definitions.len() > 4096 {
        return Err(invalid("Live inputs exceed four procedural levels or 4096 definitions"));
    }
    let limit = u64::MAX - 2_000_000;
    if input.next_id() > limit
        || input.definitions.keys().any(|id| id.0 == 0 || id.0 > limit)
        || input.materials.keys().any(|id| id.0 == 0 || id.0 > limit)
        || input.images.keys().any(|id| id.0 == 0 || id.0 > limit)
        || input.tags.keys().any(|id| id.0 == 0 || id.0 > limit)
    {
        return Err(invalid("Live input identifier budget exceeded"));
    }
    let image_bytes = input.images.values().fold(0usize, |sum, image| sum.saturating_add(image.bytes.len()));
    if image_bytes > 8 * 1024 * 1024 {
        return Err(invalid("Live input images exceed 8 MiB"));
    }
    for def in std::iter::once(&input.root).chain(input.definitions.values()) {
        *budget = budget
            .checked_sub(
                def.mesh
                    .vertex_count()
                    .saturating_add(def.mesh.edge_count())
                    .saturating_add(def.mesh.face_count())
                    .saturating_add(def.instances.len()),
            )
            .ok_or_else(|| invalid("Live inputs exceed 200000 geometry elements"))?;
        if def.mesh.next_id() > limit
            || def.instances.keys().any(|id| id.0 == 0 || id.0 > limit)
            || def.guides.keys().any(|id| id.0 == 0 || id.0 > limit)
            || def.dimensions.keys().any(|id| id.0 == 0 || id.0 > limit)
            || def.texts.keys().any(|id| id.0 == 0 || id.0 > limit)
            || def.sections.planes.keys().any(|id| *id == 0 || *id > limit)
        {
            return Err(invalid("Live input identifier budget exceeded"));
        }
        def.mesh.validate()?;
        crate::component_data::validate_connections(&def.component.connections)?;
        crate::component_data::validate_attributes(&def.component.attributes)?;
        for instance in def.instances.values() {
            crate::component_data::validate_attributes(&instance.attributes)?;
        }
        if let Some(procedure) = &def.procedure {
            validate(&procedure.input.document, depth + 1, budget)?;
        }
    }
    let mut stack = vec![(None, Vec::new())];
    let mut remaining = 20_000usize;
    while let Some((owner, mut path)) = stack.pop() {
        remaining =
            remaining.checked_sub(1).ok_or_else(|| invalid("Live input expansion exceeds 20000 occurrences"))?;
        if path.len() > 32 {
            return Err(invalid("Live input hierarchy exceeds 32 levels"));
        }
        if let Some(id) = owner {
            if path.contains(&id) {
                return Err(invalid("Cyclic live input hierarchy"));
            }
            path.push(id);
        }
        let def = input.def(owner).ok_or_else(|| invalid("Missing live input definition"))?;
        for instance in def.instances.values() {
            if !instance.transform.is_finite() || !instance.transform.inverse().is_finite() {
                return Err(invalid("Invalid live input transform"));
            }
            stack.push((Some(instance.definition), path.clone()));
        }
        if stack.len() > 20_000 {
            return Err(invalid("Live input hierarchy budget exceeded"));
        }
    }
    Ok(())
}
pub fn evaluate(procedure: &Procedure) -> Result<Clipboard, CommandError> {
    validate(&procedure.input.document, 0, &mut 200_000)?;
    let mut doc = procedure.input.document.clone();
    // Paper layouts and scene state are not part of an operation's geometry.
    doc.sheets.clear();
    doc.scenes.clear();
    doc.camera = None;
    doc.photo_match = None;
    doc.fix_next_id();
    match &procedure.operation {
        Operation::Array { options } => {
            if doc.root.instances.len() != 1 || doc.root.mesh.face_count() != 0 {
                return Err(invalid("A live array needs one component and wire path edges"));
            }
            let source = *doc.root.instances.keys().next().unwrap();
            let path = doc.root.mesh.edges().map(|(id, _)| id).collect::<Vec<_>>();
            Command::ArrayAlongPath { source, path, options: *options, context: DMat4::IDENTITY }
                .run(&mut doc, None)?;
        }
        Operation::Sweep => {
            if doc.root.mesh.face_count() != 1 || !doc.root.instances.is_empty() {
                return Err(invalid("A live sweep needs exactly one profile face and separate path edges"));
            }
            let face = doc.root.mesh.faces().next().unwrap().0;
            let profile: BTreeSet<_> = doc.root.mesh.face_edges(face).into_iter().collect();
            let path = doc.root.mesh.edges().map(|(id, _)| id).filter(|id| !profile.contains(id)).collect();
            Command::FollowMe { face, path }.run(&mut doc, None)?;
        }
        Operation::Solid { operation, reverse } => {
            if doc.root.instances.len() != 2 || doc.root.mesh.edge_count() != 0 {
                return Err(invalid("A live solid operation needs two solid occurrences"));
            }
            let ids: Vec<_> = doc.root.instances.keys().copied().collect();
            let (left, right) = if *reverse { (ids[1], ids[0]) } else { (ids[0], ids[1]) };
            Command::SolidBoolean { left, right, operation: *operation, keep_inputs: false }.run(&mut doc, None)?;
        }
    }
    Ok(Clipboard { document: doc, source_transform: DMat4::IDENTITY })
}
fn install(doc: &mut Document, definition: DefinitionId, content: &Clipboard) -> Result<(), CommandError> {
    let target = doc.def_mut(Some(definition)).ok_or_else(|| invalid("Missing live component"))?;
    target.mesh = Default::default();
    target.instances.clear();
    target.guides.clear();
    target.dimensions.clear();
    target.texts.clear();
    target.sections = Default::default();
    clipboard::paste(doc, Some(definition), content, DMat4::IDENTITY, false)?;
    Ok(())
}
pub(crate) fn create(
    doc: &mut Document,
    owner: Option<DefinitionId>,
    input: Clipboard,
    operation: Operation,
    transform: DMat4,
    replace: Vec<Element>,
) -> Result<Vec<Element>, CommandError> {
    if !transform.is_finite() || !transform.inverse().is_finite() {
        return Err(invalid("Invalid live component placement"));
    }
    let procedure = Procedure { input, operation, editing: false };
    let generated = evaluate(&procedure)?;
    let definition = DefinitionId(doc.reserve_id());
    let id = InstanceId(doc.reserve_id());
    doc.definitions.insert(
        definition,
        Definition {
            name: match procedure.operation {
                Operation::Array { .. } => "Live Array",
                Operation::Sweep => "Live Sweep",
                Operation::Solid { .. } => "Live Solid",
            }
            .into(),
            ..Default::default()
        }
        .into(),
    );
    install(doc, definition, &generated)?;
    doc.def_mut(Some(definition)).unwrap().procedure = Some(Box::new(procedure));
    doc.def_mut(owner).ok_or_else(|| invalid("Missing live component context"))?.instances.insert(
        id,
        Instance {
            definition,
            transform,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
            attributes: Default::default(),
        },
    );
    if !replace.is_empty() {
        Command::Erase { elements: replace }.run(doc, owner)?;
    }
    Ok(vec![Element::Instance(id)])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit {
    BeginInputs,
    FinishInputs,
    CancelInputs,
    Regenerate,
    Detach,
}
pub(crate) fn update(
    doc: &mut Document,
    definition: DefinitionId,
    operation: Option<Operation>,
    edit: Edit,
) -> Result<(), CommandError> {
    let mut procedure = doc
        .definitions
        .get(&definition)
        .and_then(|d| d.procedure.as_deref())
        .cloned()
        .ok_or_else(|| invalid("Choose a live operation component"))?;
    if edit == Edit::Detach {
        doc.def_mut(Some(definition)).unwrap().procedure = None;
        return Ok(());
    }
    if let Some(operation) = operation {
        procedure.operation = operation;
    }
    match edit {
        Edit::BeginInputs => {
            if procedure.editing {
                return Err(invalid("Inputs are already being edited"));
            }
            install(doc, definition, &procedure.input)?;
            procedure.editing = true;
        }
        Edit::FinishInputs => {
            if !procedure.editing {
                return Err(invalid("Open input editing first"));
            }
            let mut input = doc.clone();
            let mut root = (**doc.definitions.get(&definition).unwrap()).clone();
            root.procedure = None;
            input.root = root.into();
            input.sheets.clear();
            input.scenes.clear();
            input.camera = None;
            input.photo_match = None;
            let mut keep = BTreeSet::new();
            let mut stack = input.root.instances.values().map(|i| i.definition).collect::<Vec<_>>();
            while let Some(id) = stack.pop() {
                if id == definition {
                    return Err(invalid("A procedure cannot contain itself"));
                }
                if keep.insert(id) {
                    let def = input.definitions.get(&id).ok_or_else(|| invalid("Missing input component"))?;
                    stack.extend(def.instances.values().map(|i| i.definition));
                }
                if keep.len() > 4096 || stack.len() > 20_000 {
                    return Err(invalid("Input hierarchy budget exceeded"));
                }
            }
            input.definitions.retain(|id, _| keep.contains(id));
            clipboard::retain_geometry_assets(&mut input);
            procedure.input = Clipboard { document: input, source_transform: DMat4::IDENTITY };
            let generated = evaluate(&procedure)?;
            install(doc, definition, &generated)?;
            procedure.editing = false;
        }
        Edit::CancelInputs | Edit::Regenerate => {
            if procedure.editing && edit == Edit::Regenerate {
                return Err(invalid("Finish or cancel input editing first"));
            }
            let generated = evaluate(&procedure)?;
            install(doc, definition, &generated)?;
            procedure.editing = false;
        }
        Edit::Detach => unreachable!(),
    }
    doc.def_mut(Some(definition)).unwrap().procedure = Some(Box::new(procedure));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Editor, path_array::Distribution};
    use rysmo_math::DVec3;
    fn inputs() -> Clipboard {
        let mut editor = Editor::new();
        let elements = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        editor.apply(Command::MakeComponent { elements, name: Some("Panel".into()) }).unwrap();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::X * 10., DVec3::X * 20.]], facing: None }).unwrap();
        Clipboard { document: editor.doc().clone(), source_transform: DMat4::IDENTITY }
    }
    fn operation(count: u32) -> Operation {
        Operation::Array { options: Options { distribution: Distribution::Count(count), ..Default::default() } }
    }
    #[test]
    fn live_array_parameters_inputs_cancel_and_undo_are_independent() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateProcedure {
                input: Box::new(inputs()),
                operation: operation(3),
                transform: DMat4::IDENTITY,
                replace: vec![],
            })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let definition = editor.doc().root.instances[&id].definition;
        assert_eq!(editor.doc().definitions[&definition].instances.len(), 4);
        let original = editor.doc().clone();
        editor
            .apply(Command::UpdateProcedure { definition, operation: Some(operation(7)), edit: Edit::Regenerate })
            .unwrap();
        assert_eq!(editor.doc().definitions[&definition].instances.len(), 8);
        editor.undo();
        assert_eq!(editor.doc(), &original);
        editor.apply(Command::UpdateProcedure { definition, operation: None, edit: Edit::BeginInputs }).unwrap();
        assert!(editor.doc().definitions[&definition].procedure.as_ref().unwrap().editing);
        assert_eq!(editor.doc().definitions[&definition].instances.len(), 1);
        editor.apply(Command::UpdateProcedure { definition, operation: None, edit: Edit::CancelInputs }).unwrap();
        assert_eq!(editor.doc().definitions[&definition].instances.len(), 4);
        assert!(!editor.doc().definitions[&definition].procedure.as_ref().unwrap().editing);
    }
    #[test]
    fn invalid_regeneration_does_not_modify_the_model() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateProcedure {
                input: Box::new(inputs()),
                operation: operation(3),
                transform: DMat4::IDENTITY,
                replace: vec![],
            })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let definition = editor.doc().root.instances[&id].definition;
        let before = editor.doc().clone();
        assert!(
            editor
                .apply(Command::UpdateProcedure { definition, operation: Some(operation(0)), edit: Edit::Regenerate })
                .is_err()
        );
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::UpdateProcedure { definition, operation: None, edit: Edit::Detach }).unwrap();
        assert!(editor.doc().definitions[&definition].procedure.is_none());
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn sweep_and_solid_recipes_rebuild_from_their_stored_inputs() {
        let mut sweep = Editor::new();
        sweep
            .apply(Command::DrawRectangle {
                corner: DVec3::new(0., -0.25, -0.25),
                u: DVec3::Y * 0.5,
                v: DVec3::Z * 0.5,
                facing: None,
            })
            .unwrap();
        sweep.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 3.]], facing: None }).unwrap();
        let recipe = Procedure {
            input: Clipboard { document: sweep.doc().clone(), source_transform: DMat4::IDENTITY },
            operation: Operation::Sweep,
            editing: false,
        };
        let output = evaluate(&recipe).unwrap();
        assert!(output.document.root.mesh.face_count() >= 6);
        assert_eq!(recipe.input.document.root.mesh.face_count(), 1);
        assert_eq!(evaluate(&recipe).unwrap().document, output.document);
        let mut solid =
            crate::parametric::Spec::Stairs { width: 1., rise: 1., tread: 1., steps: 1 }.document().unwrap();
        let first = solid.root.instances.values().next().unwrap().clone();
        let id = InstanceId(solid.reserve_id());
        let mut second = first;
        second.transform = DMat4::from_translation(DVec3::X * 0.5);
        solid.root_mut().instances.insert(id, second);
        let recipe = Procedure {
            input: Clipboard { document: solid, source_transform: DMat4::IDENTITY },
            operation: Operation::Solid { operation: BooleanOp::Union, reverse: false },
            editing: false,
        };
        let output = evaluate(&recipe).unwrap();
        assert_eq!(recipe.input.document.root.instances.len(), 2);
        assert_eq!(output.document.root.instances.len(), 1);
        let result = output.document.root.instances.values().next().unwrap();
        assert!((output.document.instance_volume(result, DMat4::IDENTITY).unwrap() - 1.5).abs() < 1e-6);
    }
    #[test]
    fn finishing_extended_path_inputs_replaces_the_saved_recipe() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateProcedure {
                input: Box::new(inputs()),
                operation: operation(3),
                transform: DMat4::IDENTITY,
                replace: vec![],
            })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let definition = editor.doc().root.instances[&id].definition;
        editor.apply(Command::UpdateProcedure { definition, operation: None, edit: Edit::BeginInputs }).unwrap();
        editor.enter(id).unwrap();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::X * 20., DVec3::X * 30.]], facing: None }).unwrap();
        editor.apply(Command::UpdateProcedure { definition, operation: None, edit: Edit::FinishInputs }).unwrap();
        let saved = editor.doc().definitions[&definition].procedure.as_ref().unwrap();
        assert!(!saved.editing);
        assert_eq!(saved.input.document.root.mesh.edge_count(), 2);
        assert_eq!(editor.doc().definitions[&definition].instances.len(), 4);
    }
}
