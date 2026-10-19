//! Small ZIP32 bundle for browser exports with companion texture files.
//! Entries are stored without compression to keep exporter dependencies small.

pub fn zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(files.len()).map_err(|_| "Too many export files for ZIP32.")?;
    let mut out = Vec::new();
    let mut directory = Vec::new();
    let u32_size = |n| u32::try_from(n).map_err(|_| "Export is too large for ZIP32.".to_owned());
    for (name, bytes) in files {
        if name.is_empty() || name == "." || name == ".." || name.chars().any(|c| c.is_control() || "/\\:".contains(c))
        {
            return Err("Export bundle names must be plain file names.".into());
        }
        let name_size = u16::try_from(name.len()).map_err(|_| "Export file name is too long.")?;
        let size = u32_size(bytes.len())?;
        let offset = u32_size(out.len())?;
        let crc = crc32fast::hash(bytes);
        let word = |out: &mut Vec<u8>, n: u16| out.extend_from_slice(&n.to_le_bytes());
        let long = |out: &mut Vec<u8>, n: u32| out.extend_from_slice(&n.to_le_bytes());
        long(&mut out, 0x04034b50);
        word(&mut out, 20);
        word(&mut out, 0x800);
        word(&mut out, 0);
        word(&mut out, 0);
        word(&mut out, 0x21);
        long(&mut out, crc);
        long(&mut out, size);
        long(&mut out, size);
        word(&mut out, name_size);
        word(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(bytes);
        long(&mut directory, 0x02014b50);
        word(&mut directory, 20);
        word(&mut directory, 20);
        word(&mut directory, 0x800);
        word(&mut directory, 0);
        word(&mut directory, 0);
        word(&mut directory, 0x21);
        long(&mut directory, crc);
        long(&mut directory, size);
        long(&mut directory, size);
        word(&mut directory, name_size);
        word(&mut directory, 0);
        word(&mut directory, 0);
        word(&mut directory, 0);
        word(&mut directory, 0);
        long(&mut directory, 0);
        long(&mut directory, offset);
        directory.extend_from_slice(name.as_bytes());
    }
    let offset = u32_size(out.len())?;
    let size = u32_size(directory.len())?;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    for n in [0, 0, count, count] {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    u32_size(out.len())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_could_escape_an_extracted_bundle() {
        for name in ["", ".", "..", "../image.png", "a/b", "a\\b", "C:temp", "a\n"] {
            assert!(zip(&[(name.into(), vec![])]).is_err(), "{name:?}");
        }
        assert!(zip(&[("texture-1.png".into(), vec![1, 2, 3])]).is_ok());
    }
}
