//! Compressed hierarchy edges and expansion counts, invalidated through ancestors.
use crate::{Definition, DefinitionId, Document, Instance, InstanceId};
use rysmo_mesh::blocks::BlockMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, Weak};
type Owner = Option<DefinitionId>;
#[derive(Clone, Default)]
pub(crate) struct Node {
    pub faces: usize,
    pub vertices: usize,
    pub children: BTreeMap<DefinitionId, usize>,
}
pub(crate) type Graph = BTreeMap<Owner, Node>;
struct Signature {
    source: Weak<Definition>,
    instances: BlockMap<InstanceId, Instance>,
    node: Node,
}
impl Signature {
    fn new(definition: &Arc<Definition>) -> Self {
        let mut node = Node {
            faces: definition.mesh.face_count(),
            vertices: definition.mesh.vertex_count(),
            ..Default::default()
        };
        for instance in definition.instances.values() {
            *node.children.entry(instance.definition).or_default() += 1;
        }
        Self { source: Arc::downgrade(definition), instances: definition.instances.clone(), node }
    }
    fn update(&mut self, definition: &Arc<Definition>) -> bool {
        if self.source.as_ptr() == Arc::as_ptr(definition) {
            return false;
        }
        let mut changed =
            self.node.faces != definition.mesh.face_count() || self.node.vertices != definition.mesh.vertex_count();
        self.node.faces = definition.mesh.face_count();
        self.node.vertices = definition.mesh.vertex_count();
        for id in definition.instances.changed_since_by(&self.instances, |a, b| a.definition == b.definition) {
            let old = self.instances.get(&id).map(|i| i.definition);
            let new = definition.instances.get(&id).map(|i| i.definition);
            if old == new {
                continue;
            }
            changed = true;
            if let Some(old) = old
                && let Some(count) = self.node.children.get_mut(&old)
            {
                *count -= 1;
                if *count == 0 {
                    self.node.children.remove(&old);
                }
            }
            if let Some(new) = new {
                *self.node.children.entry(new).or_default() += 1;
            }
        }
        self.source = Arc::downgrade(definition);
        self.instances = definition.instances.clone();
        changed
    }
}
#[derive(Default)]
struct Snapshot {
    definitions: BTreeMap<Owner, Signature>,
    graph: Graph,
    memo: BTreeMap<Owner, crate::limits::Expansion>,
    result: Option<Result<(), String>>,
}
#[derive(Default)]
pub(crate) struct Cache(Mutex<Snapshot>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HierarchyCache")
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
fn ancestors(graph: &Graph, changed: &BTreeSet<Owner>) -> BTreeSet<Owner> {
    let mut parents: BTreeMap<Owner, BTreeSet<Owner>> = BTreeMap::new();
    for (owner, node) in graph {
        for child in node.children.keys() {
            parents.entry(Some(*child)).or_default().insert(*owner);
        }
    }
    let mut affected = changed.clone();
    let mut pending: Vec<_> = changed.iter().copied().collect();
    while let Some(owner) = pending.pop() {
        if let Some(parents) = parents.get(&owner) {
            for parent in parents {
                if affected.insert(*parent) {
                    pending.push(*parent);
                }
            }
        }
    }
    affected
}
impl Cache {
    pub(crate) fn with_graph<R>(&self, doc: &Document, use_graph: impl FnOnce(&Graph) -> R) -> Result<R, String> {
        self.validate(doc)?;
        let snapshot = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(use_graph(&snapshot.graph))
    }
    pub fn validate(&self, doc: &Document) -> Result<(), String> {
        let mut snapshot = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut changed = BTreeSet::new();
        snapshot.definitions.retain(|owner, _| {
            let keep = doc.def(*owner).is_some();
            if !keep {
                changed.insert(*owner);
            }
            keep
        });
        for (owner, definition) in
            std::iter::once((None, &doc.root)).chain(doc.definitions.iter().map(|(id, d)| (Some(*id), d)))
        {
            match snapshot.definitions.entry(owner) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get_mut().update(definition) {
                        changed.insert(owner);
                    }
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(Signature::new(definition));
                    changed.insert(owner);
                }
            }
        }
        if changed.is_empty()
            && let Some(result) = &snapshot.result
        {
            return result.clone();
        }
        snapshot.result = None;
        let mut affected = ancestors(&snapshot.graph, &changed);
        snapshot.graph = snapshot.definitions.iter().map(|(owner, source)| (*owner, source.node.clone())).collect();
        affected.extend(ancestors(&snapshot.graph, &changed));
        snapshot.memo.retain(|owner, _| !affected.contains(owner));
        let Snapshot { graph, memo, .. } = &mut *snapshot;
        let result = crate::limits::validate_graph(doc, graph, memo);
        snapshot.result = Some(result.clone());
        result
    }
}
