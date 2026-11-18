//! Derived membership index. Geometry validation still reads current definitions.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::{Definition, DefinitionId, Document, InstanceId};
use rysmo_mesh::blocks::BlockMap;

type Owner = Option<DefinitionId>;
pub(crate) type Owners = BTreeMap<Owner, Arc<BTreeSet<InstanceId>>>;

struct Snapshot {
    root: Arc<Definition>,
    definitions: BlockMap<DefinitionId, Arc<Definition>>,
    owners: Arc<Owners>,
}

#[derive(Default)]
pub(crate) struct Cache(Mutex<Option<Snapshot>>);
impl Clone for Cache {
    fn clone(&self) -> Self {
        // A procedure can contain a document cloned from its parent. Sharing this
        // cache with that input would let a cached definition point back to itself.
        // History snapshots rebuild their derived index on first use instead.
        Self::default()
    }
}
impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GlueIndex")
    }
}
impl PartialEq for Cache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

fn update(owners: &mut Arc<Owners>, owner: Owner, old: Option<&Definition>, new: Option<&Definition>) {
    let Some(new) = new else {
        if owners.contains_key(&owner) {
            Arc::make_mut(owners).remove(&owner);
        }
        return;
    };
    let ids: BTreeSet<_> = match old {
        Some(old) => {
            if new.instances.shares_storage_with(&old.instances) {
                return;
            }
            let changed =
                new.instances.changed_since_by(&old.instances, |a, b| a.glued_to.is_some() == b.glued_to.is_some());
            let membership: Vec<_> = changed
                .into_iter()
                .filter_map(|id| {
                    let was = old.instances.get(&id).is_some_and(|instance| instance.glued_to.is_some());
                    let now = new.instances.get(&id).is_some_and(|instance| instance.glued_to.is_some());
                    (was != now).then_some((id, now))
                })
                .collect();
            if membership.is_empty() {
                return;
            }
            let mut ids = owners.get(&owner).map(|ids| (**ids).clone()).unwrap_or_default();
            for (id, glued) in membership {
                if glued {
                    ids.insert(id);
                } else {
                    ids.remove(&id);
                }
            }
            ids
        }
        None => new.instances.iter().filter_map(|(id, instance)| instance.glued_to.map(|_| *id)).collect(),
    };
    if ids.is_empty() {
        if owners.contains_key(&owner) {
            Arc::make_mut(owners).remove(&owner);
        }
    } else {
        Arc::make_mut(owners).insert(owner, Arc::new(ids));
    }
}

impl Cache {
    pub fn seed(&self, doc: &Document, owners: Arc<Owners>) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Snapshot { root: doc.root.clone(), definitions: doc.definitions.clone(), owners });
    }
    pub fn clear(&self) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
    fn owners(&self, doc: &Document) -> Arc<Owners> {
        let mut cache = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(old) = &*cache
            && Arc::ptr_eq(&doc.root, &old.root)
            && doc.definitions.shares_storage_with(&old.definitions)
        {
            return old.owners.clone();
        }
        let mut owners = cache.as_ref().map(|old| old.owners.clone()).unwrap_or_default();
        update(&mut owners, None, cache.as_ref().map(|old| old.root.as_ref()), Some(&doc.root));
        let changed = cache
            .as_ref()
            .map(|old| doc.definitions.changed_since_by(&old.definitions, Arc::ptr_eq))
            .unwrap_or_else(|| doc.definitions.keys().copied().collect());
        for id in changed {
            update(
                &mut owners,
                Some(id),
                cache.as_ref().and_then(|old| old.definitions.get(&id)).map(Arc::as_ref),
                doc.definitions.get(&id).map(Arc::as_ref),
            );
        }
        *cache =
            Some(Snapshot { root: doc.root.clone(), definitions: doc.definitions.clone(), owners: owners.clone() });
        owners
    }
}

impl Document {
    pub(crate) fn glued_instances(&self) -> Arc<Owners> {
        self.glue_index.owners(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FaceId, Instance};

    fn instance(glued: bool) -> Instance {
        Instance {
            definition: DefinitionId(100_000),
            transform: Default::default(),
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: glued.then_some(FaceId(999_999)),
            attributes: Default::default(),
        }
    }
    fn check(document: &Document) {
        let scanned: Owners = std::iter::once((None, document.root.as_ref()))
            .chain(document.definitions.iter().map(|(id, definition)| (Some(*id), definition.as_ref())))
            .filter_map(|(owner, definition)| {
                let ids: BTreeSet<_> =
                    definition.instances.iter().filter_map(|(id, instance)| instance.glued_to.map(|_| *id)).collect();
                (!ids.is_empty()).then_some((owner, Arc::new(ids)))
            })
            .collect();
        assert_eq!(*document.glued_instances(), scanned);
    }

    #[test]
    fn membership_tracks_block_edits_removed_owners_and_snapshot_branches() {
        let mut document = Document::new();
        document.definitions.insert(DefinitionId(100_000), Definition::default().into());
        document.definitions.insert(DefinitionId(200_000), Definition::default().into());
        for id in 1..=1024 {
            document.root_mut().instances.insert(InstanceId(id), instance(id % 127 == 0));
            document
                .def_mut(Some(DefinitionId(200_000)))
                .unwrap()
                .instances
                .insert(InstanceId(2000 + id), instance(id % 255 == 0));
        }
        check(&document);
        let baseline = document.clone();
        document.root_mut().instances.get_mut(&InstanceId(256)).unwrap().glued_to = Some(FaceId(999_999));
        document.root_mut().instances.remove(&InstanceId(127));
        document.root_mut().instances.get_mut(&InstanceId(508)).unwrap().glued_to = None;
        check(&document);
        check(&baseline);
        check(&document);
        document.definitions.remove(&DefinitionId(200_000));
        check(&document);
        check(&baseline);
        let mut branch = baseline.clone();
        branch.root_mut().instances.clear();
        branch.def_mut(Some(DefinitionId(200_000))).unwrap().instances.get_mut(&InstanceId(2255)).unwrap().hidden =
            true;
        check(&branch);
        check(&document);
        check(&baseline);
    }

    #[test]
    fn index_is_derived_and_never_changes_equality_or_serialization() {
        let mut document = Document::new();
        document.definitions.insert(DefinitionId(100_000), Definition::default().into());
        document.root_mut().instances.insert(InstanceId(1), instance(true));
        let before = serde_json::to_string(&document).unwrap();
        check(&document);
        assert_eq!(serde_json::to_string(&document).unwrap(), before);
        let restored: Document = serde_json::from_str(&before).unwrap();
        assert_eq!(restored, document);
        assert!(!before.contains("glue_index"));
        check(&restored);
    }

    #[test]
    fn sparse_membership_storage_is_reused_when_only_instance_metadata_changes() {
        let mut document = Document::new();
        document.definitions.insert(DefinitionId(100_000), Definition::default().into());
        for id in 1..=1024 {
            document.root_mut().instances.insert(InstanceId(id), instance(id == 1));
        }
        let before = document.glued_instances();
        document.root_mut().instances.get_mut(&InstanceId(512)).unwrap().name = "Renamed".into();
        let after = document.glued_instances();
        assert!(Arc::ptr_eq(&before[&None], &after[&None]));
        check(&document);
    }
}
