//! Geometry dependencies of attached annotations. Derived state never enters history.
use crate::dimensions::{DimensionMeasure, PointReference};
use crate::text::{TextPlacement, TextReference};
use crate::{Definition, DefinitionId, Document, Element, Instance, InstanceId};
use rysmo_mesh::blocks::BlockMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, Weak};

type Owner = Option<DefinitionId>;
type Key = (Owner, Element);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Target {
    Definition,
    Instance(InstanceId),
    Vertex(crate::VertexId),
    Edge(crate::EdgeId),
    Face(crate::FaceId),
    Curve(rysmo_mesh::CurveId),
}
type Dependency = (Owner, Target);
pub(crate) type Targets = BTreeSet<Key>;
struct Source {
    identity: Weak<Definition>,
    mesh: crate::Mesh,
    instances: BlockMap<InstanceId, Instance>,
}
impl Source {
    fn new(definition: &Arc<Definition>) -> Self {
        Self {
            identity: Arc::downgrade(definition),
            mesh: definition.mesh.clone(),
            instances: definition.instances.clone(),
        }
    }
}
#[derive(Default)]
struct Index {
    empty: Option<BlockMap<DefinitionId, Arc<Definition>>>,
    sources: BTreeMap<Owner, Source>,
    dependencies: BTreeMap<Key, BTreeSet<Dependency>>,
    dependents: BTreeMap<Dependency, Targets>,
}
#[derive(Default)]
pub(crate) struct Cache(Mutex<Index>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnnotationCache")
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn path_dependencies(
    doc: &Document,
    mut owner: Owner,
    path: &[InstanceId],
    dependencies: &mut BTreeSet<Dependency>,
) -> Owner {
    dependencies.insert((owner, Target::Definition));
    for id in path {
        dependencies.insert((owner, Target::Instance(*id)));
        let Some(instance) = doc.def(owner).and_then(|definition| definition.instances.get(id)) else { return owner };
        owner = Some(instance.definition);
        dependencies.insert((owner, Target::Definition));
    }
    owner
}
fn point_dependency(doc: &Document, owner: Owner, reference: &PointReference, dependencies: &mut BTreeSet<Dependency>) {
    let target = path_dependencies(doc, owner, &reference.path, dependencies);
    match reference.entity {
        crate::dimensions::PointEntity::Vertex { vertex } => {
            dependencies.insert((target, Target::Vertex(vertex)));
        }
        crate::dimensions::PointEntity::Edge { edge, .. } => {
            dependencies.insert((target, Target::Edge(edge)));
            if let Some(edge) = doc.def(target).and_then(|d| d.mesh.edge(edge)) {
                for vertex in edge.v {
                    dependencies.insert((target, Target::Vertex(vertex)));
                }
            }
        }
        crate::dimensions::PointEntity::CurveCentre { curve } => {
            dependencies.insert((target, Target::Curve(curve)));
        }
    }
}
fn dependencies(doc: &Document, key: Key) -> BTreeSet<Dependency> {
    let (owner, element) = key;
    let mut result = BTreeSet::new();
    let Some(definition) = doc.def(owner) else { return result };
    match element {
        Element::Dimension(id) => {
            if let Some(dimension) = definition.dimensions.get(&id) {
                match &dimension.measure {
                    DimensionMeasure::Linear { from, to, .. } => {
                        for reference in [&from.reference, &to.reference].into_iter().flatten() {
                            point_dependency(doc, owner, reference, &mut result);
                        }
                    }
                    DimensionMeasure::Radial { reference, .. } => {
                        if let Some(curve) = reference.curve {
                            let target = path_dependencies(doc, owner, &reference.path, &mut result);
                            result.insert((target, Target::Curve(curve)));
                        }
                    }
                }
            }
        }
        Element::Text(id) => {
            if let Some(text) = definition.texts.get(&id)
                && let TextPlacement::Attached { anchor, .. } = &text.placement
                && let Some(reference) = &anchor.reference
            {
                match reference {
                    TextReference::Point { reference } => point_dependency(doc, owner, reference, &mut result),
                    TextReference::Face { path, face, vertices, .. } => {
                        let target = path_dependencies(doc, owner, path, &mut result);
                        result.insert((target, Target::Face(*face)));
                        for vertex in vertices {
                            result.insert((target, Target::Vertex(*vertex)));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    result
}

fn keys(owner: Owner, definition: &Definition) -> impl Iterator<Item = Key> + '_ {
    definition
        .dimensions
        .keys()
        .map(move |id| (owner, Element::Dimension(*id)))
        .chain(definition.texts.keys().map(move |id| (owner, Element::Text(*id))))
}
impl Index {
    fn remove(&mut self, key: Key) {
        if let Some(dependencies) = self.dependencies.remove(&key) {
            for dependency in dependencies {
                if let Some(keys) = self.dependents.get_mut(&dependency) {
                    keys.remove(&key);
                    if keys.is_empty() {
                        self.dependents.remove(&dependency);
                    }
                }
            }
        }
    }
    fn replace(&mut self, doc: &Document, key: Key) {
        self.remove(key);
        let dependencies = dependencies(doc, key);
        for dependency in &dependencies {
            self.dependents.entry(*dependency).or_default().insert(key);
        }
        self.dependencies.insert(key, dependencies);
    }
}
impl Cache {
    pub fn targets(&self, doc: &Document) -> Targets {
        let mut index = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if index.dependencies.is_empty() && doc.root.dimensions.is_empty() && doc.root.texts.is_empty() {
            let empty = if let Some(previous) = &index.empty {
                doc.definitions.changed_since_by(previous, Arc::ptr_eq).into_iter().all(|id| {
                    doc.definitions
                        .get(&id)
                        .is_none_or(|definition| definition.dimensions.is_empty() && definition.texts.is_empty())
                })
            } else {
                doc.definitions
                    .values()
                    .all(|definition| definition.dimensions.is_empty() && definition.texts.is_empty())
            };
            if empty {
                index.sources.clear();
                index.dependents.clear();
                index.empty = Some(doc.definitions.clone());
                return Targets::new();
            }
        }
        index.empty = None;
        let mut changed_geometry = BTreeSet::new();
        let mut changed_annotations = Targets::new();
        let removed: Vec<_> = index.sources.keys().filter(|owner| doc.def(**owner).is_none()).copied().collect();
        for owner in removed {
            changed_geometry.insert((owner, Target::Definition));
            let removed_keys: Vec<_> = index.dependencies.keys().filter(|key| key.0 == owner).copied().collect();
            for key in removed_keys {
                index.remove(key);
            }
            index.sources.remove(&owner);
        }
        for (owner, definition) in std::iter::once((None, &doc.root))
            .chain(doc.definitions.iter().map(|(id, definition)| (Some(*id), definition)))
        {
            let previous = index.sources.get(&owner);
            if previous.is_some_and(|source| source.identity.as_ptr() == Arc::as_ptr(definition)) {
                continue;
            }
            if let Some(source) = previous {
                let changed = definition.mesh.changed_storage_since(&source.mesh);
                changed_geometry.extend(changed.vertices.into_iter().map(|id| (owner, Target::Vertex(id))));
                changed_geometry.extend(changed.edges.into_iter().map(|id| (owner, Target::Edge(id))));
                changed_geometry.extend(changed.faces.into_iter().map(|id| (owner, Target::Face(id))));
                changed_geometry.extend(changed.curves.into_iter().map(|id| (owner, Target::Curve(id))));
                changed_geometry.extend(
                    definition
                        .instances
                        .changed_since_by(&source.instances, |a, b| {
                            a.definition == b.definition && a.transform == b.transform
                        })
                        .into_iter()
                        .map(|id| (owner, Target::Instance(id))),
                );
            } else {
                changed_geometry.insert((owner, Target::Definition));
            }
            let old = previous.and_then(|source| source.identity.upgrade());
            let candidates: Targets = keys(owner, definition)
                .chain(old.as_ref().into_iter().flat_map(|definition| keys(owner, definition)))
                .chain(index.dependencies.keys().filter(|key| key.0 == owner && old.is_none()).copied())
                .collect();
            for key in candidates {
                let exists = match key.1 {
                    Element::Dimension(id) => definition.dimensions.contains_key(&id),
                    Element::Text(id) => definition.texts.contains_key(&id),
                    _ => false,
                };
                let same = old.as_ref().is_some_and(|old| match key.1 {
                    Element::Dimension(id) => old.dimensions.get(&id) == definition.dimensions.get(&id),
                    Element::Text(id) => old.texts.get(&id) == definition.texts.get(&id),
                    _ => true,
                });
                if !exists {
                    index.remove(key);
                } else if !same {
                    index.replace(doc, key);
                    changed_annotations.insert(key);
                }
            }
            index.sources.insert(owner, Source::new(definition));
        }
        for owner in changed_geometry {
            if let Some(keys) = index.dependents.get(&owner) {
                changed_annotations.extend(keys);
            }
        }
        // A retargeted path may now reach a different definition. Register its new
        // targets even if resolving it only changes the retained fallback position.
        for key in &changed_annotations {
            index.replace(doc, *key);
        }
        changed_annotations
    }
    fn finish(&self, doc: &Document) {
        let mut index = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if index.empty.is_some() {
            return;
        }
        for (owner, definition) in std::iter::once((None, &doc.root))
            .chain(doc.definitions.iter().map(|(id, definition)| (Some(*id), definition)))
        {
            if index.sources.get(&owner).is_none_or(|source| source.identity.as_ptr() != Arc::as_ptr(definition)) {
                index.sources.insert(owner, Source::new(definition));
            }
        }
    }
}
impl Document {
    pub(crate) fn refresh_annotations(&mut self) {
        let targets = self.annotation_cache.targets(self);
        self.refresh_dimensions_selected(Some(&targets));
        self.refresh_texts_selected(Some(&targets));
        self.annotation_cache.finish(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimensions::PointEntity;
    use crate::text::{Text, TextAnchor, TextLeader};
    use rysmo_math::DVec3;
    #[test]
    fn individual_vertex_edits_invalidate_only_their_dependents() {
        let mut editor = crate::Editor::new();
        editor
            .apply(crate::Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::Y * 3., DVec3::new(1., 3., 0.)]],
                facing: None,
            })
            .unwrap();
        let mut doc = editor.doc().clone();
        let vertices: Vec<_> = doc.root.mesh.vertices().map(|(id, _)| id).collect();
        for (index, vertex) in vertices.iter().enumerate() {
            let id = crate::TextId(doc.reserve_id());
            doc.root_mut().texts.insert(
                id,
                Text {
                    content: index.to_string(),
                    placement: TextPlacement::Attached {
                        anchor: TextAnchor {
                            point: DVec3::ZERO,
                            reference: Some(TextReference::Point {
                                reference: PointReference {
                                    path: Vec::new(),
                                    entity: PointEntity::Vertex { vertex: *vertex },
                                },
                            }),
                        },
                        leader: TextLeader::None { offset: DVec3::ZERO },
                    },
                    font: Default::default(),
                    endpoint: Default::default(),
                },
            );
        }
        doc.refresh_annotations();
        assert!(doc.annotation_cache.targets(&doc).is_empty());
        let before = doc.clone();
        let edge = doc.root.mesh.edges().next().unwrap().0;
        let moved = doc.root.mesh.edge(edge).unwrap().v;
        doc.with_mesh(None, |mesh| mesh.move_elements(&[Element::Edge(edge)], DVec3::Z).unwrap());
        let selected = doc.annotation_cache.targets(&doc);
        assert_eq!(selected.len(), 2);
        for (_, element) in &selected {
            let Element::Text(id) = element else { panic!("text expected") };
            let text = &doc.root.texts[id];
            let TextPlacement::Attached { anchor, .. } = &text.placement else { panic!("attachment expected") };
            let Some(TextReference::Point { reference }) = &anchor.reference else { panic!("point expected") };
            let PointEntity::Vertex { vertex } = reference.entity else { panic!("vertex expected") };
            assert!(moved.contains(&vertex));
        }
        doc.refresh_dimensions_selected(Some(&selected));
        doc.refresh_texts_selected(Some(&selected));
        doc.annotation_cache.finish(&doc);
        let mut reference = before.clone();
        reference.with_mesh(None, |mesh| *mesh = doc.root.mesh.clone());
        reference.refresh_dimensions();
        reference.refresh_texts();
        assert_eq!(doc, reference);
        assert_eq!(before.root.texts.len(), doc.root.texts.len());
        let restored: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored, doc);
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::dimensions::PointEntity;
    use crate::text::{Text, TextAnchor, TextLeader};
    use rysmo_math::{DMat4, DVec3};
    fn check(doc: &mut Document) {
        let mut full = doc.clone();
        full.refresh_dimensions();
        full.refresh_texts();
        doc.refresh_annotations();
        assert_eq!(*doc, full);
    }
    #[test]
    fn retargeted_deleted_and_restored_paths_match_a_full_refresh() {
        let mut editor = crate::Editor::new();
        editor.apply(crate::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let mut doc = editor.doc().clone();
        let mesh = doc.root.mesh.clone();
        let vertex = mesh.vertices().next().unwrap().0;
        let a = DefinitionId(doc.reserve_id());
        let b = DefinitionId(doc.reserve_id());
        doc.definitions.insert(a, Definition { mesh, ..Default::default() }.into());
        doc.definitions.insert(b, Definition::default().into());
        doc.root_mut().mesh = crate::Mesh::new();
        let first = InstanceId(doc.reserve_id());
        let second = InstanceId(doc.reserve_id());
        for id in [first, second] {
            doc.root_mut().instances.insert(
                id,
                Instance {
                    definition: a,
                    transform: DMat4::IDENTITY,
                    name: String::new(),
                    tag: None,
                    material: None,
                    hidden: false,
                    glued_to: None,
                    attributes: Default::default(),
                },
            );
        }
        for instance in [first, second, InstanceId(99999)] {
            let id = crate::TextId(doc.reserve_id());
            doc.root_mut().texts.insert(
                id,
                Text {
                    content: instance.0.to_string(),
                    placement: TextPlacement::Attached {
                        anchor: TextAnchor {
                            point: DVec3::ZERO,
                            reference: Some(TextReference::Point {
                                reference: PointReference {
                                    path: vec![instance],
                                    entity: PointEntity::Vertex { vertex },
                                },
                            }),
                        },
                        leader: TextLeader::None { offset: DVec3::ZERO },
                    },
                    font: Default::default(),
                    endpoint: Default::default(),
                },
            );
        }
        check(&mut doc);
        let before = doc.clone();
        doc.root_mut().instances.get_mut(&first).unwrap().transform = DMat4::from_translation(DVec3::Y);
        let selected = doc.annotation_cache.targets(&doc);
        assert_eq!(selected.len(), 1);
        doc.refresh_dimensions_selected(Some(&selected));
        doc.refresh_texts_selected(Some(&selected));
        doc.annotation_cache.finish(&doc);
        check(&mut doc);
        drop(before);
        doc.root_mut().instances.get_mut(&first).unwrap().definition = b;
        check(&mut doc);
        doc.definitions.remove(&a);
        check(&mut doc);
        let restored_mesh = editor.mesh().clone();
        doc.definitions.insert(a, Definition { mesh: restored_mesh, ..Default::default() }.into());
        check(&mut doc);
        doc.root_mut().instances.get_mut(&first).unwrap().definition = a;
        check(&mut doc);
        doc.root_mut().instances.remove(&second);
        check(&mut doc);
    }
}
