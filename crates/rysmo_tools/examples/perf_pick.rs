//! CPU-only first and subsequent picks of 4,000 shared cubes. No renderer/GPU work.
use rysmo_math::{DMat4, DVec2, DVec3, OrbitCamera};
use rysmo_model::{Command, DefinitionId, Document, Editor, Element, Instance, InstanceId};
use rysmo_tools::{Ctx, Modifiers, ToolBox};
use std::{hint::black_box, time::Instant};

fn measure(mut setup: impl FnMut(), mut run: impl FnMut()) {
    let mut samples = Vec::new();
    for index in 0..24 {
        setup();
        let start = Instant::now();
        run();
        if index >= 3 {
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }
    samples.sort_by(f64::total_cmp);
    println!("median_ms={:.6} p95_ms={:.6}", samples[10], samples[19]);
}

fn main() {
    let mut cube = Editor::new();
    cube.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
    let face = cube.mesh().faces().next().unwrap().0;
    cube.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
    let mut doc = Document::new();
    let definition = DefinitionId(doc.reserve_id());
    let mesh = cube.mesh().clone().with_fresh_ids(|| doc.reserve_id()).unwrap();
    doc.definitions.insert(definition, rysmo_model::Definition { mesh, ..Default::default() }.into());
    for index in 0..4_000 {
        let id = InstanceId(doc.reserve_id());
        doc.root_mut().instances.insert(
            id,
            Instance {
                definition,
                transform: DMat4::from_translation(DVec3::new((index % 64) as f64 * 2., (index / 64) as f64 * 2., 0.)),
                name: String::new(),
                tag: None,
                material: None,
                hidden: false,
                glued_to: None,
                attributes: Default::default(),
            },
        );
    }
    let id = *doc.root.instances.keys().next().unwrap();
    let editor = std::cell::RefCell::new(Editor::with_document(doc));
    let view = OrbitCamera { target: DVec3::new(64., 64., 0.), distance: 160., ..Default::default() }
        .view(DVec2::new(1600., 1000.));
    let ctx = Ctx { view: &view, cursor: DVec2::new(800., 500.), modifiers: Modifiers::default(), scale: 1. };
    let tools = std::cell::RefCell::new(ToolBox::default());
    println!("instances=4000 samples=21 debug_assertions={}", cfg!(debug_assertions));
    println!("cold_scene_and_pick");
    measure(
        || *tools.borrow_mut() = ToolBox::default(),
        || {
            black_box(tools.borrow().pick_full(&editor.borrow(), &ctx));
        },
    );
    println!("warm_pick");
    measure(
        || {},
        || {
            black_box(tools.borrow().pick_full(&editor.borrow(), &ctx));
        },
    );
    println!("pick_after_move_one");
    measure(
        || {
            editor
                .borrow_mut()
                .apply(Command::Move { elements: vec![Element::Instance(id)], offset: DVec3::Y })
                .unwrap();
        },
        || {
            black_box(tools.borrow().pick_full(&editor.borrow(), &ctx));
        },
    );
}
