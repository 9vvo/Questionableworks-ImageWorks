//! File-system helpers.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Same directory as the target, so the final rename stays on one
    // file system and is atomic.
    path.with_file_name(format!(".{name}.{}.{unique}.tmp", std::process::id()))
}

/// Writes a file so that `path` always holds either the complete old
/// contents or the complete new contents, never a partial write
/// (architecture rule 7).
///
/// `write` fills a temporary file next to `path`. Only once it has
/// succeeded and the data is flushed to disk is the temporary file renamed
/// over `path`. If anything fails, the temporary file is removed and
/// `path` is untouched.
pub fn atomic_write<E: From<io::Error>>(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<(), E>,
) -> Result<(), E> {
    let temp = temp_path_for(path);
    let mut file = File::options().write(true).create_new(true).open(&temp)?;
    let result = write(&mut file).and_then(|()| file.sync_all().map_err(E::from));
    drop(file);
    let result = result.and_then(|()| fs::rename(&temp, path).map_err(E::from));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
        return result;
    }
    // Make the rename itself durable. Not possible on every platform, and
    // the data is already safe, so a failure here is not reported.
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        if let Ok(dir) = File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    result
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A fresh empty directory under the system temp dir, removed on drop.
    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("iw-test-{tag}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        pub fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TempDir;
    use super::*;
    use std::io::Write;

    #[test]
    fn writes_a_new_file_and_leaves_no_temporary() {
        let dir = TempDir::new("atomic-new");
        let path = dir.file("out.bin");
        atomic_write::<io::Error>(&path, |f| f.write_all(b"hello")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        assert_eq!(dir.entries(), ["out.bin"]);
    }

    #[test]
    fn replaces_an_existing_file() {
        let dir = TempDir::new("atomic-replace");
        let path = dir.file("out.bin");
        fs::write(&path, b"old contents that are longer").unwrap();
        atomic_write::<io::Error>(&path, |f| f.write_all(b"new")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(dir.entries(), ["out.bin"]);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file_intact() {
        let dir = TempDir::new("atomic-fail");
        let path = dir.file("out.bin");
        fs::write(&path, b"precious").unwrap();
        let result = atomic_write::<io::Error>(&path, |f| {
            f.write_all(b"half of the new da")?;
            Err(io::Error::other("disk full"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"precious");
        assert_eq!(dir.entries(), ["out.bin"], "temporary file was left behind");
    }

    #[test]
    fn a_failed_first_write_creates_nothing() {
        let dir = TempDir::new("atomic-fail-new");
        let path = dir.file("out.bin");
        let result = atomic_write::<io::Error>(&path, |_| Err(io::Error::other("nope")));
        assert!(result.is_err());
        assert!(dir.entries().is_empty());
    }

    #[test]
    fn a_missing_directory_is_an_error() {
        let dir = TempDir::new("atomic-nodir");
        let path = dir.file("no/such/dir/out.bin");
        assert!(atomic_write::<io::Error>(&path, |f| f.write_all(b"x")).is_err());
    }
}
