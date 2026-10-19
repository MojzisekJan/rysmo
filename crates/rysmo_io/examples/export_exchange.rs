//! Export an existing native model for independent format validation.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Usage: export_exchange input.rysmo output-directory".into());
    }
    let doc = rysmo_io::load(&std::fs::read_to_string(&args[0])?)?;
    let directory = std::path::Path::new(&args[1]);
    std::fs::create_dir_all(directory)?;
    let out = rysmo_io::collada::export(&doc);
    std::fs::write(directory.join("model.dae"), &out.dae)?;
    std::fs::write(directory.join("model.dxf"), rysmo_io::dxf::export(&doc))?;
    let mut files = out.images;
    for (name, bytes) in &files {
        std::fs::write(directory.join(name), bytes)?;
    }
    files.insert(0, ("model.dae".into(), out.dae.into_bytes()));
    std::fs::write(directory.join("model-collada.zip"), rysmo_io::archive::zip(&files)?)?;
    Ok(())
}
