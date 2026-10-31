//! Bounding volume trees for finding geometry near a place without visiting all of it.
//!
//! Model scales vary too widely for a uniform grid: a screw can sit in a site plan. A
//! tree of boxes adapts to that. Visiting order is unspecified, so callers that need
//! the order of the id-keyed maps sort what they collect.

use rysmo_math::{DVec3, Plane, Ray};

/// How many items a leaf holds.
const LEAF: usize = 6;
/// How many items a `DynamicBvh` scans one by one before filing them in a tree.
const TAIL: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub lo: DVec3,
    pub hi: DVec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { lo: DVec3::INFINITY, hi: DVec3::NEG_INFINITY };

    pub fn at(p: DVec3) -> Self {
        Aabb { lo: p, hi: p }
    }

    pub fn of(points: impl IntoIterator<Item = DVec3>) -> Self {
        points.into_iter().fold(Self::EMPTY, |b, p| Aabb { lo: b.lo.min(p), hi: b.hi.max(p) })
    }

    pub fn union(self, other: Aabb) -> Self {
        Aabb { lo: self.lo.min(other.lo), hi: self.hi.max(other.hi) }
    }

    pub fn expanded(self, margin: f64) -> Self {
        Aabb { lo: self.lo - DVec3::splat(margin), hi: self.hi + DVec3::splat(margin) }
    }

    pub fn corners(&self) -> [DVec3; 8] {
        std::array::from_fn(|k| {
            DVec3::new(
                if k & 1 == 0 { self.lo.x } else { self.hi.x },
                if k & 2 == 0 { self.lo.y } else { self.hi.y },
                if k & 4 == 0 { self.lo.z } else { self.hi.z },
            )
        })
    }

    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.lo.cmple(other.hi).all() && other.lo.cmple(self.hi).all()
    }

    /// Whether the box comes within `margin` of the plane.
    pub fn near_plane(&self, plane: &Plane, margin: f64) -> bool {
        let reach = ((self.hi - self.lo) * 0.5).dot(plane.normal.abs());
        plane.distance((self.lo + self.hi) * 0.5).abs() <= reach + margin
    }

    /// Where a ray enters the box, if it does before `limit`.
    pub fn entry(&self, ray: &Ray, limit: f64) -> Option<f64> {
        let (mut near, mut far) = (f64::NEG_INFINITY, limit);
        for axis in 0..3 {
            let inverse = 1.0 / ray.dir[axis];
            let a = (self.lo[axis] - ray.origin[axis]) * inverse;
            let b = (self.hi[axis] - ray.origin[axis]) * inverse;
            // A ray along a box side gives NaN, which leaves the interval open.
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
        (near <= far).then_some(near)
    }

    fn centre(&self, axis: usize) -> f64 {
        (self.lo[axis] + self.hi[axis]) * 0.5
    }
}

#[derive(Clone, Debug)]
struct Node {
    bounds: Aabb,
    /// The items of a leaf, or for an inner node the first child at `start` (the next
    /// node) and the second at `end`.
    start: u32,
    end: u32,
    leaf: bool,
}

/// A static tree of boxes.
#[derive(Clone, Debug)]
pub struct Bvh<T> {
    nodes: Vec<Node>,
    items: Vec<(Aabb, T)>,
}

impl<T> Default for Bvh<T> {
    fn default() -> Self {
        Bvh { nodes: Vec::new(), items: Vec::new() }
    }
}

impl<T> Bvh<T> {
    pub fn new(mut items: Vec<(Aabb, T)>) -> Self {
        let mut nodes = Vec::with_capacity(items.len() / LEAF * 2 + 1);
        if !items.is_empty() {
            let len = items.len();
            split(&mut nodes, &mut items, 0, len);
        }
        Bvh { nodes, items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Calls `found` for the items in every leaf reached by descending through the
    /// boxes `enter` accepts. `enter` also sees the item boxes.
    pub fn visit(&self, mut enter: impl FnMut(&Aabb) -> bool, mut found: impl FnMut(&T)) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0u32];
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index as usize];
            if !enter(&node.bounds) {
                continue;
            }
            if node.leaf {
                for (bounds, item) in &self.items[node.start as usize..node.end as usize] {
                    if enter(bounds) {
                        found(item);
                    }
                }
            } else {
                stack.push(node.end);
                stack.push(node.start);
            }
        }
    }

    pub fn overlapping(&self, area: Aabb, found: impl FnMut(&T)) {
        self.visit(|b| b.overlaps(&area), found);
    }

    /// Whether an item overlapping `area` passes `test`.
    pub fn any(&self, area: Aabb, mut test: impl FnMut(&T) -> bool) -> bool {
        let hit = std::cell::Cell::new(false);
        self.visit(|b| !hit.get() && b.overlaps(&area), |item| hit.set(test(item)));
        hit.get()
    }

    pub fn into_items(self) -> Vec<(Aabb, T)> {
        self.items
    }
}

fn split<T>(nodes: &mut Vec<Node>, items: &mut [(Aabb, T)], start: usize, end: usize) -> u32 {
    let index = nodes.len();
    let bounds = items[start..end].iter().fold(Aabb::EMPTY, |b, (x, _)| b.union(*x));
    nodes.push(Node { bounds, start: start as u32, end: end as u32, leaf: true });
    if end - start <= LEAF {
        return index as u32;
    }
    let size = bounds.hi - bounds.lo;
    let axis = if size.x >= size.y && size.x >= size.z {
        0
    } else if size.y >= size.z {
        1
    } else {
        2
    };
    let middle = (start + end) / 2;
    items[start..end].select_nth_unstable_by(middle - start, |a, b| a.0.centre(axis).total_cmp(&b.0.centre(axis)));
    let first = split(nodes, items, start, middle);
    let second = split(nodes, items, middle, end);
    nodes[index] = Node { bounds, start: first, end: second, leaf: false };
    index as u32
}

/// A tree that takes insertions: a few static trees of growing size, merged the way a
/// binary counter carries, so that an item is refiled O(log n) times. It does not
/// remove; callers check what it finds against the mesh.
#[derive(Clone, Debug)]
pub struct DynamicBvh<T> {
    trees: Vec<Bvh<T>>,
    tail: Vec<(Aabb, T)>,
}

impl<T> Default for DynamicBvh<T> {
    fn default() -> Self {
        DynamicBvh { trees: Vec::new(), tail: Vec::new() }
    }
}

impl<T> DynamicBvh<T> {
    pub fn new(items: Vec<(Aabb, T)>) -> Self {
        DynamicBvh { trees: vec![Bvh::new(items)], tail: Vec::new() }
    }

    pub fn insert(&mut self, bounds: Aabb, item: T) {
        self.tail.push((bounds, item));
        if self.tail.len() < TAIL {
            return;
        }
        let mut items = std::mem::take(&mut self.tail);
        while self.trees.last().is_some_and(|tree| tree.len() <= items.len() * 2) {
            items.extend(self.trees.pop().expect("just checked").into_items());
        }
        self.trees.push(Bvh::new(items));
    }

    pub fn overlapping(&self, area: Aabb, mut found: impl FnMut(&T)) {
        for tree in &self.trees {
            tree.overlapping(area, &mut found);
        }
        for (_, item) in self.tail.iter().filter(|(bounds, _)| bounds.overlaps(&area)) {
            found(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scattered(count: u64) -> Vec<(Aabb, u64)> {
        let mut state = 88172645463325252u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 2000) as f64 / 10.
        };
        (0..count)
            .map(|id| {
                let lo = DVec3::new(next(), next(), next());
                let size = next() / 50.;
                (Aabb { lo, hi: lo + DVec3::splat(size) }, id)
            })
            .collect()
    }

    fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
        ids.sort();
        ids
    }

    #[test]
    fn tree_finds_what_a_scan_finds() {
        let items = scattered(5000);
        let tree = Bvh::new(items.clone());
        for (area, _) in items.iter().step_by(97) {
            let area = area.expanded(3.);
            let scan: Vec<_> = items.iter().filter(|(b, _)| b.overlaps(&area)).map(|(_, id)| *id).collect();
            let mut found = Vec::new();
            tree.overlapping(area, |id| found.push(*id));
            assert_eq!(sorted(found), sorted(scan));
        }
    }

    #[test]
    fn dynamic_tree_finds_items_however_they_were_filed() {
        let items = scattered(3000);
        let mut tree = DynamicBvh::new(items[..1000].to_vec());
        for (bounds, id) in &items[1000..] {
            tree.insert(*bounds, *id);
        }
        for (area, _) in items.iter().step_by(83) {
            let area = area.expanded(3.);
            let scan: Vec<_> = items.iter().filter(|(b, _)| b.overlaps(&area)).map(|(_, id)| *id).collect();
            let mut found = Vec::new();
            tree.overlapping(area, |id| found.push(*id));
            assert_eq!(sorted(found), sorted(scan));
        }
    }

    #[test]
    fn plane_and_ray_tests_agree_with_the_box() {
        let unit = Aabb { lo: DVec3::ZERO, hi: DVec3::ONE };
        assert!(unit.near_plane(&Plane::new(DVec3::Z, DVec3::splat(0.5)), 0.));
        assert!(unit.near_plane(&Plane::new(DVec3::Z, DVec3::Z * 1.5), 0.6));
        assert!(!unit.near_plane(&Plane::new(DVec3::Z, DVec3::Z * 1.5), 0.1));
        let across = Ray::new(DVec3::new(-1., 0.5, 0.5), DVec3::X);
        assert_eq!(unit.entry(&across, f64::INFINITY), Some(1.));
        assert_eq!(unit.entry(&across, 0.5), None);
        assert_eq!(unit.entry(&Ray::new(DVec3::new(-1., 2., 0.5), DVec3::X), f64::INFINITY), None);
    }
}
