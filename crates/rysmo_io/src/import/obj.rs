use super::{ImportError, Imported, Options, Result, UpAxis, error, geometry::Geometry};
use rysmo_math::{DVec2, DVec3};
use rysmo_model::{Definition, Document};

fn index(text: &str, count: usize) -> Result<usize> {
    let value: i64 = text.parse().map_err(|_| ImportError("Invalid OBJ index.".into()))?;
    let index = if value < 0 { count as i64 + value } else { value - 1 };
    if value == 0 || index < 0 || index >= count as i64 {
        return error("OBJ index is out of range.");
    }
    Ok(index as usize)
}
pub(super) fn read(text: &str, options: Options) -> Result<Imported> {
    let mut positions = Vec::new();
    let mut uv = Vec::new();
    let mut geometry = Geometry::default();
    let mut external_materials = false;
    let mut unsupported = std::collections::BTreeSet::new();
    let scale = options.unit_scale.unwrap_or(1.);
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("");
        let mut words = line.split_whitespace();
        let Some(kind) = words.next() else { continue };
        let words: Vec<_> = words.collect();
        let number = |i: usize| -> Result<f64> {
            words
                .get(i)
                .ok_or_else(|| ImportError(format!("Incomplete OBJ line {}.", line_no + 1)))?
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| ImportError(format!("Invalid OBJ number at line {}.", line_no + 1)))
        };
        match kind {
            "v" => {
                if positions.len() >= 200_000 {
                    return error("OBJ exceeds the vertex limit.");
                }
                let p = DVec3::new(number(0)?, number(1)?, number(2)?) * scale;
                positions.push(if options.obj_up == UpAxis::Y { DVec3::new(p.x, -p.z, p.y) } else { p });
            }
            "vt" => {
                if uv.len() >= 600_000 {
                    return error("OBJ exceeds the UV limit.");
                }
                uv.push(DVec2::new(number(0)?, 1. - number(1)?));
            }
            "f" => {
                if words.len() > 16_384 {
                    return error("OBJ face is too large.");
                }
                let refs = words
                    .iter()
                    .map(|w| {
                        let mut values = w.split('/');
                        let vertex = index(values.next().unwrap_or(""), positions.len())?;
                        let texture =
                            values.next().filter(|s| !s.is_empty()).map(|s| index(s, uv.len())).transpose()?;
                        Ok((vertex, texture))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let points: Vec<_> = refs.iter().map(|(i, _)| positions[*i]).collect();
                let tex: Option<Vec<_>> = refs.iter().map(|(_, i)| i.map(|i| uv[i])).collect();
                geometry.polygon(&points, tex.as_deref(), None)?;
            }
            "l" => {
                let refs = words
                    .iter()
                    .map(|s| index(s.split('/').next().unwrap_or(""), positions.len()))
                    .collect::<Result<Vec<_>>>()?;
                for pair in refs.windows(2) {
                    geometry.line(positions[pair[0]], positions[pair[1]])?;
                }
            }
            "mtllib" | "usemtl" => external_materials = true,
            "vn" | "o" | "g" | "s" => {}
            other => {
                unsupported.insert(other.to_owned());
            }
        }
    }
    let mut warnings = Vec::new();
    if external_materials {
        warnings.push(
            "External OBJ MTL files and image companions are not loaded. Geometry and UV coordinates are retained."
                .into(),
        );
    }
    if !unsupported.is_empty() {
        warnings.push(format!("Unsupported OBJ records: {}", unsupported.into_iter().collect::<Vec<_>>().join(", ")));
    }
    let mesh = geometry.finish()?;
    let mut document = Document::new();
    document.root = Definition { mesh, ..Default::default() };
    Ok(Imported { document, warnings })
}
