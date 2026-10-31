//! Timings of whole-model operations on scenes of many objects.
//!
//!     cargo run --release -p rysmo_tools --example perf -- 250 1000 4000
//!
//! `raw` keeps every box in the model's own mesh, `groups` gives each box its own group
//! and `component` places one shared box many times. An operation that passes the time
//! limit is skipped at the larger sizes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rysmo_math::{DMat4, DVec2, DVec3, OrbitCamera, View};
use rysmo_mesh::{DefinitionId, Edge, EdgeId, Element, Face, FaceId, InstanceId, Mesh, Vertex, VertexId};
use rysmo_model::{Command, Definition, Document, Editor, Instance};
use rysmo_tools::{Ctx, Modifiers, Pointer, ToolBox, ToolKind};

const LIMIT: Duration = Duration::from_secs(10);
const LOOPS: [[usize; 4]; 6] = [[3, 2, 1, 0], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]];

/// `count` unit boxes on a grid with gaps, with ids starting at one.
fn boxes(count: usize) -> Mesh {
    let (mut vertices, mut edges, mut faces) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let side = (count as f64).sqrt().ceil() as usize;
    let mut id = 0;
    let mut next = || {
        id += 1;
        id
    };
    for n in 0..count {
        let base = DVec3::new((n % side) as f64 * 2., (n / side) as f64 * 2., 0.);
        let v: Vec<VertexId> = (0..8)
            .map(|k| {
                let id = VertexId(next());
                let corner = DVec3::new((k & 1 ^ k >> 1 & 1) as f64, (k >> 1 & 1) as f64, (k >> 2) as f64);
                vertices.insert(id, Vertex { pos: base + corner });
                id
            })
            .collect();
        let mut seen = BTreeSet::new();
        for l in LOOPS {
            for k in 0..4 {
                let (a, b) = (v[l[k]], v[l[(k + 1) % 4]]);
                if seen.insert((a.min(b), a.max(b))) {
                    edges.insert(EdgeId(next()), Edge::new(a, b));
                }
            }
            let outer = l.iter().map(|k| v[*k]).collect();
            let face = Face { hidden: false, outer, holes: Vec::new(), material: None, back_material: None, uv: None };
            faces.insert(FaceId(next()), face);
        }
    }
    Mesh::from_parts(vertices, edges, faces).expect("valid boxes")
}

fn raw(count: usize) -> Editor {
    let mut doc = Document::new();
    doc.with_mesh(None, |mesh| *mesh = boxes(count));
    Editor::with_document(doc)
}

fn instanced(count: usize, shared: bool) -> Editor {
    let mut doc = Document::new();
    let side = (count as f64).sqrt().ceil() as usize;
    let mut definition = DefinitionId(0);
    for n in 0..count {
        if n == 0 || !shared {
            definition = DefinitionId(doc.reserve_id());
            let mut mesh = boxes(1).with_fresh_ids(|| doc.reserve_id()).expect("fresh ids");
            mesh.ensure_next_id(doc.reserve_id());
            let def = Definition { name: "Box".into(), group: !shared, mesh, ..Default::default() };
            doc.definitions.insert(definition, Arc::new(def));
        }
        let at = DVec3::new((n % side) as f64 * 2., (n / side) as f64 * 2., 0.);
        let instance = Instance {
            definition,
            transform: DMat4::from_translation(at),
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
            attributes: Default::default(),
        };
        let id = InstanceId(doc.reserve_id());
        Arc::make_mut(&mut doc.root).instances.insert(id, instance);
    }
    Editor::with_document(doc)
}

/// `count` cylinders of 24 sides, with curves and smooth sides, drawn by commands.
fn cylinders(count: usize) -> Editor {
    let side = ((count as f64).sqrt().ceil() as u32).max(2);
    let mut editor = Editor::new();
    let circle = Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 0.5, segments: 24, start: None };
    editor.apply(circle).expect("a circle");
    let face = editor.mesh().faces().next().expect("a face").0;
    editor.apply(Command::PushPull { face, distance: 1., keep_base: false }).expect("a cylinder");
    let row = everything(&editor);
    editor.apply(Command::Copy { elements: row, offset: DVec3::X * 2., copies: side - 1 }).expect("a row");
    let all = everything(&editor);
    editor.apply(Command::Copy { elements: all, offset: DVec3::Y * 2., copies: side - 1 }).expect("rows");
    editor
}

fn everything(editor: &Editor) -> Vec<Element> {
    let def = editor.active();
    let geometry = def.mesh.faces().map(|(id, _)| Element::Face(id));
    let geometry = geometry.chain(def.mesh.edges().map(|(id, _)| Element::Edge(id)));
    geometry.chain(def.instances.keys().map(|id| Element::Instance(*id))).collect()
}

/// One object: a box's faces and edges, or one instance.
fn one(editor: &Editor) -> Vec<Element> {
    let all = everything(editor);
    if editor.mesh().face_count() == 0 { all[..1].to_vec() } else { select_box(editor) }
}

fn select_box(editor: &Editor) -> Vec<Element> {
    let mesh = editor.mesh();
    let (first, _) = mesh.faces().next().expect("a face");
    let mut out: Vec<_> = mesh.face_edges(first).into_iter().map(Element::Edge).collect();
    out.push(Element::Face(first));
    out
}

fn view(count: usize) -> View {
    let side = (count as f64).sqrt().ceil() * 2.;
    let camera =
        OrbitCamera { target: DVec3::new(side / 2., side / 2., 0.), distance: side * 1.5, ..Default::default() };
    camera.view(DVec2::new(1600., 1000.))
}

struct Bench {
    over: BTreeSet<&'static str>,
}

impl Bench {
    fn time(&mut self, scene: &'static str, name: &'static str, mut run: impl FnMut()) {
        let key: &'static str = Box::leak(format!("{scene}/{name}").into_boxed_str());
        if self.over.contains(key) {
            println!("  {name:<24} skipped");
            return;
        }
        // The best of several, because the first run can pay for caches the rest reuse.
        let begun = Instant::now();
        let mut each = Duration::MAX;
        let mut rounds = 0;
        while rounds < 3 || begun.elapsed() < Duration::from_millis(100) {
            let start = Instant::now();
            run();
            each = each.min(start.elapsed());
            rounds += 1;
        }
        println!("  {name:<24} {:>12.3} ms", each.as_secs_f64() * 1e3);
        if each > LIMIT {
            self.over.insert(key);
        }
    }
}

fn scene(bench: &mut Bench, name: &'static str, count: usize, editor: &Editor) {
    let doc = editor.doc();
    println!(
        "{name} {count}: {} vertices, {} faces, {} instances",
        editor.mesh().vertex_count(),
        editor.mesh().face_count(),
        doc.root.instances.len()
    );
    let all = everything(editor);
    let single = one(editor);
    let offset = DVec3::new(0.1, 0.05, 0.03);
    let far = DVec3::new(0., 0., 5.);
    let apply = |command: Command| {
        let mut e = editor.clone();
        e.apply(command).expect("command runs");
    };
    bench.time(name, "clone editor", || drop(editor.clone()));
    bench.time(name, "occurrences", || drop(doc.occurrences(&[])));
    bench.time(name, "move one", || apply(Command::Move { elements: single.clone(), offset }));
    bench.time(name, "move all", || apply(Command::Move { elements: all.clone(), offset }));
    bench.time(name, "rotate all", || {
        apply(Command::Rotate { elements: all.clone(), centre: DVec3::ZERO, axis: DVec3::Z, angle: 15., copies: 0 })
    });
    if let Some((edge, _)) = editor.mesh().edges().next() {
        let elements = vec![Element::Edge(edge)];
        bench.time(name, "move an edge", || {
            apply(Command::Move { elements: elements.clone(), offset: DVec3::new(0.3, 0.2, 0.1) })
        });
    }
    bench.time(name, "copy one", || apply(Command::Copy { elements: single.clone(), offset: far, copies: 1 }));
    bench.time(name, "copy all", || apply(Command::Copy { elements: all.clone(), offset: far, copies: 1 }));
    bench.time(name, "erase all", || apply(Command::Erase { elements: all.clone() }));
    bench.time(name, "draw a line", || {
        apply(Command::DrawLines { lines: vec![[DVec3::new(-5., -5., 0.), DVec3::new(-4., -5., 0.)]], facing: None })
    });
    bench.time(name, "move, undo, redo", || {
        let mut e = editor.clone();
        e.apply(Command::Move { elements: single.clone(), offset }).expect("command runs");
        e.undo();
        e.redo();
    });
    bench.time(name, "select all", || {
        let mut e = editor.clone();
        e.set_selection(all.iter().copied());
    });
    let view = view(count);
    let at = |cursor| Ctx { view: &view, cursor, modifiers: Modifiers::default(), scale: 1. };
    let mut line = editor.clone();
    let mut tools = ToolBox::default();
    tools.set_tool(ToolKind::Line);
    bench.time(name, "hover with Line", || {
        tools.pointer(&mut line, &at(DVec2::new(800., 500.)), Pointer::Move);
    });
    let mut edited = editor.clone();
    let mut steps = 0;
    let mut spent = Duration::ZERO;
    tools.pointer(&mut edited, &at(DVec2::new(800., 500.)), Pointer::Move);
    while steps < 5 {
        steps += 1;
        let step = DVec3::Z * 3. * if steps % 2 == 0 { -1. } else { 1. };
        edited.apply(Command::Move { elements: single.clone(), offset: step }).expect("command runs");
        let start = Instant::now();
        tools.pointer(&mut edited, &at(DVec2::new(800., 500.)), Pointer::Move);
        spent += start.elapsed();
    }
    println!("  {:<24} {:>12.3} ms", "hover after an edit", spent.as_secs_f64() * 1e3 / steps as f64);
    bench.time(name, "smooth normals", || drop(editor.mesh().smooth_normals()));
    bench.time(name, "box select", || {
        let mut e = editor.clone();
        let mut tools = ToolBox::default();
        tools.pointer(&mut e, &at(DVec2::new(10., 10.)), Pointer::Move);
        tools.pointer(&mut e, &at(DVec2::new(10., 10.)), Pointer::Down { clicks: 1 });
        tools.pointer(&mut e, &at(DVec2::new(1590., 990.)), Pointer::Move);
        tools.pointer(&mut e, &at(DVec2::new(1590., 990.)), Pointer::Up);
    });
}

fn main() {
    let sizes: Vec<usize> = std::env::args().skip(1).map(|a| a.parse().expect("a box count")).collect();
    let sizes = if sizes.is_empty() { vec![250, 1000] } else { sizes };
    let mut bench = Bench { over: BTreeSet::new() };
    for count in sizes {
        scene(&mut bench, "raw", count, &raw(count));
        scene(&mut bench, "groups", count, &instanced(count, false));
        scene(&mut bench, "component", count, &instanced(count, true));
        scene(&mut bench, "cylinders", count, &cylinders(count));
    }
}
