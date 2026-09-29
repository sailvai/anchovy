//! The notes folder the user chose on first launch. The app sandbox only lets
//! Anchovy into folders the user picked, so the choice is kept as a
//! security-scoped bookmark in the app's container and opened again at each
//! launch. The folder may be an existing Obsidian vault; recordings then go in
//! their own folders next to the user's notes, and the library scan leaves
//! everything else alone.

pub mod commands;
pub mod mac;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where the bookmark is saved, inside the app's support folder.
pub const BOOKMARK_FILE: &str = "notes-folder.bookmark";

/// The notes folder offered on first launch, relative to the user's home.
pub const DEFAULT_FOLDER: &str = "Documents/Anchovy";

/// Obsidian keeps its settings in this folder at the root of every vault.
const OBSIDIAN_DIR: &str = ".obsidian";

/// Saves and reopens access to a folder across launches. `mac::MacBookmarks`
/// uses security-scoped bookmarks.
pub trait Bookmarks {
    /// Bookmark data that reopens `folder` in a later launch.
    fn create(&self, folder: &Path) -> io::Result<Vec<u8>>;
    /// The folder `data` points to, with access to it started.
    fn open(&self, data: &[u8]) -> io::Result<Opened>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub folder: PathBuf,
    /// The folder moved or was renamed; the bookmark should be saved again.
    pub stale: bool,
}

/// The notes folder as the first-launch screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotesFolder {
    pub path: PathBuf,
    /// The path with the home folder written as `~`.
    pub display: String,
    pub exists: bool,
    pub obsidian_vault: bool,
}

/// `~/Documents/Anchovy` for the user whose home is `home`.
pub fn default_folder(home: &Path) -> PathBuf {
    home.join(DEFAULT_FOLDER)
}

pub fn is_obsidian_vault(folder: &Path) -> bool {
    folder.join(OBSIDIAN_DIR).is_dir()
}

pub fn describe(folder: &Path, home: &Path) -> NotesFolder {
    let display = match folder.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    };
    NotesFolder {
        path: folder.to_path_buf(),
        display,
        exists: folder.is_dir(),
        obsidian_vault: is_obsidian_vault(folder),
    }
}

/// The saved choice of notes folder.
pub struct FolderAccess<B> {
    support_dir: PathBuf,
    bookmarks: B,
}

impl<B: Bookmarks> FolderAccess<B> {
    /// `support_dir` is where the bookmark is saved.
    pub fn new(support_dir: PathBuf, bookmarks: B) -> Self {
        FolderAccess {
            support_dir,
            bookmarks,
        }
    }

    fn bookmark_path(&self) -> PathBuf {
        self.support_dir.join(BOOKMARK_FILE)
    }

    /// The saved notes folder with access to it started. `None` when no
    /// folder was chosen, or the saved one can no longer be found, so the
    /// first-launch screen asks again.
    pub fn restore(&self) -> io::Result<Option<PathBuf>> {
        let data = match fs::read(self.bookmark_path()) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let Ok(opened) = self.bookmarks.open(&data) else {
            return Ok(None);
        };
        if !opened.folder.is_dir() {
            return Ok(None);
        }
        if opened.stale {
            self.save(&opened.folder)?;
        }
        Ok(Some(opened.folder))
    }

    /// Makes `folder` the notes folder, creating it if it does not exist,
    /// and saves the bookmark. Existing files in it are never touched.
    pub fn choose(&self, folder: &Path) -> io::Result<PathBuf> {
        fs::create_dir_all(folder)?;
        self.save(folder)?;
        Ok(folder.to_path_buf())
    }

    fn save(&self, folder: &Path) -> io::Result<()> {
        let data = self.bookmarks.create(folder)?;
        fs::create_dir_all(&self.support_dir)?;
        let tmp = self.support_dir.join(format!("{BOOKMARK_FILE}.tmp"));
        crate::notes::write_atomically(&tmp, &self.bookmark_path(), |file| {
            io::Write::write_all(file, &data)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::mac::MacBookmarks;
    use super::*;
    use crate::library::Library;
    use crate::notes::state::{write_state, State, Status};
    use crate::notes::test_dir::TestDir;
    use std::cell::Cell;

    /// Bookmarks that store the path itself, and can pretend a bookmark went
    /// stale or no longer opens.
    #[derive(Default)]
    struct FakeBookmarks {
        stale: Cell<bool>,
        broken: Cell<bool>,
        created: Cell<usize>,
    }

    impl Bookmarks for FakeBookmarks {
        fn create(&self, folder: &Path) -> io::Result<Vec<u8>> {
            self.created.set(self.created.get() + 1);
            Ok(folder.to_str().unwrap().as_bytes().to_vec())
        }

        fn open(&self, data: &[u8]) -> io::Result<Opened> {
            if self.broken.get() {
                return Err(io::Error::other("the bookmark does not resolve"));
            }
            Ok(Opened {
                folder: PathBuf::from(std::str::from_utf8(data).unwrap()),
                stale: self.stale.get(),
            })
        }
    }

    /// A vault as Obsidian leaves it: settings, notes, attachments, and one
    /// recording Anchovy made earlier.
    fn obsidian_vault(root: &Path) -> PathBuf {
        let vault = root.join("Work vault");
        fs::create_dir_all(vault.join(".obsidian/plugins")).unwrap();
        fs::write(vault.join(".obsidian/app.json"), "{}").unwrap();
        fs::write(vault.join("Welcome.md"), "# Welcome\n").unwrap();
        fs::create_dir_all(vault.join("Daily notes")).unwrap();
        fs::write(vault.join("Daily notes/2026-09-25.md"), "- standup\n").unwrap();
        fs::create_dir_all(vault.join("attachments")).unwrap();
        // Named like a recording, but Anchovy did not make it.
        fs::create_dir_all(vault.join("2026-09-20-0900")).unwrap();
        let recording = vault.join("2026-09-26-1410");
        let mut state = State::new();
        state.move_to(Status::Saved).unwrap();
        write_state(&recording, &state).unwrap();
        fs::write(recording.join("audio.wav"), b"").unwrap();
        vault
    }

    /// The sandbox lets Anchovy into the folder the user picks and keep it
    /// across launches, and nothing more: no other file access entitlement.
    #[test]
    fn the_bundle_has_only_the_entitlements_it_needs() {
        let entitlements = include_str!("../Entitlements.plist");
        let keys: Vec<&str> = entitlements
            .split("<key>")
            .skip(1)
            .map(|rest| &rest[..rest.find("</key>").unwrap()])
            .collect();
        assert_eq!(
            keys,
            [
                "com.apple.security.app-sandbox",
                "com.apple.security.device.audio-input",
                "com.apple.security.files.bookmarks.app-scope",
                "com.apple.security.files.user-selected.read-write",
                "com.apple.security.network.client",
            ]
        );
        assert_eq!(entitlements.matches("<true/>").count(), keys.len());
    }

    #[test]
    fn the_default_folder_is_anchovy_in_documents() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            default_folder(home),
            PathBuf::from("/Users/someone/Documents/Anchovy")
        );
        assert_eq!(
            describe(&default_folder(home), home).display,
            "~/Documents/Anchovy"
        );
    }

    #[test]
    fn a_folder_outside_home_is_shown_in_full() {
        let folder = describe(Path::new("/Volumes/Notes"), Path::new("/Users/someone"));
        assert_eq!(folder.display, "/Volumes/Notes");
        assert!(!folder.exists);
    }

    #[test]
    fn an_obsidian_vault_is_recognised_by_its_settings_folder() {
        let dir = TestDir::new();
        let vault = obsidian_vault(dir.path());
        assert!(is_obsidian_vault(&vault));
        assert!(describe(&vault, dir.path()).obsidian_vault);
        assert!(!is_obsidian_vault(dir.path()));
    }

    #[test]
    fn nothing_is_restored_before_a_folder_is_chosen() {
        let dir = TestDir::new();
        let access = FolderAccess::new(dir.path().join("support"), FakeBookmarks::default());
        assert_eq!(access.restore().unwrap(), None);
    }

    #[test]
    fn choosing_the_default_creates_it_and_it_is_restored_next_launch() {
        let dir = TestDir::new();
        let home = dir.path().join("home");
        let support = dir.path().join("support");
        let folder = default_folder(&home);

        let chosen = FolderAccess::new(support.clone(), FakeBookmarks::default())
            .choose(&folder)
            .unwrap();

        assert_eq!(chosen, folder);
        assert!(folder.is_dir());
        let next_launch = FolderAccess::new(support, FakeBookmarks::default());
        assert_eq!(next_launch.restore().unwrap(), Some(folder));
    }

    #[test]
    fn choosing_a_vault_keeps_every_file_in_it() {
        let dir = TestDir::new();
        let vault = obsidian_vault(dir.path());
        let access = FolderAccess::new(dir.path().join("support"), FakeBookmarks::default());

        access.choose(&vault).unwrap();

        assert_eq!(
            fs::read_to_string(vault.join("Welcome.md")).unwrap(),
            "# Welcome\n"
        );
        assert_eq!(
            fs::read_to_string(vault.join(".obsidian/app.json")).unwrap(),
            "{}"
        );
        assert!(!vault.join(BOOKMARK_FILE).exists());
    }

    #[test]
    fn a_stale_bookmark_is_saved_again() {
        let dir = TestDir::new();
        let folder = dir.path().join("Notes");
        let bookmarks = FakeBookmarks::default();
        let access = FolderAccess::new(dir.path().join("support"), bookmarks);
        access.choose(&folder).unwrap();
        access.bookmarks.stale.set(true);

        assert_eq!(access.restore().unwrap(), Some(folder));
        assert_eq!(access.bookmarks.created.get(), 2);
    }

    #[test]
    fn a_folder_that_is_gone_is_asked_for_again() {
        let dir = TestDir::new();
        let folder = dir.path().join("Notes");
        let access = FolderAccess::new(dir.path().join("support"), FakeBookmarks::default());
        access.choose(&folder).unwrap();
        fs::remove_dir(&folder).unwrap();
        assert_eq!(access.restore().unwrap(), None);

        access.choose(&folder).unwrap();
        access.bookmarks.broken.set(true);
        assert_eq!(access.restore().unwrap(), None);
    }

    #[test]
    fn a_real_bookmark_reopens_a_temp_folder() {
        let dir = TestDir::new();
        let folder = dir.path().join("Notes");
        let support = dir.path().join("support");
        FolderAccess::new(support.clone(), MacBookmarks)
            .choose(&folder)
            .unwrap();
        assert!(fs::metadata(support.join(BOOKMARK_FILE)).unwrap().len() > 0);

        let restored = FolderAccess::new(support, MacBookmarks).restore().unwrap();

        assert_eq!(
            restored.map(|path| path.canonicalize().unwrap()),
            Some(folder.canonicalize().unwrap())
        );
    }

    #[test]
    fn a_real_bookmark_follows_a_renamed_vault() {
        let dir = TestDir::new();
        let vault = obsidian_vault(dir.path());
        let support = dir.path().join("support");
        FolderAccess::new(support.clone(), MacBookmarks)
            .choose(&vault)
            .unwrap();
        let renamed = dir.path().join("Renamed vault");
        fs::rename(&vault, &renamed).unwrap();

        let restored = FolderAccess::new(support, MacBookmarks).restore().unwrap();

        assert_eq!(
            restored.map(|path| path.canonicalize().unwrap()),
            Some(renamed.canonicalize().unwrap())
        );
    }

    #[test]
    fn the_library_scans_a_chosen_vault_and_lists_only_recordings() {
        let dir = TestDir::new();
        let vault = obsidian_vault(dir.path());
        let support = dir.path().join("support");
        FolderAccess::new(support.clone(), MacBookmarks)
            .choose(&vault)
            .unwrap();
        let restored = FolderAccess::new(support, MacBookmarks)
            .restore()
            .unwrap()
            .unwrap();

        let library = Library::without_folder();
        assert!(library.list().unwrap().is_empty());
        library.set_notes_dir(restored);
        let listed: Vec<String> = library
            .list()
            .unwrap()
            .into_iter()
            .map(|recording| recording.folder)
            .collect();

        assert_eq!(listed, vec!["2026-09-26-1410"]);
    }
}
