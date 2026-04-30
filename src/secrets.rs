//! Atomic write helper for files holding sensitive content.
//!
//! `write_secret_file` writes via a temp file + rename so partial writes
//! are never observable, and on Unix opens the temp file with mode 0600
//! so the bytes are never world-readable even between create and rename.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Atomically write `contents` to `path`. Creates parent dirs as needed.
/// On Unix the temp file is opened with mode 0o600.
pub fn write_secret_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let tmp_path = match path.file_name() {
        Some(name) => {
            let mut tmp_name = name.to_os_string();
            tmp_name.push(".tmp");
            path.with_file_name(tmp_name)
        }
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write_secret_file: path has no file name",
            ));
        }
    };

    {
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut f = opts.open(&tmp_path)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Ensure a sensitive append-only file exists with mode 0600 set at
/// creation time. Subsequent appends inherit the mode. No-op if the
/// file already exists.
pub fn ensure_secret_file(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_secret_file(path, b"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_write_secret_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("bcf-secrets-{}", std::process::id()));
        let path = dir.join("token");
        write_secret_file(&path, b"hunter2").unwrap();

        let mut s = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "hunter2");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_secret_file_overwrites() {
        let dir = std::env::temp_dir().join(format!("bcf-secrets2-{}", std::process::id()));
        let path = dir.join("data");
        write_secret_file(&path, b"first").unwrap();
        write_secret_file(&path, b"second").unwrap();

        let s = std::fs::read_to_string(&path).unwrap();
        assert_eq!(s, "second");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn test_write_secret_file_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("bcf-secrets3-{}", std::process::id()));
        let path = dir.join("locked");
        write_secret_file(&path, b"x").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0600, got {:o}", mode);

        std::fs::remove_dir_all(&dir).ok();
    }
}
