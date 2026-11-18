//! Work budgets shared by native loading, import and transactional editing.
use crate::{CommandError, DefinitionId, Document, Element};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum copies in one linear or rotational array.
pub const MAX_COPIES: u32 = 10_000;
const MAX_COPY_ITEMS: usize = 1_000_000;

#[derive(Clone, Copy, Default)]
pub(crate) struct Expansion {
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
struct Hierarchy<'a> {
    graph: Option<&'a crate::hierarchy_cache::Graph>,
    path: BTreeSet<Option<DefinitionId>>,
    memo: BTreeMap<Option<DefinitionId>, Expansion>,
    /// Prospective copies in one context, checked without allocating instances.
    addition: Option<(Option<DefinitionId>, Expansion)>,
}
impl Hierarchy<'_> {
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
        let (faces, vertices, children) = if let Some(graph) = self.graph {
            let node = graph.get(&id).ok_or("Missing component definition")?;
            (node.faces, node.vertices, node.children.clone())
        } else {
            let definition = doc.def(id).ok_or("Missing component definition")?;
            let mut children = BTreeMap::new();
            for instance in definition.instances.values() {
                *children.entry(instance.definition).or_default() += 1;
            }
            (definition.mesh.face_count(), definition.mesh.vertex_count(), children)
        };
        let mut counts = Expansion { occurrences: 1, faces, vertices, depth: 1 };
        counts.check()?;
        for (definition, copies) in children {
            let child = self.visit(doc, Some(definition))?;
            counts.add(child, copies)?;
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
    doc.hierarchy_cache
        .with_graph(doc, |graph| {
            let mut hierarchy = Hierarchy { graph: Some(graph), ..Default::default() };
            let mut added = Expansion { vertices, ..Default::default() };
            for element in elements.iter().collect::<BTreeSet<_>>() {
                match element {
                    Element::Instance(id) => {
                        let instance = source
                            .instances
                            .get(id)
                            .ok_or_else(|| CommandError::Invalid("Missing copy instance".into()))?;
                        let child = hierarchy.visit(doc, Some(instance.definition)).map_err(CommandError::Invalid)?;
                        added.add(child, 1).map_err(CommandError::Invalid)?;
                    }
                    Element::Face(_) => added.faces += 1,
                    _ => {}
                }
            }
            let mut total = Expansion::default();
            total.add(added, copies as usize).map_err(CommandError::Invalid)?;
            Hierarchy { graph: Some(graph), addition: Some((def, total)), ..Default::default() }
                .validate(doc)
                .map_err(CommandError::Invalid)
        })
        .map_err(CommandError::Invalid)?
}

/// Shared definitions may expand exponentially even when the file itself is small.
/// Validates every definition with memoization; never expands its occurrences.
pub fn validate_document(doc: &Document) -> Result<(), String> {
    doc.hierarchy_cache.validate(doc)
}

pub(crate) fn validate_graph(
    doc: &Document,
    graph: &crate::hierarchy_cache::Graph,
    memo: &mut BTreeMap<Option<DefinitionId>, Expansion>,
) -> Result<(), String> {
    let mut hierarchy = Hierarchy { graph: Some(graph), memo: std::mem::take(memo), ..Default::default() };
    let result = hierarchy.validate(doc);
    if result.is_ok() {
        *memo = hierarchy.memo;
    }
    result
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

    #[test]
    fn cached_validation_tracks_graph_edits_unused_definitions_and_branches() {
        fn check(doc: &Document) {
            let expected = Hierarchy::default().validate(doc);
            assert_eq!(validate_document(doc), expected);
            assert_eq!(validate_document(doc), expected);
        }
        let mut doc = hierarchy(3, 2);
        check(&doc);
        let original = doc.clone();
        doc.root_mut().instances.get_mut(&InstanceId(999)).unwrap().hidden = true;
        check(&doc);
        // A same-sized target replacement creates a cycle even in an unused definition.
        doc.root_mut().instances.clear();
        doc.def_mut(Some(DefinitionId(3))).unwrap().instances.insert(InstanceId(2000), instance(1));
        check(&doc);
        assert!(validate_document(&doc).is_err());
        doc.def_mut(Some(DefinitionId(3))).unwrap().instances.clear();
        check(&doc);
        assert!(validate_document(&doc).is_ok());
        doc.def_mut(Some(DefinitionId(2))).unwrap().instances.values_mut().next().unwrap().definition = DefinitionId(9);
        check(&doc);
        assert!(validate_document(&doc).is_err());
        doc.definitions.insert(DefinitionId(9), Definition::default().into());
        check(&doc);
        assert!(validate_document(&doc).is_ok());
        doc.definitions.remove(&DefinitionId(3));
        check(&doc);
        assert!(validate_document(&doc).is_err());
        let mut branch = original.clone();
        branch.def_mut(Some(DefinitionId(3))).unwrap().instances.insert(InstanceId(3000), instance(3));
        check(&branch);
        check(&original);
        assert!(validate_document(&original).is_ok());
        let restored: Document = serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(restored, original);
        check(&restored);
        check(&hierarchy(64, 1));
        check(&hierarchy(17, 2));
    }

    #[test]
    fn cached_validation_rechecks_mesh_counts_and_rolls_back_excessive_edits() {
        let mut doc = hierarchy(1, 1);
        // An empty shared definition is cheap even with many placements.
        for id in 10_000..70_000 {
            doc.root_mut().instances.insert(InstanceId(id), instance(1));
        }
        doc.fix_next_id().unwrap();
        assert!(validate_document(&doc).is_ok());
        let mut editor = Editor::with_document(doc);
        editor.enter(InstanceId(999)).unwrap();
        let before = editor.doc().clone();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        // Six faces / eight vertices remain below the expanded budgets.
        editor.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
        let cube = editor.doc().clone();
        // A second extrusion exceeds the face budget. Intermediate states are valid.
        let outcome = editor
            .apply(Command::DrawRectangle { corner: DVec3::X * 3., u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let second = outcome
            .created
            .iter()
            .find_map(|element| match element {
                Element::Face(id) => Some(*id),
                _ => None,
            })
            .unwrap();
        assert!(editor.apply(Command::PushPull { face: second, distance: 1., keep_base: false }).is_err());
        assert!(editor.undo());
        assert_eq!(editor.doc(), &cube);
        assert!(editor.undo());
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn structural_cache_matches_cold_validation_after_repeated_graph_edits() {
        let mut doc = hierarchy(5, 2);
        for index in 0..120u64 {
            let owner = DefinitionId(index % 5 + 1);
            let id = InstanceId(5000 + index % 9);
            if index % 4 == 0 {
                doc.def_mut(Some(owner)).unwrap().instances.remove(&id);
            } else {
                doc.def_mut(Some(owner)).unwrap().instances.insert(id, instance((index * 3) % 7 + 1));
            }
            let expected = Hierarchy::default().validate(&doc);
            assert_eq!(validate_document(&doc), expected, "graph edit {index}");
            assert_eq!(validate_document(&doc), expected);
        }
    }
    #[test]
    #[ignore = "manual structural validation measurement"]
    fn benchmark_structural_validation() {
        use std::time::Instant;
        let mut doc = hierarchy(1, 1);
        for id in 10000..60000 {
            doc.root_mut().instances.insert(InstanceId(id), instance(1));
        }
        validate_document(&doc).unwrap();
        let (mut full, mut incremental) = (Vec::new(), Vec::new());
        for index in 0..44 {
            if index % 2 == 0 {
                doc.root_mut().instances.insert(InstanceId(70000), instance(1));
            } else {
                doc.root_mut().instances.remove(&InstanceId(70000));
            }
            let start = Instant::now();
            let expected = Hierarchy::default().validate(&doc);
            if index >= 3 {
                full.push(start.elapsed().as_secs_f64() * 1000.);
            }
            let start = Instant::now();
            let actual = validate_document(&doc);
            if index >= 3 {
                incremental.push(start.elapsed().as_secs_f64() * 1000.);
            }
            assert_eq!(actual, expected);
        }
        full.sort_by(f64::total_cmp);
        incremental.sort_by(f64::total_cmp);
        println!(
            "structural-validation: {}",
            serde_json::json!({"instances":50001,"samples":41,
            "cold_current_validator_median_ms":full[20],"cold_current_validator_p95_ms":full[38],
            "incremental_median_ms":incremental[20],"incremental_p95_ms":incremental[38]})
        );
    }
}
