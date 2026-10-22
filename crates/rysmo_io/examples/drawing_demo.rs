//! Reproducible vector export fixture: dimensions, Unicode labels and physical scale.
use rysmo_math::DVec3;
use rysmo_model::{
    Command, Editor,
    dimensions::{Dimension, DimensionAnchor, DimensionMeasure},
    text::{Text, TextAnchor, TextLeader, TextPlacement},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).unwrap_or_else(|| "target/drawing-demo".into());
    std::fs::create_dir_all(&output)?;
    let mut editor = Editor::new();
    for (corner, u, v) in
        [(DVec3::ZERO, DVec3::X * 8., DVec3::Y * 5.), (DVec3::new(0.2, 0.2, 0.), DVec3::X * 7.6, DVec3::Y * 4.6)]
    {
        editor.apply(Command::DrawRectangle { corner, u, v, facing: Some(DVec3::Z) })?;
    }
    for (to, offset) in [(DVec3::X * 8., DVec3::NEG_Y * 0.5), (DVec3::Y * 5., DVec3::NEG_X * 0.5)] {
        editor.apply(Command::AddDimension {
            dimension: Dimension {
                measure: DimensionMeasure::Linear {
                    from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                    to: DimensionAnchor { point: to, reference: None },
                    axis: None,
                },
                normal: DVec3::Z,
                offset,
                text: String::new(),
                alignment: Default::default(),
                font: Default::default(),
                text_position: Default::default(),
                endpoints: Default::default(),
            },
        })?;
    }
    editor.apply(Command::AddText {
        text: Text {
            content: "Room A - 40 m²\nCafé / přízemí".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor { point: DVec3::new(3., 2.5, 0.), reference: None },
                leader: TextLeader::None { offset: DVec3::ZERO },
            },
            font: Default::default(),
            endpoint: Default::default(),
        },
    })?;
    let drawing = rysmo_io::drawing::build(
        editor.doc(),
        &rysmo_io::drawing::Options { title: "Ground floor".into(), ..Default::default() },
    )?;
    std::fs::write(std::path::Path::new(&output).join("ground-floor.pdf"), drawing.pdf())?;
    std::fs::write(std::path::Path::new(&output).join("ground-floor.svg"), drawing.svg())?;
    Ok(())
}
