//! Work budgets shared by native loading, import and transactional editing.
use crate::{CommandError, DefinitionId, Document, Element};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum copies in one linear or rotational array.
pub const MAX_COPIES: u32 = 10_000;
const MAX_COPY_ITEMS: usize = 1_000_000;

#[derive(Clone, Copy, Default)]
struct Expansion {
    occurrences: usize,
    faces: usize,
    vertices: usize,
    depth: usize,
}
impl Expansion {
    fn add(&mut self, other: Self, copies: usize) -> Result<(), String> {
        self.occurrences = self.occurrences.saturating_add(other.occurrences.saturating_mul(copies));
        self.faces = self.faces.saturating_add(other.faces.saturating_mul(copies));
        self.vertices = self.vertices.saturating_add(other.vertices.saturating_mul(copies));
        self.check()
    }
    fn check(self) -> Result<(), String> {
        if self.occurrences > 100_000 || self.faces > 500_000 || self.vertices > 1_000_000 {
            return Err("Expanded model exceeds 100,000 occurrences, 500,000 faces or one million vertices.".into());
        }
        Ok(())
    }
}

#[derive(Default)]
struct Hierarchy {
    path: BTreeSet<Option<DefinitionId>>,
    memo: BTreeMap<Option<DefinitionId>, Expansion>,
    /// Prospective copies in one context, checked without allocating instances.
    addition: Option<(Option<DefinitionId>, Expansion)>,
}
impl Hierarchy {
    fn visit(&mut self, doc: &Document, id: Option<DefinitionId>) -> Result<Expansion, String> {
        if self.path.len() >= 64 || self.path.contains(&id) {
            return Err("Component hierarchy is cyclic (a definition contains itself) or exceeds 64 levels.".into());
        }
        if let Some(count) = self.memo.get(&id) {
            if self.path.len() + count.depth > 64 {
                return Err("Component hierarchy exceeds 64 levels.".into());
            }
            return Ok(*count);
        }
        self.path.insert(id);
        let definition = doc.def(id).ok_or("Missing component definition")?;
        let mut counts = Expansion {
            occurrences: 1,
            faces: definition.mesh.face_count(),
            vertices: definition.mesh.vertex_count(),
            depth: 1,
        };
        counts.check()?;
        for instance in definition.instances.values() {
            let child = self.visit(doc, Some(instance.definition))?;
            counts.add(child, 1)?;
            counts.depth = counts.depth.max(child.depth + 1);
        }
        if let Some((owner, addition)) = self.addition
            && owner == id
        {
            counts.add(addition, 1)?;
        }
        self.path.remove(&id);
        self.memo.insert(id, counts);
        Ok(counts)
    }
    fn validate(&mut self, doc: &Document) -> Result<(), String> {
        self.visit(doc, None)?;
        // Unused definitions can be placed or inspected later.
        for id in doc.definitions.keys() {
            self.visit(doc, Some(*id))?;
        }
        Ok(())
    }
}

/// Reject excessive work before entering a copy loop or allocating its result.
pub(crate) fn validate_copies(
    doc: &Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    copies: u32,
) -> Result<(), CommandError> {
    if copies > MAX_COPIES {
        return Err(CommandError::Invalid(format!("An array supports at most {MAX_COPIES} copies.")));
    }
    if copies == 0 {
        return Ok(());
    }
    let source = doc.def(def).ok_or_else(|| CommandError::Invalid("Missing copy context".into()))?;
    let vertices = source.mesh.vertices_of(elements).len();
    if vertices.saturating_add(elements.len()).saturating_mul(copies as usize) > MAX_COPY_ITEMS {
        return Err(CommandError::Invalid("Copy array exceeds one million elements. Use fewer copies.".into()));
    }
    let mut hierarchy = Hierarchy::default();
    let mut added = Expansion { vertices, ..Default::default() };
    for element in elements.iter().collect::<BTreeSet<_>>() {
        match element {
            Element::Instance(id) => {
                let instance =
                    source.instances.get(id).ok_or_else(|| CommandError::Invalid("Missing copy instance".into()))?;
                let child = hierarchy.visit(doc, Some(instance.definition)).map_err(CommandError::Invalid)?;
                added.add(child, 1).map_err(CommandError::Invalid)?;
            }
            Element::Face(_) => added.faces += 1,
            _ => {}
        }
    }
    let mut total = Expansion::default();
    total.add(added, copies as usize).map_err(CommandError::Invalid)?;
    Hierarchy { addition: Some((def, total)), ..Default::default() }.validate(doc).map_err(CommandError::Invalid)
}

/// Shared definitions may expand exponentially even when the file itself is small.
/// Validates every definition with memoization; never expands its occurrences.
pub fn validate_document(doc: &Document) -> Result<(), String> {
    Hierarchy::default().validate(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Definition, Editor, Instance, InstanceId};
    use rysmo_math::{DMat4, DVec3};

    fn instance(definition: u64) -> Instance {
        Instance {
            definition: DefinitionId(definition),
            transform: DMat4::IDENTITY,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
            attributes: Default::default(),
        }
    }
    fn hierarchy(levels: u64, branching: u64) -> Document {
        let mut doc = Document::new();
        for id in 1..=levels {
            let mut definition = Definition::default();
            if id < levels {
                for branch in 0..branching {
                    definition.instances.insert(InstanceId(1000 + id * branching + branch), instance(id + 1));
                }
            }
            doc.definitions.insert(DefinitionId(id), definition.into());
        }
        doc.root_mut().instances.insert(InstanceId(999), instance(1));
        doc.fix_next_id().unwrap();
        doc
    }
    #[test]
    fn bounded_shared_graphs_are_accepted_but_deep_and_unused_cycles_are_not() {
        assert!(validate_document(&hierarchy(16, 2)).is_ok());
        assert!(validate_document(&hierarchy(17, 2)).is_err());
        assert!(validate_document(&hierarchy(63, 1)).is_ok());
        assert!(validate_document(&hierarchy(64, 1)).is_err());
        let mut doc = hierarchy(2, 1);
        doc.root_mut().instances.clear();
        doc.def_mut(Some(DefinitionId(2))).unwrap().instances.insert(InstanceId(2000), instance(1));
        assert!(validate_document(&doc).is_err());
    }
    #[test]
    fn small_instance_array_is_rejected_before_expanding_shared_geometry() {
        let mut editor = Editor::with_document(hierarchy(16, 2));
        let before = editor.doc().clone();
        let result = editor.apply(Command::Copy {
            elements: vec![Element::Instance(InstanceId(999))],
            offset: DVec3::X,
            copies: 1,
        });
        assert!(result.is_err());
        assert_eq!(editor.doc(), &before);
        assert!(editor.undo_label().is_none());
    }
}
