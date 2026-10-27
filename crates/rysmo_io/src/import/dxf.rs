use super::{ImportError, Imported, Options, Result, error, geometry::Geometry};
use rysmo_math::{DMat4, DVec2, DVec3};
use rysmo_mesh::{DefinitionId, InstanceId, TagId};
use rysmo_model::{Definition, Document, Instance, Tag};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Record {
    kind: String,
    data: Vec<(i16, String)>,
}
impl Record {
    fn text(&self, code: i16) -> Option<&str> {
        self.data.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
    fn number(&self, code: i16, default: f64) -> Result<f64> {
        self.text(code).map_or(Ok(default), |s| {
            s.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| ImportError(format!("Invalid DXF number in {} code {code}.", self.kind)))
        })
    }
    fn point(&self, code: i16) -> Result<DVec3> {
        Ok(DVec3::new(self.number(code, 0.)?, self.number(code + 10, 0.)?, self.number(code + 20, 0.)?))
    }
}
#[derive(Clone)]
struct Block {
    base: DVec3,
    records: Vec<Record>,
}
struct Reader {
    blocks: BTreeMap<String, Block>,
    layers: BTreeMap<String, Geometry>,
    hidden: BTreeSet<String>,
    ignored: BTreeSet<String>,
    remaining: usize,
    segments: u32,
}
impl Reader {
    fn line(&mut self, layer: &str, t: DMat4, a: DVec3, b: DVec3) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| ImportError("DXF exceeds the expanded geometry budget.".into()))?;
        self.layers.entry(layer.into()).or_default().line(t.transform_point3(a), t.transform_point3(b))
    }
    fn polyline(&mut self, layer: &str, t: DMat4, vertices: &[(DVec3, f64)], closed: bool) -> Result<()> {
        if vertices.len() < 2 {
            return Ok(());
        }
        let count = if closed { vertices.len() } else { vertices.len() - 1 };
        for i in 0..count {
            let (a, bulge) = vertices[i];
            let b = vertices[(i + 1) % vertices.len()].0;
            if bulge.abs() < 1e-12 {
                self.line(layer, t, a, b)?;
                continue;
            }
            let chord = b.truncate() - a.truncate();
            let length = chord.length();
            if length < 1e-9 {
                continue;
            }
            if (a.z - b.z).abs() > 1e-6 {
                return error("A curved DXF polyline must be planar.");
            }
            let left = DVec2::new(-chord.y, chord.x) / length;
            let centre = (a.truncate() + b.truncate()) * 0.5 + left * (length * (1. - bulge * bulge) / (4. * bulge));
            let start = (a.y - centre.y).atan2(a.x - centre.x);
            let sweep = 4. * bulge.atan();
            let radius = (a.truncate() - centre).length();
            self.arc(layer, t, Arc { centre: centre.extend(a.z), radius, start, sweep })?;
        }
        Ok(())
    }
    fn arc(&mut self, layer: &str, t: DMat4, arc: Arc) -> Result<()> {
        if arc.radius <= 0. || !arc.radius.is_finite() {
            return error("Invalid DXF arc radius.");
        }
        let count = ((self.segments as f64 * arc.sweep.abs() / std::f64::consts::TAU).ceil() as usize).clamp(1, 256);
        let at = |i: usize| {
            let a = arc.start + arc.sweep * i as f64 / count as f64;
            arc.centre + DVec3::new(a.cos(), a.sin(), 0.) * arc.radius
        };
        for i in 0..count {
            self.line(layer, t, at(i), at(i + 1))?;
        }
        Ok(())
    }
    fn records(&mut self, records: &[Record], transform: DMat4, inherited: &str, depth: usize) -> Result<()> {
        if depth > 32 {
            return error("DXF block hierarchy is cyclic or too deep.");
        }
        let mut i = 0;
        while i < records.len() {
            self.remaining = self
                .remaining
                .checked_sub(1)
                .ok_or_else(|| ImportError("DXF exceeds the expanded record budget.".into()))?;
            let r = &records[i];
            i += 1;
            let name = r.text(8).unwrap_or("0");
            let layer = if name == "0" { inherited } else { name };
            match r.kind.as_str() {
                "LINE" => self.line(layer, transform, r.point(10)?, r.point(11)?)?,
                "LWPOLYLINE" => {
                    let z = r.number(38, 0.)?;
                    let mut vertices = Vec::<(DVec3, f64)>::new();
                    for (code, value) in &r.data {
                        let number = || {
                            value
                                .parse::<f64>()
                                .ok()
                                .filter(|v| v.is_finite())
                                .ok_or_else(|| ImportError("Invalid polyline coordinate.".into()))
                        };
                        match *code {
                            10 => vertices.push((DVec3::new(number()?, 0., z), 0.)),
                            20 => {
                                let Some(last) = vertices.last_mut() else { return error("DXF Y precedes X.") };
                                last.0.y = number()?;
                            }
                            42 => {
                                let Some(last) = vertices.last_mut() else {
                                    return error("DXF bulge precedes its vertex.");
                                };
                                last.1 = number()?;
                            }
                            _ => {}
                        }
                    }
                    let declared = r.number(90, vertices.len() as f64)?;
                    if declared != vertices.len() as f64 {
                        return error("DXF polyline vertex count differs from its declaration.");
                    }
                    self.polyline(layer, transform * ocs(r)?, &vertices, (r.number(70, 0.)? as u32) & 1 != 0)?;
                }
                "POLYLINE" => {
                    let flags = r.number(70, 0.)? as u32;
                    if flags & (16 | 64) != 0 {
                        return error("DXF polygon/polyface meshes are not supported; use GLB or OBJ.");
                    }
                    let mut vertices = Vec::new();
                    while i < records.len() && records[i].kind == "VERTEX" {
                        vertices.push((records[i].point(10)?, records[i].number(42, 0.)?));
                        i += 1;
                    }
                    if i >= records.len() || records[i].kind != "SEQEND" {
                        return error("Unterminated DXF POLYLINE.");
                    }
                    i += 1;
                    let t = if flags & 8 != 0 { transform } else { transform * ocs(r)? };
                    self.polyline(layer, t, &vertices, flags & 1 != 0)?;
                }
                "CIRCLE" | "ARC" => {
                    let start = r.number(50, 0.)?.to_radians();
                    let sweep = if r.kind == "CIRCLE" {
                        std::f64::consts::TAU
                    } else {
                        (r.number(51, 0.)?.to_radians() - start).rem_euclid(std::f64::consts::TAU)
                    };
                    self.arc(
                        layer,
                        transform * ocs(r)?,
                        Arc { centre: r.point(10)?, radius: r.number(40, 0.)?, start, sweep },
                    )?;
                }
                "3DFACE" | "SOLID" | "TRACE" => {
                    let mut points = vec![r.point(10)?, r.point(11)?, r.point(12)?];
                    if r.text(13).is_some() {
                        let fourth = r.point(13)?;
                        if fourth.distance(points[2]) > 1e-9 {
                            points.push(fourth);
                        }
                    }
                    if r.kind != "3DFACE" && points.len() == 4 {
                        points.swap(2, 3);
                    }
                    let points: Vec<_> = points.into_iter().map(|p| transform.transform_point3(p)).collect();
                    self.layers.entry(layer.into()).or_default().polygon(&points, None, None)?;
                }
                "INSERT" => {
                    let name = r.text(2).ok_or_else(|| ImportError("Unnamed DXF INSERT.".into()))?;
                    let block = self
                        .blocks
                        .get(name)
                        .ok_or_else(|| ImportError(format!("DXF block {name} is missing.")))?
                        .clone();
                    let scale = DVec3::new(r.number(41, 1.)?, r.number(42, 1.)?, r.number(43, 1.)?);
                    if scale.abs().min_element() < 1e-12 {
                        return error("DXF INSERT has zero scale.");
                    }
                    let count_x = r.number(70, 1.)?;
                    let count_y = r.number(71, 1.)?;
                    if count_x < 1.
                        || count_y < 1.
                        || count_x.fract() != 0.
                        || count_y.fract() != 0.
                        || count_x * count_y > 10_000.
                    {
                        return error("DXF INSERT array is too large.");
                    }
                    let frame = transform
                        * ocs(r)?
                        * DMat4::from_translation(r.point(10)?)
                        * DMat4::from_rotation_z(r.number(50, 0.)?.to_radians());
                    for y in 0..count_y as usize {
                        for x in 0..count_x as usize {
                            self.remaining = self
                                .remaining
                                .checked_sub(1)
                                .ok_or_else(|| ImportError("DXF expansion limit exceeded.".into()))?;
                            let offset = DVec3::new(x as f64 * r.number(44, 0.)?, y as f64 * r.number(45, 0.)?, 0.);
                            let t = frame
                                * DMat4::from_translation(offset)
                                * DMat4::from_scale(scale)
                                * DMat4::from_translation(-block.base);
                            self.records(&block.records, t, layer, depth + 1)?;
                        }
                    }
                }
                "SEQEND" | "VERTEX" => return error("Unexpected DXF polyline record."),
                other => {
                    self.ignored.insert(other.into());
                }
            }
        }
        Ok(())
    }
}
struct Arc {
    centre: DVec3,
    radius: f64,
    start: f64,
    sweep: f64,
}
fn ocs(record: &Record) -> Result<DMat4> {
    if record.text(210).is_none() && record.text(220).is_none() && record.text(230).is_none() {
        return Ok(DMat4::IDENTITY);
    }
    let n = DVec3::new(record.number(210, 0.)?, record.number(220, 0.)?, record.number(230, 1.)?)
        .try_normalize()
        .ok_or_else(|| ImportError("Invalid DXF extrusion direction.".into()))?;
    let x =
        if n.x.abs() < 1. / 64. && n.y.abs() < 1. / 64. { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }.normalize();
    Ok(DMat4::from_cols(x.extend(0.), n.cross(x).extend(0.), n.extend(0.), rysmo_math::DVec4::W))
}
pub(super) fn read(text: &str, options: Options) -> Result<Imported> {
    if text.starts_with("AutoCAD Binary DXF") {
        return error("Binary DXF is not supported. Export ASCII DXF.");
    }
    let lines: Vec<_> = text.trim_start_matches('\u{feff}').lines().collect();
    if !lines.len().is_multiple_of(2) {
        return error("DXF has an incomplete group-code pair.");
    }
    let mut pairs = Vec::new();
    for pair in lines.as_chunks::<2>().0 {
        pairs.push((
            pair[0].trim().parse::<i16>().map_err(|_| ImportError("Invalid DXF group code.".into()))?,
            pair[1].trim().to_owned(),
        ));
    }
    let unit = pairs
        .windows(2)
        .find(|p| p[0].0 == 9 && p[0].1 == "$INSUNITS")
        .and_then(|p| p[1].1.parse::<u32>().ok())
        .unwrap_or(0);
    let file_scale = match unit {
        1 => 0.0254,
        2 => 0.3048,
        3 => 1609.344,
        4 => 0.001,
        5 => 0.01,
        6 => 1.,
        7 => 1000.,
        8 => 0.0000000254,
        9 => 0.0000254,
        10 => 0.9144,
        11 => 1e-10,
        12 => 1e-9,
        13 => 1e-6,
        14 => 0.1,
        15 => 10.,
        16 => 100.,
        17 => 1e9,
        18 => 149597870700.,
        19 => 9.4607304725808e15,
        20 => 3.08567758149137e16,
        21 => 1200. / 3937.,
        22 => 100. / 3937.,
        23 => 3600. / 3937.,
        24 => 6336000. / 3937.,
        _ => 1.,
    };
    if !(0..=24).contains(&unit) && options.unit_scale.is_none() {
        return error("Unknown DXF units; choose source units explicitly.");
    }
    let scale = options.unit_scale.unwrap_or(file_scale);
    if !scale.is_finite() || scale > 1e9 {
        return error("DXF units are too large for modelling.");
    }
    let mut records = Vec::<Record>::new();
    for (code, value) in pairs {
        if code == 0 {
            records.push(Record { kind: value, data: Vec::new() });
        } else if let Some(last) = records.last_mut() {
            last.data.push((code, value));
        }
    }
    let mut reader = Reader {
        blocks: BTreeMap::new(),
        layers: BTreeMap::new(),
        hidden: BTreeSet::new(),
        ignored: BTreeSet::new(),
        remaining: 400_000,
        segments: options.curve_segments,
    };
    let mut entities = Vec::new();
    let mut section = String::new();
    let mut i = 0;
    while i < records.len() {
        let record = &records[i];
        i += 1;
        match record.kind.as_str() {
            "SECTION" => section = record.text(2).unwrap_or("").into(),
            "ENDSEC" => section.clear(),
            "BLOCK" if section == "BLOCKS" => {
                let name = record.text(2).ok_or_else(|| ImportError("Unnamed DXF block.".into()))?.to_owned();
                let mut block = Block { base: record.point(10)?, records: Vec::new() };
                while i < records.len() && records[i].kind != "ENDBLK" {
                    block.records.push(records[i].clone());
                    i += 1;
                }
                if i == records.len() {
                    return error("Unterminated DXF block.");
                }
                i += 1;
                reader.blocks.insert(name, block);
            }
            "LAYER" if section == "TABLES" => {
                if record.number(62, 7.)? < 0. || (record.number(70, 0.)? as u32) & 1 != 0 {
                    reader.hidden.insert(record.text(2).unwrap_or("0").into());
                }
            }
            _ if section == "ENTITIES" => entities.push(record.clone()),
            _ => {}
        }
    }
    reader.records(&entities, DMat4::from_scale(DVec3::splat(scale)), "0", 0)?;
    let mut document = Document::new();
    for (layer, geometry) in reader.layers {
        let mesh = geometry.finish()?;
        if mesh.vertex_count() == 0 {
            continue;
        }
        let tag = TagId(document.reserve_id());
        document.tags.insert(tag, Tag { name: layer.clone(), visible: !reader.hidden.contains(&layer) });
        let mesh = mesh.with_fresh_ids(|| document.reserve_id())?;
        let definition = DefinitionId(document.reserve_id());
        document
            .definitions
            .insert(definition, Definition { name: layer.clone(), group: true, mesh, ..Default::default() }.into());
        let instance = InstanceId(document.reserve_id());
        document.root_mut().instances.insert(
            instance,
            Instance {
                definition,
                transform: DMat4::IDENTITY,
                name: layer,
                tag: Some(tag),
                material: None,
                hidden: false,
                glued_to: None,
                attributes: Default::default(),
            },
        );
    }
    let mut warnings = Vec::new();
    if unit == 0 && options.unit_scale.is_none() {
        warnings.push("DXF has no units; metres were assumed. Choose source units if necessary.".into());
    }
    if !reader.ignored.is_empty() {
        warnings.push(format!(
            "Unsupported DXF entities: {}. Their contents were not imported.",
            reader.ignored.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(Imported { document, warnings })
}
