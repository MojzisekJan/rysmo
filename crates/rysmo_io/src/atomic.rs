//! Replace a model only after its complete contents have reached a sibling file.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Write in the destination directory, then atomically replace the destination.
/// Existing symlinks are followed and existing file permissions are retained.
/// An error before replacement leaves the previous file intact.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, |file| file.write_all(bytes))
}

fn replace_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    let path = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)?,
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error),
    };
    let permissions = match fs::metadata(&path) {
        Ok(meta) => {
            if !meta.is_file() || meta.permissions().readonly() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "destination is not a writable file"));
            }
            Some(meta.permissions())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if path.file_name().is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "missing file name"));
    }
    let (temp, mut file) = loop {
        let name = format!(".rysmo-save-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        let candidate = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => break (Temporary(candidate), file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    write(&mut file)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp.0, &path)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rysmo-save-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn model(&self) -> PathBuf {
            self.0.join("model.rysmo")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn creates_and_replaces_without_leaving_temporary_files() {
        let dir = Directory::new();
        write(&dir.model(), b"first model").unwrap();
        write(&dir.model(), b"second").unwrap();
        assert_eq!(fs::read(dir.model()).unwrap(), b"second");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
    #[test]
    fn partial_write_failure_keeps_the_original_and_removes_temporary_file() {
        let dir = Directory::new();
        fs::write(dir.model(), b"original model").unwrap();
        let result = replace_with(&dir.model(), |file| {
            file.write_all(b"incomplete")?;
            Err(io::Error::other("simulated full disk"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(dir.model()).unwrap(), b"original model");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
    #[test]
    fn failed_first_save_does_not_create_a_truncated_model() {
        let dir = Directory::new();
        assert!(replace_with(&dir.model(), |_| Err(io::Error::other("write failed"))).is_err());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn accepts_long_destination_names() {
        let dir = Directory::new();
        let path = dir.0.join(format!("{}.rysmo", "m".repeat(240)));
        write(&path, b"first").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"second");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
    #[cfg(unix)]
    #[test]
    fn preserves_symlinks_and_file_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = Directory::new();
        fs::write(dir.model(), b"old").unwrap();
        fs::set_permissions(dir.model(), fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.0.join("linked.rysmo");
        symlink("model.rysmo", &link).unwrap();
        write(&link, b"new").unwrap();
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(dir.model()).unwrap(), b"new");
        assert_eq!(fs::metadata(dir.model()).unwrap().permissions().mode() & 0o777, 0o640);
    }
}
