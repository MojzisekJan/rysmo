//! Open native models and save them again; report any that do not come back identical.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.is_empty() {
        return Err("Usage: round_trip model.rysmo...".into());
    }
    let mut changed = 0;
    for path in &paths {
        let text = std::fs::read_to_string(path)?;
        let doc = rysmo_io::load(&text)?;
        let saved = rysmo_io::save(&doc);
        let same = saved == text && rysmo_io::load(&saved)? == doc;
        println!("{} {}", if same { "same   " } else { "CHANGED" }, path.to_string_lossy());
        changed += usize::from(!same);
    }
    if changed > 0 {
        return Err(format!("{changed} of {} models changed", paths.len()).into());
    }
    Ok(())
}
