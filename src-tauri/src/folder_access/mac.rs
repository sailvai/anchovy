//! The macOS calls behind the notes folder: security-scoped bookmarks, the
//! folder panels, and the user's real home folder. Kept thin; the logic lives
//! in `folder_access.rs`.

use std::ffi::CStr;
use std::io;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{
    NSData, NSError, NSString, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSURL,
};

use super::{Bookmarks, Opened};

fn ns_error(err: Retained<NSError>) -> io::Error {
    io::Error::other(err.localizedDescription().to_string())
}

fn folder_url(path: &Path) -> io::Result<Retained<NSURL>> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is not UTF-8"))?;
    Ok(NSURL::fileURLWithPath_isDirectory(
        &NSString::from_str(path),
        true,
    ))
}

fn url_path(url: &NSURL) -> Option<PathBuf> {
    url.path().map(|path| PathBuf::from(path.to_string()))
}

/// Security-scoped bookmarks, which let the sandboxed app back into a folder
/// the user chose in an earlier launch.
pub struct MacBookmarks;

impl Bookmarks for MacBookmarks {
    fn create(&self, folder: &Path) -> io::Result<Vec<u8>> {
        let data = folder_url(folder)?
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::WithSecurityScope,
                None,
                None,
            )
            .map_err(ns_error)?;
        Ok(data.to_vec())
    }

    fn open(&self, data: &[u8]) -> io::Result<Opened> {
        let data = NSData::with_bytes(data);
        let mut stale = Bool::NO;
        // SAFETY: `data` is valid bookmark data or the call returns an error;
        // `stale` is a valid out-parameter.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope,
                None,
                &mut stale,
            )
        }
        .map_err(ns_error)?;
        // Access stays open for the life of the app. Outside the sandbox this
        // returns false and is not needed.
        // SAFETY: `url` came from a security-scoped bookmark.
        let _ = unsafe { url.startAccessingSecurityScopedResource() };
        let folder =
            url_path(&url).ok_or_else(|| io::Error::other("the bookmark has no file path"))?;
        Ok(Opened {
            folder,
            stale: stale.as_bool(),
        })
    }
}

/// The user's real home folder. Inside the sandbox `HOME` is the app's
/// container, so the password database is asked instead.
pub fn user_home() -> io::Result<PathBuf> {
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: plain data, filled in by getpwuid_r.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result = std::ptr::null_mut();
    // SAFETY: `pwd`, `buf`, and `result` are valid for the call.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the home folder is unknown",
        ));
    }
    // SAFETY: pw_dir points into `buf`, which is still alive.
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
    Ok(PathBuf::from(dir.to_string_lossy().into_owned()))
}

/// `~/Library/Application Support/Anchovy`. Inside the app sandbox `HOME` is
/// the app's container, so this stays inside it.
pub fn support_dir() -> io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
    Ok(PathBuf::from(home).join("Library/Application Support/Anchovy"))
}

/// Asks the user to choose a folder, starting in `start`. Returns `None` if
/// they cancel. Must run on the main thread.
pub fn choose_folder(mtm: MainThreadMarker, start: &Path) -> Option<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setMessage(Some(&NSString::from_str(
        "Choose a folder for your notes, or an existing Obsidian vault.",
    )));
    panel.setPrompt(Some(&NSString::from_str("Choose")));
    if let Ok(url) = folder_url(start) {
        panel.setDirectoryURL(Some(&url));
    }
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    panel.URL().and_then(|url| url_path(&url))
}

/// Asks the user to confirm `folder`, which the sandbox will not let Anchovy
/// create or open on its own. An existing folder is confirmed in an open
/// panel; a new one in a save panel that grants access to create it.
pub fn confirm_folder(mtm: MainThreadMarker, folder: &Path) -> Option<PathBuf> {
    let message = NSString::from_str("Anchovy saves your recordings and notes in this folder.");
    let prompt = NSString::from_str("Use Folder");
    let url = if folder.is_dir() {
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setMessage(Some(&message));
        panel.setPrompt(Some(&prompt));
        panel.setDirectoryURL(Some(&*folder_url(folder).ok()?));
        (panel.runModal() == NSModalResponseOK).then(|| panel.URL())??
    } else {
        let panel = NSSavePanel::savePanel(mtm);
        let name = folder.file_name()?.to_str()?;
        panel.setNameFieldStringValue(&NSString::from_str(name));
        panel.setShowsTagField(false);
        panel.setCanCreateDirectories(true);
        panel.setMessage(Some(&message));
        panel.setPrompt(Some(&prompt));
        if let Some(parent) = folder.parent() {
            panel.setDirectoryURL(Some(&*folder_url(parent).ok()?));
        }
        (panel.runModal() == NSModalResponseOK).then(|| panel.URL())??
    };
    url_path(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_is_the_users_not_a_container() {
        let home = user_home().unwrap();
        assert!(home.is_absolute());
        assert!(!home.to_string_lossy().contains("/Library/Containers/"));
        assert!(home.is_dir());
    }

    #[test]
    fn app_state_lives_under_application_support() {
        assert!(support_dir()
            .unwrap()
            .ends_with("Library/Application Support/Anchovy"));
    }

    #[test]
    fn broken_bookmark_data_is_an_error() {
        assert!(MacBookmarks.open(b"not a bookmark").is_err());
    }
}
