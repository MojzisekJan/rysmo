//! Editable geometric text from bundled TrueType outlines.
use rysmo_math::{DVec2, DVec3};
use rysmo_mesh::{Edge, EdgeId, Face, FaceId, Mesh, Vertex, VertexId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Text3dFont {
    #[default]
    Sans,
    Serif,
    Mono,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Text3dAlign {
    #[default]
    Left,
    Centre,
    Right,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text3dSettings {
    pub font: Text3dFont,
    pub bold: bool,
    /// Capital-letter height in model metres.
    pub height: f64,
    pub align: Text3dAlign,
    pub filled: bool,
    pub extruded: bool,
    pub depth: f64,
}
impl Default for Text3dSettings {
    fn default() -> Self {
        Self {
            font: Text3dFont::Sans,
            bold: false,
            height: 1.,
            align: Text3dAlign::Left,
            filled: true,
            extruded: true,
            depth: 0.1,
        }
    }
}
impl Text3dSettings {
    pub fn validate(&self) -> bool {
        self.height.is_finite()
            && self.height >= 1e-4
            && self.depth.is_finite()
            && (!self.filled || !self.extruded || self.depth >= 1e-5)
    }
    fn bytes(&self) -> &'static [u8] {
        match (self.font, self.bold) {
            (Text3dFont::Sans, false) => include_bytes!("../fonts/DejaVuSans.ttf"),
            (Text3dFont::Sans, true) => include_bytes!("../fonts/DejaVuSans-Bold.ttf"),
            (Text3dFont::Serif, false) => include_bytes!("../fonts/DejaVuSerif.ttf"),
            (Text3dFont::Serif, true) => include_bytes!("../fonts/DejaVuSerif-Bold.ttf"),
            (Text3dFont::Mono, false) => include_bytes!("../fonts/DejaVuSansMono.ttf"),
            (Text3dFont::Mono, true) => include_bytes!("../fonts/DejaVuSansMono-Bold.ttf"),
        }
    }
}

#[derive(Default)]
struct Outline {
    loops: Vec<Vec<DVec2>>,
    current: Vec<DVec2>,
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.current.push(DVec2::new(x as f64, y as f64));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(DVec2::new(x as f64, y as f64));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = *self.current.last().unwrap();
        let b = DVec2::new(x1 as f64, y1 as f64);
        let c = DVec2::new(x as f64, y as f64);
        for i in 1..=12 {
            let t = i as f64 / 12.;
            self.current.push(a * (1. - t).powi(2) + b * (2. * t * (1. - t)) + c * t * t);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = *self.current.last().unwrap();
        let b = DVec2::new(x1 as f64, y1 as f64);
        let c = DVec2::new(x2 as f64, y2 as f64);
        let d = DVec2::new(x as f64, y as f64);
        for i in 1..=16 {
            let t = i as f64 / 16.;
            self.current.push(
                a * (1. - t).powi(3) + b * (3. * t * (1. - t).powi(2)) + c * (3. * t * t * (1. - t)) + d * t.powi(3),
            );
        }
    }
    fn close(&mut self) {
        self.current.dedup_by(|a, b| a.distance(*b) < 1e-7);
        if self.current.len() > 1 && self.current[0].distance(*self.current.last().unwrap()) < 1e-7 {
            self.current.pop();
        }
        if self.current.len() >= 3 {
            self.loops.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}
fn area(points: &[DVec2]) -> f64 {
    points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).map(|(a, b)| a.perp_dot(*b)).sum::<f64>() * 0.5
}
fn inside(p: DVec2, points: &[DVec2]) -> bool {
    let mut hit = false;
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            hit = !hit;
        }
    }
    hit
}

fn glyph_positions(font: &ttf_parser::Face<'_>, line: &str) -> Result<(Vec<(ttf_parser::GlyphId, f64)>, f64), String> {
    let mut placed = Vec::new();
    let mut x = 0.;
    let mut previous = None;
    for character in line.chars() {
        let glyph = font.glyph_index(character).ok_or_else(|| format!("font does not contain {character:?}"))?;
        if let Some(left) = previous
            && let Some(kern) = font.tables().kern
        {
            for subtable in kern.subtables {
                if subtable.horizontal && !subtable.variable && !subtable.has_cross_stream {
                    x += subtable.glyphs_kerning(left, glyph).unwrap_or(0) as f64;
                }
            }
        }
        placed.push((glyph, x));
        x += font.glyph_hor_advance(glyph).unwrap_or(0) as f64;
        previous = Some(glyph);
    }
    Ok((placed, x))
}

/// Builds local XY text, with extrusion along +Z. The returned mesh can be
/// grouped, transformed and edited with ordinary modelling tools.
pub fn build_text(content: &str, settings: &Text3dSettings) -> Result<Mesh, String> {
    if !settings.validate() || content.trim().is_empty() || content.chars().count() > 2048 {
        return Err("invalid 3D text or settings".into());
    }
    let font = ttf_parser::Face::parse(settings.bytes(), 0).map_err(|_| "invalid bundled font")?;
    let capital = font.glyph_index('H').and_then(|g| font.glyph_bounding_box(g)).ok_or("font has no capital height")?;
    let scale = settings.height / (capital.y_max - capital.y_min) as f64;
    let spacing = (font.ascender() as f64 - font.descender() as f64 + font.line_gap() as f64) * scale;
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let mut faces = BTreeMap::new();
    let mut next = 1u64;
    for (line_index, line) in content.lines().enumerate() {
        let (glyphs, width) = glyph_positions(&font, line)?;
        let width = width * scale;
        let x = match settings.align {
            Text3dAlign::Left => 0.,
            Text3dAlign::Centre => -width * 0.5,
            Text3dAlign::Right => -width,
        };
        for (glyph, advance) in glyphs {
            let x = x + advance * scale;
            let mut outline = Outline::default();
            font.outline_glyph(glyph, &mut outline);
            ttf_parser::OutlineBuilder::close(&mut outline);
            let loops: Vec<_> = outline
                .loops
                .into_iter()
                .map(|points| {
                    points
                        .into_iter()
                        .map(|p| p * scale + DVec2::new(x, -(line_index as f64) * spacing))
                        .collect::<Vec<_>>()
                })
                .collect();
            let parents: Vec<_> = loops
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    loops
                        .iter()
                        .enumerate()
                        .filter(|(j, q)| *j != i && area(q).abs() > area(p).abs() && inside(p[0], q))
                        .min_by(|(_, a), (_, b)| area(a).abs().total_cmp(&area(b).abs()))
                        .map(|(j, _)| j)
                })
                .collect();
            let mut ids = Vec::new();
            for (i, points) in loops.iter().enumerate() {
                let mut depth = 0;
                let mut parent = parents[i];
                while let Some(p) = parent {
                    depth += 1;
                    parent = parents[p];
                }
                let mut points = points.clone();
                if (area(&points) > 0.) != (depth % 2 == 0) {
                    points.reverse();
                }
                let loop_ids: Vec<_> = points
                    .iter()
                    .map(|p| {
                        let id = VertexId(next);
                        next += 1;
                        vertices.insert(id, Vertex { pos: DVec3::new(p.x, p.y, 0.) });
                        id
                    })
                    .collect();
                for (a, b) in loop_ids.iter().zip(loop_ids.iter().cycle().skip(1)).take(loop_ids.len()) {
                    edges.insert(EdgeId(next), Edge::new(*a, *b));
                    next += 1;
                }
                ids.push((loop_ids, depth));
            }
            if settings.filled {
                for (i, (outer, depth)) in ids.iter().enumerate().filter(|(_, (_, d))| d % 2 == 0) {
                    let _ = depth;
                    let holes = ids
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| parents[*j] == Some(i))
                        .map(|(_, (p, _))| p.clone())
                        .collect();
                    faces.insert(
                        FaceId(next),
                        Face {
                            hidden: false,
                            outer: outer.clone(),
                            holes,
                            material: None,
                            back_material: None,
                            uv: None,
                        },
                    );
                    next += 1;
                }
            }
        }
    }
    if vertices.is_empty() {
        return Err("text has no visible outlines".into());
    }
    let mut mesh = Mesh::from_parts(vertices, edges, faces).map_err(|e| e.to_string())?;
    if settings.filled && settings.extruded {
        let faces: Vec<_> = mesh.faces().map(|(id, _)| id).collect();
        for face in faces {
            mesh.push_pull(face, settings.depth, false).map_err(|e| e.to_string())?;
        }
    }
    mesh.validate().map_err(|e| e.to_string())?;
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kerning_changes_letter_positions_and_alignment_width() {
        let settings = Text3dSettings { filled: false, ..Default::default() };
        let font = ttf_parser::Face::parse(settings.bytes(), 0).unwrap();
        let (positions, width) = glyph_positions(&font, "AV").unwrap();
        let a = font.glyph_hor_advance(positions[0].0).unwrap() as f64;
        let v = font.glyph_hor_advance(positions[1].0).unwrap() as f64;
        assert!(positions[1].1 < a);
        assert!(width < a + v);
        let capital = font.glyph_bounding_box(font.glyph_index('H').unwrap()).unwrap();
        let scale = settings.height / (capital.y_max - capital.y_min) as f64;
        let left = build_text("AV", &settings).unwrap();
        let right = build_text("AV", &Text3dSettings { align: Text3dAlign::Right, ..settings }).unwrap();
        assert!((left.bounds().unwrap().0.x - right.bounds().unwrap().0.x - width * scale).abs() < 1e-9);
        let mono_settings = Text3dSettings { font: Text3dFont::Mono, ..Default::default() };
        let mono_font = ttf_parser::Face::parse(mono_settings.bytes(), 0).unwrap();
        let (mono, width) = glyph_positions(&mono_font, "AV").unwrap();
        assert_eq!(width, mono[1].1 * 2.);
    }
    #[test]
    fn creation_places_a_group_with_fresh_ids_and_one_undo() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let before = editor.doc().clone();
        let command = Command::CreateText3d {
            content: "Číslo 8".into(),
            settings: Text3dSettings::default(),
            origin: DVec3::new(4., 3., 2.),
            x: DVec3::Y,
            y: DVec3::Z,
        };
        let serialized = serde_json::to_string(&command).unwrap();
        let result = editor.apply(serde_json::from_str(&serialized).unwrap()).unwrap();
        let Element::Instance(id) = result.created[0] else { panic!() };
        let instance = &editor.active().instances[&id];
        assert_eq!(instance.transform.transform_point3(DVec3::ZERO), DVec3::new(4., 3., 2.));
        assert_eq!(instance.transform.transform_vector3(DVec3::Z), DVec3::X);
        let definition = &editor.doc().definitions[&instance.definition];
        assert!(definition.group);
        assert!(definition.mesh.vertices().all(|(id, _)| id.0 >= before.next_id));
        assert!(definition.mesh.faces().count() > 10);
        editor.undo();
        assert_eq!(editor.doc(), &before);
        let invalid = Command::CreateText3d {
            content: "H".into(),
            settings: Default::default(),
            origin: DVec3::ZERO,
            x: DVec3::X,
            y: DVec3::X,
        };
        assert!(editor.apply(invalid).is_err());
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn letter_holes_and_extrusion_form_closed_editable_solids() {
        let settings = Text3dSettings { height: 2., depth: 0.4, ..Default::default() };
        let mesh = build_text("OB", &settings).unwrap();
        let (lo, hi) = mesh.bounds().unwrap();
        assert!((hi.z - lo.z - 0.4).abs() < 1e-9);
        assert!(mesh.faces().any(|(_, f)| !f.holes.is_empty()));
        for (_, edge) in mesh.edges() {
            let adjacent = mesh
                .faces()
                .filter(|(_, f)| f.loop_edges().iter().any(|(a, b)| edge.v.contains(a) && edge.v.contains(b)))
                .count();
            assert_eq!(adjacent, 2, "every solid edge has two adjacent faces");
        }
        mesh.validate().unwrap();
    }
    #[test]
    fn outlines_flat_faces_fonts_and_czech_text() {
        for font in [Text3dFont::Sans, Text3dFont::Serif, Text3dFont::Mono] {
            for bold in [false, true] {
                let settings = Text3dSettings { font, bold, filled: false, ..Default::default() };
                let mesh = build_text("Přízemí č. 2", &settings).unwrap();
                assert_eq!(mesh.faces().count(), 0);
                assert!(mesh.edges().count() > 30);
                let filled = build_text("O", &Text3dSettings { filled: true, extruded: false, ..settings }).unwrap();
                assert_eq!(filled.faces().count(), 1);
                assert_eq!(filled.faces().next().unwrap().1.holes.len(), 1);
                assert_eq!(filled.bounds().unwrap().1.z, 0.);
            }
        }
    }
    #[test]
    fn capital_height_multiline_alignment_and_invalid_input() {
        let settings = Text3dSettings { height: 3., filled: false, ..Default::default() };
        let mesh = build_text("H", &settings).unwrap();
        let (lo, hi) = mesh.bounds().unwrap();
        assert!((hi.y - lo.y - 3.).abs() < 1e-9);
        let left = build_text("HI\nH", &settings).unwrap();
        let centre = build_text("HI\nH", &Text3dSettings { align: Text3dAlign::Centre, ..settings.clone() }).unwrap();
        let right = build_text("HI\nH", &Text3dSettings { align: Text3dAlign::Right, ..settings.clone() }).unwrap();
        assert!(left.bounds().unwrap().0.y < -3.);
        assert!(right.bounds().unwrap().1.x < left.bounds().unwrap().1.x);
        assert!(centre.bounds().unwrap().0.x < left.bounds().unwrap().0.x);
        for text in ["", " \n ", "\u{10ffff}"] {
            assert!(build_text(text, &settings).is_err());
        }
        assert!(build_text("H", &Text3dSettings { height: f64::NAN, ..settings }).is_err());
    }
}
