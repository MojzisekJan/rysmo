//! Bounded polygon BSP clipping. Boolean composition follows Evan Wallace's
//! MIT-licensed csg.js (2011), https://github.com/evanw/csg.js . See licenses/csg-js.txt.
use crate::solids::BooleanOp;
use rysmo_math::{DVec3, Plane};
use rysmo_mesh::Face;
const TOL: f64 = 1e-7;
const LIMIT: usize = 8192;

#[derive(Clone)]
pub(super) struct Polygon {
    pub points: Vec<DVec3>,
    pub face: Face,
    plane: Plane,
}
impl Polygon {
    pub fn new(mut points: Vec<DVec3>, face: Face) -> Option<Self> {
        points.dedup_by(|a, b| a.distance(*b) < TOL);
        if points.len() > 1 && points[0].distance(*points.last().unwrap()) < TOL {
            points.pop();
        }
        let plane = Plane::from_polygon(&points)?;
        Some(Self { points, face, plane })
    }
    fn invert(&mut self) {
        self.points.reverse();
        self.plane.normal = -self.plane.normal;
        self.plane.d = -self.plane.d;
    }
}
#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}
struct Budget<'a>(&'a mut usize);
impl Budget<'_> {
    fn spend(&mut self, n: usize) -> Result<(), String> {
        *self.0 = self.0.checked_sub(n).ok_or("Solid operation exceeds its complexity limit")?;
        Ok(())
    }
}
fn split(
    plane: Plane,
    p: Polygon,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
    coplanar: &mut Vec<Polygon>,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    budget.spend(p.points.len())?;
    let sides = p
        .points
        .iter()
        .map(|v| {
            let d = plane.distance(*v);
            if d > TOL {
                1
            } else if d < -TOL {
                2
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    match sides.iter().fold(0, |a, b| a | b) {
        0 => {
            if plane.normal.dot(p.plane.normal) > 0. {
                coplanar.push(p)
            } else {
                back.push(p)
            }
        }
        1 => front.push(p),
        2 => back.push(p),
        _ => {
            let mut f = Vec::new();
            let mut b = Vec::new();
            for i in 0..p.points.len() {
                let j = (i + 1) % p.points.len();
                let a = p.points[i];
                let z = p.points[j];
                if sides[i] != 2 {
                    f.push(a);
                }
                if sides[i] != 1 {
                    b.push(a);
                }
                if sides[i] | sides[j] == 3 {
                    let t = (plane.d - plane.normal.dot(a)) / plane.normal.dot(z - a);
                    let v = a + (z - a) * t.clamp(0., 1.);
                    f.push(v);
                    b.push(v);
                }
            }
            if let Some(p) = Polygon::new(f, p.face.clone()) {
                front.push(p);
            }
            if let Some(p) = Polygon::new(b, p.face) {
                back.push(p);
            }
        }
    }
    Ok(())
}
impl Node {
    fn build(&mut self, polygons: Vec<Polygon>, depth: usize, budget: &mut Budget<'_>) -> Result<(), String> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > 128 || polygons.len() > LIMIT {
            return Err("Solid BSP exceeds its depth or polygon limit".into());
        }
        let plane = *self.plane.get_or_insert(polygons[0].plane);
        let mut front = Vec::new();
        let mut back = Vec::new();
        for polygon in polygons {
            // Coplanar polygons of either facing belong in this node during build.
            let mut opposite = Vec::new();
            if polygon.points.iter().all(|p| plane.distance(*p).abs() <= TOL) {
                self.polygons.push(polygon);
            } else {
                split(plane, polygon, &mut front, &mut back, &mut opposite, budget)?;
                self.polygons.extend(opposite);
            }
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front, depth + 1, budget)?;
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back, depth + 1, budget)?;
        }
        Ok(())
    }
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.invert();
        }
        if let Some(p) = &mut self.plane {
            p.normal = -p.normal;
            p.d = -p.d;
        }
        if let Some(n) = &mut self.front {
            n.invert();
        }
        if let Some(n) = &mut self.back {
            n.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip(&self, polygons: Vec<Polygon>, budget: &mut Budget<'_>) -> Result<Vec<Polygon>, String> {
        let Some(plane) = self.plane else { return Ok(polygons) };
        let mut front = Vec::new();
        let mut back = Vec::new();
        let mut same = Vec::new();
        for p in polygons {
            split(plane, p, &mut front, &mut back, &mut same, budget)?;
        }
        front.extend(same);
        if let Some(node) = &self.front {
            front = node.clip(front, budget)?;
        }
        if let Some(node) = &self.back {
            back = node.clip(back, budget)?;
        } else {
            back.clear();
        }
        front.extend(back);
        if front.len() > LIMIT {
            return Err("Solid clipping produces too many polygons".into());
        }
        Ok(front)
    }
    fn clip_to(&mut self, other: &Node, budget: &mut Budget<'_>) -> Result<(), String> {
        self.polygons = other.clip(std::mem::take(&mut self.polygons), budget)?;
        if let Some(n) = &mut self.front {
            n.clip_to(other, budget)?;
        }
        if let Some(n) = &mut self.back {
            n.clip_to(other, budget)?;
        }
        Ok(())
    }
    fn take(mut self) -> Vec<Polygon> {
        if let Some(n) = self.front.take() {
            self.polygons.extend(n.take());
        }
        if let Some(n) = self.back.take() {
            self.polygons.extend(n.take());
        }
        self.polygons
    }
}
pub(super) fn boolean_with_budget(
    left: Vec<Polygon>,
    right: Vec<Polygon>,
    op: BooleanOp,
    remaining: &mut usize,
) -> Result<Vec<Polygon>, String> {
    let mut budget = Budget(remaining);
    let mut a = Node::default();
    let mut b = Node::default();
    a.build(left, 0, &mut budget)?;
    b.build(right, 0, &mut budget)?;
    match op {
        BooleanOp::Union => {
            a.clip_to(&b, &mut budget)?;
            b.clip_to(&a, &mut budget)?;
            b.invert();
            b.clip_to(&a, &mut budget)?;
            b.invert();
        }
        BooleanOp::Subtract => {
            a.invert();
            a.clip_to(&b, &mut budget)?;
            b.clip_to(&a, &mut budget)?;
            b.invert();
            b.clip_to(&a, &mut budget)?;
            b.invert();
        }
        BooleanOp::Trim | BooleanOp::Split | BooleanOp::OuterShell => {
            return Err("Composite solid operation must be staged by the caller".into());
        }
        BooleanOp::Intersect => {
            a.invert();
            b.clip_to(&a, &mut budget)?;
            b.invert();
            a.clip_to(&b, &mut budget)?;
            b.clip_to(&a, &mut budget)?;
        }
    }
    a.build(b.take(), 0, &mut budget)?;
    if op != BooleanOp::Union {
        a.invert();
    }
    let out = a.take();
    if out.len() > LIMIT {
        return Err("Solid result exceeds its polygon limit".into());
    }
    Ok(out)
}
