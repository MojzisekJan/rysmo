//! Scaled orthographic vector drawings, shared by SVG and PDF output.
use rysmo_math::{DVec2, DVec3};
use rysmo_mesh::triangulate_face;
use rysmo_model::{
    Document,
    sections::{clip_segment, clip_triangle, triangle_slice},
};
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Top,
    Front,
    Right,
    Isometric,
}
impl Orientation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "Top",
            Self::Front => "Front",
            Self::Right => "Right",
            Self::Isometric => "Isometric",
        }
    }
    fn basis(self) -> [DVec3; 3] {
        match self {
            Self::Top => [DVec3::X, DVec3::Y, DVec3::Z],
            Self::Front => [DVec3::X, DVec3::Z, DVec3::NEG_Y],
            Self::Right => [DVec3::Y, DVec3::Z, DVec3::X],
            Self::Isometric => {
                let right = DVec3::new(1., -1., 0.).normalize();
                let up = DVec3::new(-1., -1., 2.).normalize();
                [right, up, right.cross(up)]
            }
        }
    }
}
#[derive(Clone, Debug)]
pub struct Options {
    pub page_mm: [f64; 2],
    pub margin_mm: f64,
    pub scale: f64,
    pub orientation: Orientation,
    pub hidden_lines: bool,
    pub dimensions: bool,
    pub annotations: bool,
    pub title: String,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            page_mm: [297., 210.],
            margin_mm: 12.,
            scale: 50.,
            orientation: Orientation::Top,
            hidden_lines: true,
            dimensions: true,
            annotations: true,
            title: "Drawing".into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Line {
    pub points: [DVec2; 2],
    pub width: f64,
}
#[derive(Clone, Debug)]
pub struct Label {
    pub at: DVec2,
    pub text: String,
    pub size: f64,
}
pub struct Drawing {
    pub page_mm: [f64; 2],
    pub lines: Vec<Line>,
    pub fills: Vec<Vec<DVec2>>,
    pub labels: Vec<Label>,
    pub scale: f64,
}
fn err(message: &str) -> String {
    message.into()
}

/// Large occlusion jobs fail with a diagnostic; callers can explicitly choose wireframe.
pub fn build(doc: &Document, options: &Options) -> Result<Drawing, String> {
    if !options.scale.is_finite()
        || !(0.001..=1e6).contains(&options.scale)
        || !options.margin_mm.is_finite()
        || options.margin_mm < 0.
        || options.page_mm.iter().any(|v| !v.is_finite() || !(50. ..=2000.).contains(v))
        || options.page_mm.iter().any(|v| *v < 2. * options.margin_mm + 20.)
        || options.title.len() > 512
    {
        return Err(err("Invalid drawing scale, page, margin or title."));
    }
    let basis = options.orientation.basis();
    let project = |p: DVec3| DVec3::new(p.dot(basis[0]), p.dot(basis[1]), p.dot(basis[2]));
    let mut triangles = Vec::new();
    let mut lines = Vec::new();
    let mut labels = Vec::new();
    let mut overlay = Vec::new();
    let mut fills = Vec::new();
    for occurrence in doc.occurrences(&[]) {
        let Some(definition) = doc.def(occurrence.definition) else { continue };
        let mesh = doc.effective_mesh(occurrence.definition);
        let transform = occurrence.transform;
        let planes = doc.section_planes(&occurrence.path);
        for (_, edge) in mesh.edges().filter(|(_, e)| e.visible()) {
            let points = edge.v.map(|v| transform.transform_point3(mesh.pos(v)));
            if let Some(line) = clip_segment(points, &planes) {
                lines.push((line.map(project), 0.18));
            }
        }
        for (id, _) in mesh.faces().filter(|(_, f)| !f.hidden) {
            for tri in triangulate_face(&mesh, id) {
                let triangle = tri.map(|v| transform.transform_point3(mesh.pos(v)));
                for plane in &planes {
                    if let Some(cut) = triangle_slice(triangle, plane).and_then(|l| clip_segment(l, &planes)) {
                        lines.push((cut.map(project), 0.5));
                    }
                }
                if options.hidden_lines {
                    let polygon = clip_triangle(triangle, &planes);
                    for i in 1..polygon.len().saturating_sub(1) {
                        triangles.push([polygon[0].0, polygon[i].0, polygon[i + 1].0].map(project));
                    }
                }
            }
        }
        if lines.len() > 100_000 || triangles.len() > 200_000 {
            return Err(err("Drawing exceeds the 100,000 line / 200,000 triangle limit. Export a smaller model."));
        }
        if options.dimensions {
            for dimension in definition.dimensions.values() {
                if let Some(layout) = dimension.layout(doc, occurrence.definition, transform, 0.) {
                    overlay.extend(layout.segments.iter().map(|l| (l.map(project), 0.13)));
                    for (tip, direction) in &layout.arrows {
                        let tip = project(*tip);
                        let direction = project(*direction).truncate().normalize_or_zero();
                        if direction.length_squared() > 0. {
                            let length = 2. * options.scale / 1000.;
                            let side = DVec2::new(-direction.y, direction.x) * length * 0.22;
                            let base = tip.truncate() + direction * length;
                            use rysmo_model::dimensions::DimensionEndpoints;
                            match layout.endpoints {
                                DimensionEndpoints::None => {}
                                DimensionEndpoints::OpenArrow => {
                                    for end in [base + side, base - side] {
                                        overlay.push(([tip, end.extend(tip.z)], 0.13));
                                    }
                                }
                                DimensionEndpoints::ClosedArrow => {
                                    fills.push(vec![tip, (base + side).extend(tip.z), (base - side).extend(tip.z)])
                                }
                                DimensionEndpoints::Slash => {
                                    let d = (direction + DVec2::new(-direction.y, direction.x)) * length * 0.25;
                                    overlay.push((
                                        [(tip.truncate() - d).extend(tip.z), (tip.truncate() + d).extend(tip.z)],
                                        0.13,
                                    ));
                                }
                                DimensionEndpoints::Dot => fills.push(
                                    (0..12)
                                        .map(|i| {
                                            let angle = i as f64 * std::f64::consts::TAU / 12.;
                                            (tip.truncate() + DVec2::new(angle.cos(), angle.sin()) * length * 0.18)
                                                .extend(tip.z)
                                        })
                                        .collect(),
                                ),
                            }
                        }
                    }
                    labels.push((
                        project(layout.text_position),
                        dimension.label_in_document(&layout.measurement, doc),
                        true,
                    ));
                }
            }
        }
        if options.annotations {
            for text in definition.texts.values() {
                use rysmo_model::text::{TextLeader, TextPlacement};
                if let TextPlacement::Attached { anchor, leader } = &text.placement {
                    let at = anchor.resolve(doc, occurrence.definition).0;
                    let offset = match leader {
                        TextLeader::None { offset } | TextLeader::Pushpin { offset } => *offset,
                        TextLeader::ViewBased { offset } => {
                            basis[0] * offset.x * options.scale / 4000. - basis[1] * offset.y * options.scale / 4000.
                        }
                    };
                    let from = transform.transform_point3(at);
                    let to = from + transform.transform_vector3(offset);
                    if !matches!(leader, TextLeader::None { .. }) {
                        overlay.push(([project(from), project(to)], 0.13));
                    }
                    labels.push((project(to), text.content.clone(), false));
                }
            }
        }
    }
    if lines.is_empty() {
        return Err(err("No visible linework in this view."));
    }
    let mut visible = Vec::new();
    let mut budget = 8_000_000usize;
    for (line, width) in lines {
        let intervals = if options.hidden_lines && width < 0.3 {
            visible_intervals(line, &triangles, &mut budget)?
        } else {
            vec![[0., 1.]]
        };
        for [a, b] in intervals {
            visible.push(([line[0].lerp(line[1], a).truncate(), line[0].lerp(line[1], b).truncate()], width));
        }
    }
    visible.extend(overlay.into_iter().map(|(line, width)| (line.map(|p| p.truncate()), width)));
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for (line, _) in &visible {
        for p in line {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    for p in fills.iter().flatten() {
        lo = lo.min(p.truncate());
        hi = hi.max(p.truncate());
    }
    for (point, text, centred) in &labels {
        let width = text.lines().map(|line| text_width(line, 3.)).fold(0., f64::max) * options.scale / 1000.;
        let x = if *centred { -width * 0.5 } else { 0. };
        let top = 3. * options.scale / 1000.;
        let bottom = text.lines().count().saturating_sub(1) as f64 * 3.8 * options.scale / 1000.;
        lo = lo.min(point.truncate() + DVec2::new(x, -bottom));
        hi = hi.max(point.truncate() + DVec2::new(x + width, top));
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Err(err("No visible linework in this view."));
    }
    let mm = 1000. / options.scale;
    let usable = DVec2::from_array(options.page_mm) - DVec2::new(2. * options.margin_mm, 2. * options.margin_mm + 12.);
    if ((hi - lo) * mm).cmpgt(usable - DVec2::splat(6.)).any() {
        return Err(format!(
            "Model does not fit the page at 1:{}. Choose a larger scale denominator or page.",
            options.scale
        ));
    }
    let centre = (lo + hi) * 0.5;
    let page = DVec2::from_array(options.page_mm) * 0.5 - DVec2::Y * 6.;
    let at = |p: DVec2| {
        let d = (p - centre) * mm;
        page + DVec2::new(d.x, -d.y)
    };
    let mut drawing = Drawing {
        page_mm: options.page_mm,
        lines: visible.into_iter().map(|(l, width)| Line { points: l.map(at), width }).collect(),
        fills: fills.into_iter().map(|polygon| polygon.into_iter().map(|p| at(p.truncate())).collect()).collect(),
        labels: labels
            .into_iter()
            .flat_map(|(p, text, centred)| {
                let position = at(p.truncate());
                text.lines()
                    .enumerate()
                    .map(|(i, line)| Label {
                        at: position
                            + DVec2::new(if centred { -text_width(line, 3.) * 0.5 } else { 0. }, i as f64 * 3.8),
                        text: line.into(),
                        size: 3.,
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
        scale: options.scale,
    };
    drawing.labels.push(Label {
        at: DVec2::new(options.margin_mm, options.page_mm[1] - options.margin_mm),
        text: format!("{} | {} | 1:{} | Rysmo", options.title, options.orientation.name(), options.scale),
        size: 3.,
    });
    if drawing
        .labels
        .last()
        .is_some_and(|label| text_width(&label.text, label.size) > options.page_mm[0] - 2. * options.margin_mm)
    {
        return Err(err("Drawing title is too long for the page."));
    }
    Ok(drawing)
}
fn visible_intervals(line: [DVec3; 2], triangles: &[[DVec3; 3]], budget: &mut usize) -> Result<Vec<[f64; 2]>, String> {
    let a = line[0].truncate();
    let delta = line[1].truncate() - a;
    if delta.length_squared() < 1e-18 {
        return Ok(Vec::new());
    }
    let lo = a.min(a + delta);
    let hi = a.max(a + delta);
    let mut hits = Vec::new();
    let mut cuts = vec![0., 1.];
    for triangle in triangles {
        *budget = budget.checked_sub(1).ok_or_else(|| {
            err("Hidden-line drawing exceeds the work budget. Disable hidden-line removal or reduce the model.")
        })?;
        let xy = triangle.map(|p| p.truncate());
        let tlo = xy[0].min(xy[1]).min(xy[2]);
        let thi = xy[0].max(xy[1]).max(xy[2]);
        if tlo.cmpgt(hi).any() || thi.cmplt(lo).any() {
            continue;
        }
        if (xy[1] - xy[0]).perp_dot(xy[2] - xy[0]).abs() < 1e-15 {
            continue;
        }
        for i in 0..3 {
            let b = xy[i];
            let edge = xy[(i + 1) % 3] - b;
            let determinant = delta.perp_dot(edge);
            if determinant.abs() < 1e-15 {
                continue;
            }
            let t = (b - a).perp_dot(edge) / determinant;
            let u = (b - a).perp_dot(delta) / determinant;
            if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&u) {
                cuts.push(t);
            }
        }
        // Occlusion can change where the line crosses the triangle's depth plane.
        let z0 = triangle_depth(line[0].truncate(), *triangle);
        let z1 = triangle_depth(line[1].truncate(), *triangle);
        let d0 = line[0].z - z0;
        let d1 = line[1].z - z1;
        if d0 * d1 < 0. {
            cuts.push(d0 / (d0 - d1));
        }
        hits.push(*triangle);
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    let mut result = Vec::new();
    for interval in cuts.windows(2) {
        let t = (interval[0] + interval[1]) * 0.5;
        let p = line[0].lerp(line[1], t);
        let mut hidden = false;
        for triangle in &hits {
            *budget = budget.checked_sub(1).ok_or_else(|| err("Hidden-line drawing exceeds the work budget."))?;
            let xy = triangle.map(|p| p.truncate());
            let q = p.truncate();
            let signs = (0..3).map(|i| (xy[(i + 1) % 3] - xy[i]).perp_dot(q - xy[i])).collect::<Vec<_>>();
            let inside = signs.iter().all(|v| *v >= -1e-10) || signs.iter().all(|v| *v <= 1e-10);
            if inside && triangle_depth(q, *triangle) > p.z + 1e-7 {
                hidden = true;
                break;
            }
        }
        if !hidden {
            result.push([interval[0], interval[1]]);
        }
    }
    Ok(result)
}
fn triangle_depth(p: DVec2, t: [DVec3; 3]) -> f64 {
    let a = t[0].truncate();
    let u = t[1].truncate() - a;
    let v = t[2].truncate() - a;
    let det = u.perp_dot(v);
    let b = (p - a).perp_dot(v) / det;
    let c = u.perp_dot(p - a) / det;
    t[0].z + (t[1].z - t[0].z) * b + (t[2].z - t[0].z) * c
}
fn text_width(text: &str, size: f64) -> f64 {
    let face =
        ttf_parser::Face::parse(include_bytes!("../../rysmo_model/fonts/DejaVuSans.ttf"), 0).expect("bundled font");
    text.chars().map(|c| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).unwrap_or(0) as f64).sum::<f64>()
        * size
        / face.units_per_em() as f64
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
impl Drawing {
    pub fn svg(&self) -> String {
        let [w, h] = self.page_mm;
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g fill=\"none\" stroke=\"#20252c\" stroke-linecap=\"round\">\n"
        );
        for line in &self.lines {
            let [a, b] = line.points;
            writeln!(out, "<path stroke-width=\"{}\" d=\"M {} {} L {} {}\"/>", line.width, a.x, a.y, b.x, b.y).unwrap();
        }
        out.push_str("</g><g fill=\"#20252c\" font-family=\"DejaVu Sans,sans-serif\">\n");
        for polygon in &self.fills {
            let points = polygon.iter().map(|p| format!("{},{}", p.x, p.y)).collect::<Vec<_>>().join(" ");
            writeln!(out, "<polygon points=\"{points}\"/>").unwrap();
        }
        for label in &self.labels {
            writeln!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"white\"/>",
                label.at.x - 0.6,
                label.at.y - label.size - 0.3,
                text_width(&label.text, label.size) + 1.2,
                label.size + 1.
            )
            .unwrap();
            writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\">{}</text>",
                label.at.x,
                label.at.y,
                label.size,
                escape(&label.text)
            )
            .unwrap();
        }
        out.push_str("</g></svg>");
        out
    }
    pub fn pdf(&self) -> Vec<u8> {
        pdf::write(self)
    }
}
mod pdf {
    use super::*;
    use std::collections::BTreeMap;
    pub fn write(drawing: &Drawing) -> Vec<u8> {
        let font = include_bytes!("../../rysmo_model/fonts/DejaVuSans.ttf");
        let face = ttf_parser::Face::parse(font, 0).expect("bundled font");
        let unit = 1000. / face.units_per_em() as f64;
        let mut glyphs = BTreeMap::new();
        let mut content = String::new();
        let pt = 72. / 25.4;
        let height = drawing.page_mm[1];
        for line in &drawing.lines {
            let [a, b] = line.points;
            writeln!(
                content,
                "{} w {} {} m {} {} l S",
                line.width * pt,
                a.x * pt,
                (height - a.y) * pt,
                b.x * pt,
                (height - b.y) * pt
            )
            .unwrap();
        }
        for polygon in &drawing.fills {
            for (i, p) in polygon.iter().enumerate() {
                writeln!(content, "{} {} {}", p.x * pt, (height - p.y) * pt, if i == 0 { "m" } else { "l" }).unwrap();
            }
            content.push_str("h f\n");
        }
        for label in &drawing.labels {
            writeln!(
                content,
                "q 1 g {} {} {} {} re f Q",
                (label.at.x - 0.6) * pt,
                (height - label.at.y - 0.7) * pt,
                (text_width(&label.text, label.size) + 1.2) * pt,
                (label.size + 1.) * pt
            )
            .unwrap();
            let mut encoded = String::new();
            for character in label.text.chars() {
                let glyph = face.glyph_index(character).unwrap_or(ttf_parser::GlyphId(0));
                glyphs.entry(glyph.0).or_insert(character);
                write!(encoded, "{:04X}", glyph.0).unwrap();
            }
            writeln!(
                content,
                "BT /F1 {} Tf 1 0 0 1 {} {} Tm <{}> Tj ET",
                label.size * pt,
                label.at.x * pt,
                (height - label.at.y) * pt,
                encoded
            )
            .unwrap();
        }
        let stream = |data: &[u8], extra: &str| {
            let mut v = format!("<< /Length {} {extra} >>\nstream\n", data.len()).into_bytes();
            v.extend_from_slice(data);
            v.extend_from_slice(b"\nendstream");
            v
        };
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /RysmoUnicode def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
        );
        let entries: Vec<_> = glyphs.iter().collect();
        for chunk in entries.chunks(100) {
            writeln!(cmap, "{} beginbfchar", chunk.len()).unwrap();
            for (id, character) in chunk {
                let mut buffer = [0u16; 2];
                let utf16 = character.encode_utf16(&mut buffer);
                write!(cmap, "<{id:04X}> <").unwrap();
                for v in utf16 {
                    write!(cmap, "{v:04X}").unwrap();
                }
                cmap.push_str(">\n");
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
        let widths = glyphs
            .keys()
            .map(|id| format!("{id} [{}]", face.glyph_hor_advance(ttf_parser::GlyphId(*id)).unwrap_or(0) as f64 * unit))
            .collect::<Vec<_>>()
            .join(" ");
        let bbox = face.global_bounding_box();
        let objects=[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",drawing.page_mm[0]*pt,height*pt).into_bytes(),
            stream(content.as_bytes(),""),
            b"<< /Type /Font /Subtype /Type0 /BaseFont /DejaVuSans /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".to_vec(),
            format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /DejaVuSans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 1000 /W [{widths}] >>").into_bytes(),
            format!("<< /Type /FontDescriptor /FontName /DejaVuSans /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 8 0 R >>",bbox.x_min as f64*unit,bbox.y_min as f64*unit,bbox.x_max as f64*unit,bbox.y_max as f64*unit,face.ascender() as f64*unit,face.descender() as f64*unit,face.capital_height().unwrap_or(face.ascender()) as f64*unit).into_bytes(),
            stream(font,&format!("/Length1 {}",font.len())),stream(cmap.as_bytes(),""),
        ];
        let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(object);
            out.extend_from_slice(b"\nendobj\n");
        }
        let start = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
        for offset in &offsets[1..] {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n", offsets.len()).as_bytes(),
        );
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn one_metre_is_twenty_millimetres_at_one_to_fifty() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
        let drawing = build(editor.doc(), &Options::default()).unwrap();
        assert!(drawing.lines.iter().all(|l| (l.points[0].distance(l.points[1]) - 20.).abs() < 1e-8));
        assert!(drawing.svg().contains("297mm"));
        assert!(drawing.pdf().starts_with(b"%PDF-1.7"));
        assert!(build(editor.doc(), &Options { scale: 1., ..Default::default() }).is_err());
    }
    #[test]
    fn occlusion_splits_lines_at_triangle_boundaries_and_work_is_bounded() {
        let triangle = [DVec3::new(0., 0., 1.), DVec3::new(2., 0., 1.), DVec3::new(0., 2., 1.)];
        let line = [DVec3::new(-1., 0.5, 0.), DVec3::new(3., 0.5, 0.)];
        let intervals = visible_intervals(line, &[triangle], &mut 100).unwrap();
        assert_eq!(intervals.len(), 2);
        assert!((intervals[0][1] - 0.25).abs() < 1e-8);
        assert!((intervals[1][0] - 0.625).abs() < 1e-8);
        assert!(visible_intervals(line, &[triangle], &mut 0).is_err());
    }
}
