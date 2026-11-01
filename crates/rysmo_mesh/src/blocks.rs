//! An ordered map whose clones share storage.
//!
//! Undo keeps a document snapshot per step and an edit usually touches a little of a
//! mesh, so entries live in small blocks behind `Arc`. Cloning copies the block list;
//! writing copies only the block written to.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::{EdgeId, FaceId, InstanceId, VertexId};

/// Log2 of the id range one block covers.
const BLOCK_BITS: u32 = 8;

/// Keys sort by block first, so a block holds a contiguous run of keys.
pub trait BlockKey: Copy + Ord {
    fn block(&self) -> u64;
}

macro_rules! block_key {
    ($($name:ty),*) => {
        $(impl BlockKey for $name {
            fn block(&self) -> u64 {
                self.0 >> BLOCK_BITS
            }
        })*
    };
}

block_key!(VertexId, EdgeId, FaceId, InstanceId, crate::CurveId, crate::DefinitionId);

impl BlockKey for (VertexId, VertexId) {
    fn block(&self) -> u64 {
        self.0.block()
    }
}

/// Sorted keys beside their values. Ids are handed out in order, so inserts mostly
/// append, and a scan over the values never walks over the keys.
#[derive(Clone, Debug, PartialEq)]
struct Block<K, V> {
    keys: Vec<K>,
    values: Vec<V>,
}

impl<K, V> Default for Block<K, V> {
    fn default() -> Self {
        Block { keys: Vec::new(), values: Vec::new() }
    }
}

impl<K: Ord, V> Block<K, V> {
    fn get(&self, key: &K) -> Option<&V> {
        self.keys.binary_search(key).ok().map(|at| &self.values[at])
    }

    fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + Clone {
        self.keys.iter().zip(&self.values)
    }
}

#[derive(Clone, Debug)]
pub struct BlockMap<K, V> {
    /// Sorted by block number; no block is empty.
    blocks: Vec<(u64, Arc<Block<K, V>>)>,
    len: usize,
}

impl<K, V> Default for BlockMap<K, V> {
    fn default() -> Self {
        BlockMap { blocks: Vec::new(), len: 0 }
    }
}

impl<K: BlockKey, V> BlockMap<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn slot(&self, key: &K) -> Result<usize, usize> {
        let block = key.block();
        self.blocks.binary_search_by_key(&block, |(number, _)| *number)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.blocks[self.slot(key).ok()?].1.get(key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + Clone {
        self.blocks.iter().flat_map(|(_, block)| block.iter())
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &K> + Clone {
        self.iter().map(|(key, _)| key)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + Clone {
        self.iter().map(|(_, value)| value)
    }
}

impl<K: BlockKey, V: PartialEq> BlockMap<K, V> {
    /// The keys added, removed or changed since `older`. Blocks the two share are
    /// skipped without looking inside, so the cost follows the size of the change.
    pub fn changed_since(&self, older: &Self) -> Vec<K> {
        let mut changed = Vec::new();
        let (mut new, mut old) = (self.blocks.iter().peekable(), older.blocks.iter().peekable());
        loop {
            let order = match (new.peek(), old.peek()) {
                (None, None) => return changed,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some((a, _)), Some((b, _))) => a.cmp(b),
            };
            match order {
                std::cmp::Ordering::Less => {
                    changed.extend(new.next().into_iter().flat_map(|(_, x)| x.keys.iter().copied()))
                }
                std::cmp::Ordering::Greater => {
                    changed.extend(old.next().into_iter().flat_map(|(_, x)| x.keys.iter().copied()))
                }
                std::cmp::Ordering::Equal => {
                    let ((_, x), (_, y)) = (new.next().expect("peeked"), old.next().expect("peeked"));
                    if !Arc::ptr_eq(x, y) {
                        changed.extend(x.iter().filter(|(k, v)| y.get(k) != Some(v)).map(|(k, _)| *k));
                        changed.extend(y.keys.iter().filter(|k| x.get(k).is_none()).copied());
                    }
                }
            }
        }
    }
}

impl<K: BlockKey + Clone, V: Clone> BlockMap<K, V> {
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        let slot = self.slot(key).ok()?;
        let block = &mut self.blocks[slot].1;
        let at = block.keys.binary_search(key).ok()?;
        Some(&mut Arc::make_mut(block).values[at])
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let slot = match self.slot(&key) {
            Ok(slot) => slot,
            Err(slot) => {
                self.blocks.insert(slot, (key.block(), Arc::default()));
                slot
            }
        };
        let block = Arc::make_mut(&mut self.blocks[slot].1);
        match block.keys.binary_search(&key) {
            Ok(at) => Some(std::mem::replace(&mut block.values[at], value)),
            Err(at) => {
                block.keys.insert(at, key);
                block.values.insert(at, value);
                self.len += 1;
                None
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let slot = self.slot(key).ok()?;
        let at = self.blocks[slot].1.keys.binary_search(key).ok()?;
        let block = Arc::make_mut(&mut self.blocks[slot].1);
        block.keys.remove(at);
        let old = block.values.remove(at);
        self.len -= 1;
        if self.blocks[slot].1.keys.is_empty() {
            self.blocks.remove(slot);
        }
        Some(old)
    }

    /// Writes to every block. Prefer `get_mut` for what actually changes, which keeps
    /// the other blocks shared with older snapshots.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.blocks.iter_mut().flat_map(|(_, block)| {
            let block = Arc::make_mut(block);
            block.keys.iter().zip(&mut block.values)
        })
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.iter_mut().map(|(_, value)| value)
    }

    /// Keeps the entries `keep` accepts. Blocks with nothing to drop stay shared.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        for (_, block) in &mut self.blocks {
            let kept: Vec<bool> = block.iter().map(|(key, value)| keep(key, value)).collect();
            let dropped = kept.iter().filter(|kept| !**kept).count();
            if dropped > 0 {
                let block = Arc::make_mut(block);
                let mut flags = kept.iter();
                block.keys.retain(|_| *flags.next().expect("a flag per key"));
                let mut flags = kept.iter();
                block.values.retain(|_| *flags.next().expect("a flag per value"));
                self.len -= dropped;
            }
        }
        self.blocks.retain(|(_, block)| !block.keys.is_empty());
    }
}

impl<K: BlockKey, V> std::ops::Index<&K> for BlockMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("no entry for this key")
    }
}

impl<K: BlockKey, V: PartialEq> PartialEq for BlockMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len
            && self.blocks.len() == other.blocks.len()
            && self.blocks.iter().zip(&other.blocks).all(|((a, x), (b, y))| a == b && (Arc::ptr_eq(x, y) || x == y))
    }
}

impl<K: BlockKey + Clone, V: Clone> FromIterator<(K, V)> for BlockMap<K, V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(entries: I) -> Self {
        let mut map = Self::new();
        for (key, value) in entries {
            map.insert(key, value);
        }
        map
    }
}

impl<K: BlockKey + Clone, V: Clone, const N: usize> From<[(K, V); N]> for BlockMap<K, V> {
    fn from(entries: [(K, V); N]) -> Self {
        entries.into_iter().collect()
    }
}

impl<'a, K: BlockKey, V> IntoIterator for &'a BlockMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = (&'a K, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<K: BlockKey + Serialize, V: Serialize> Serialize for BlockMap<K, V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.iter())
    }
}

impl<'de, K: BlockKey + Deserialize<'de>, V: Clone + Deserialize<'de>> Deserialize<'de> for BlockMap<K, V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(BTreeMap::<K, V>::deserialize(deserializer)?.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(keys: impl IntoIterator<Item = u64>) -> BlockMap<VertexId, u64> {
        keys.into_iter().map(|k| (VertexId(k), k)).collect()
    }

    #[test]
    fn behaves_like_an_ordered_map() {
        let mut model = BTreeMap::new();
        let mut blocks = BlockMap::new();
        let mut state = 2463534242u64;
        for step in 0..20_000u64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let key = VertexId(state % 4000);
            if step % 3 == 2 {
                assert_eq!(blocks.remove(&key), model.remove(&key));
            } else {
                assert_eq!(blocks.insert(key, step), model.insert(key, step));
            }
            assert_eq!(blocks.len(), model.len());
        }
        assert!(blocks.iter().eq(model.iter()));
        assert!(blocks.iter().rev().eq(model.iter().rev()));
        blocks.retain(|key, _| key.0 % 2 == 0);
        model.retain(|key, _| key.0 % 2 == 0);
        assert!(blocks.iter().eq(model.iter()));
        assert_eq!(blocks.len(), model.len());
    }

    #[test]
    fn a_clone_shares_every_block_it_does_not_write() {
        let original = map(0..10_000);
        let mut copy = original.clone();
        *copy.get_mut(&VertexId(5)).unwrap() = 99;
        copy.insert(VertexId(20_000), 1);
        copy.retain(|key, _| key.0 != 7);
        copy.retain(|_, _| true);
        let shared =
            original.blocks.iter().filter(|(n, x)| copy.blocks.iter().any(|(m, y)| m == n && Arc::ptr_eq(x, y)));
        assert_eq!(shared.count(), original.blocks.len() - 1);
        assert_eq!(original[&VertexId(5)], 5);
        assert_eq!(original.len(), 10_000);
        assert_eq!(copy.len(), 10_000);
    }

    #[test]
    fn reports_exactly_what_changed() {
        let original = map((0..3000).filter(|k| k % 2 == 0));
        let mut copy = original.clone();
        *copy.get_mut(&VertexId(4)).unwrap() = 99;
        copy.insert(VertexId(5), 5);
        copy.remove(&VertexId(2000));
        copy.insert(VertexId(90_000), 1);
        let mut changed = copy.changed_since(&original);
        changed.sort();
        assert_eq!(changed, [4, 5, 2000, 90_000].map(VertexId));
        assert!(original.changed_since(&original.clone()).is_empty());
    }

    #[test]
    fn stores_as_a_plain_map() {
        let original = map([1, 700, 70_000]);
        let plain: BTreeMap<_, _> = original.iter().map(|(k, v)| (*k, *v)).collect();
        let text = serde_json::to_string(&original).unwrap();
        assert_eq!(text, serde_json::to_string(&plain).unwrap());
        assert_eq!(serde_json::from_str::<BlockMap<VertexId, u64>>(&text).unwrap(), original);
    }
}
