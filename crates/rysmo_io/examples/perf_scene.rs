//! Writes a model of many boxes for judging the viewport by hand.
//!
//!     cargo run --release -p rysmo_io --example perf_scene -- out.rysmo 5000

use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("an output path");
    let count: u32 = args.next().map_or(1000, |n| n.parse().expect("a box count"));
    let rows = count.isqrt().max(2);
    let mut editor = Editor::new();
    let unit = Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None };
    editor.apply(unit).expect("a rectangle");
    let face = editor.mesh().faces().next().expect("a face").0;
    editor.apply(Command::PushPull { face, distance: 1., keep_base: false }).expect("a box");
    let everything = |editor: &Editor| -> Vec<Element> {
        let mesh = editor.mesh();
        mesh.faces().map(|(id, _)| Element::Face(id)).chain(mesh.edges().map(|(id, _)| Element::Edge(id))).collect()
    };
    let row = everything(&editor);
    editor.apply(Command::Copy { elements: row, offset: DVec3::X * 2., copies: rows - 1 }).expect("a row");
    let all = everything(&editor);
    editor.apply(Command::Copy { elements: all, offset: DVec3::Y * 2., copies: rows - 1 }).expect("rows");
    std::fs::write(path, rysmo_io::save(editor.doc())).expect("a writable path");
    println!("{} faces", editor.mesh().face_count());
}
