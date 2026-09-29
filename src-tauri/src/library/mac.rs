//! The macOS calls the library needs: moving a folder to the Trash and
//! showing a folder in Finder. The notes folder itself comes from
//! `folder_access`. Kept thin; the logic
//! lives in `library.rs`.

use std::io;
use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSFileManager, NSString, NSURL};

fn file_url(path: &Path) -> io::Result<objc2::rc::Retained<NSURL>> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    Ok(NSURL::fileURLWithPath(&NSString::from_str(path)))
}

/// Moves `path` to the Trash, where the user can put it back, and returns
/// where it ended up. Never deletes.
pub fn trash(path: &Path) -> io::Result<PathBuf> {
    let url = file_url(path)?;
    let mut resulting = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|err| io::Error::other(err.localizedDescription().to_string()))?;
    Ok(resulting
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .unwrap_or_default())
}

/// Opens a Finder window with `path` selected.
pub fn reveal(path: &Path) -> io::Result<()> {
    let url = file_url(path)?;
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;
    use std::fs;

    #[test]
    fn trash_moves_the_folder_and_keeps_its_contents() {
        let dir = TestDir::new();
        let folder = dir.path().join("2026-09-26-1410");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("audio.wav"), b"RIFF").unwrap();

        let trashed = trash(&folder).unwrap();

        assert!(!folder.exists());
        assert_eq!(fs::read(trashed.join("audio.wav")).unwrap(), b"RIFF");
        // Tidy up the user's Trash after the test.
        fs::remove_dir_all(trashed).unwrap();
    }

    #[test]
    fn trashing_a_missing_folder_is_an_error() {
        let dir = TestDir::new();
        assert!(trash(&dir.path().join("missing")).is_err());
    }
}
