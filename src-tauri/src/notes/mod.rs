//! Recording folders, their state, and the notes written into them.
//!
//! Each recording gets its own folder inside the user's notes folder. The
//! folder holds the audio, `note.md` once a note is generated, and app state
//! in a hidden `.anchovy/` folder so Obsidian does not show it.

pub mod folder;
pub mod note;
pub mod state;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Hidden folder inside each recording folder for app state and temp files.
pub const APP_DIR: &str = ".anchovy";

#[derive(Debug)]
pub enum NotesError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidTransition {
        from: state::Status,
        to: state::Status,
    },
}

impl fmt::Display for NotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotesError::Io(err) => write!(f, "{err}"),
            NotesError::Json(err) => write!(f, "{err}"),
            NotesError::InvalidTransition { from, to } => {
                write!(f, "cannot move from {} to {}", from.name(), to.name())
            }
        }
    }
}

impl std::error::Error for NotesError {}

impl From<io::Error> for NotesError {
    fn from(err: io::Error) -> Self {
        NotesError::Io(err)
    }
}

impl From<serde_json::Error> for NotesError {
    fn from(err: serde_json::Error) -> Self {
        NotesError::Json(err)
    }
}

/// Writes `dest` through `tmp`: `write` fills `tmp`, which is flushed to disk
/// and then renamed over `dest`. If anything fails, `dest` keeps its old
/// contents (or stays absent) and `tmp` is removed.
fn write_atomically(
    tmp: &Path,
    dest: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let result = (|| {
        let mut file = File::create(tmp)?;
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(tmp, dest)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
pub(crate) mod test_dir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh empty folder under the system temp folder, removed on drop.
    pub struct TestDir(PathBuf);

    impl TestDir {
        pub fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "anchovy-notes-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TestDir(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_dir::TestDir;
    use super::*;

    #[test]
    fn atomic_write_replaces_the_destination() {
        let dir = TestDir::new();
        let dest = dir.path().join("note.md");
        let tmp = dir.path().join("note.tmp");
        fs::write(&dest, "old").unwrap();

        write_atomically(&tmp, &dest, |file| file.write_all(b"new")).unwrap();

        assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
        assert!(!tmp.exists());
    }

    #[test]
    fn atomic_write_failure_keeps_the_old_file_and_removes_the_temp_file() {
        let dir = TestDir::new();
        let dest = dir.path().join("note.md");
        let tmp = dir.path().join("note.tmp");
        fs::write(&dest, "old").unwrap();

        let result = write_atomically(&tmp, &dest, |file| {
            file.write_all(b"half")?;
            Err(io::Error::other("disk full"))
        });

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
        assert!(!tmp.exists());
    }
}
