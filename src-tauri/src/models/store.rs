//! Models on disk, under `<root>/<id>/<revision>/`, and which model is
//! selected for each role.
//!
//! A file being downloaded is `<name>.part`. It is renamed to `<name>` only
//! after its sha256 matches, and the model is marked usable with a
//! `.verified` file only after every file has passed.

use super::catalog::{Catalog, Model, Role};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const VERIFIED: &str = ".verified";
const SELECTION: &str = "selection.json";

pub struct Store {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Selection {
    pub transcribe: Option<String>,
    pub summarize: Option<String>,
}

impl Selection {
    fn get(&self, role: Role) -> Option<&String> {
        match role {
            Role::Transcribe => self.transcribe.as_ref(),
            Role::Summarize => self.summarize.as_ref(),
        }
    }

    fn set(&mut self, role: Role, id: String) {
        match role {
            Role::Transcribe => self.transcribe = Some(id),
            Role::Summarize => self.summarize = Some(id),
        }
    }
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn model_dir(&self, model: &Model) -> PathBuf {
        self.root.join(&model.id).join(&model.revision)
    }

    pub fn part_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.part"))
    }

    /// Usable means every file passed its checksum: the marker is there and
    /// every file is still present at its listed size.
    pub fn is_usable(&self, model: &Model) -> bool {
        let dir = self.model_dir(model);
        dir.join(VERIFIED).is_file()
            && model
                .files
                .iter()
                .all(|file| file_len(&dir.join(&file.name)) == Some(file.size))
    }

    /// Bytes already on disk for this model, finished or partial.
    pub fn downloaded_bytes(&self, model: &Model) -> u64 {
        let dir = self.model_dir(model);
        model
            .files
            .iter()
            .map(|file| {
                file_len(&dir.join(&file.name))
                    .or_else(|| file_len(&Self::part_path(&dir, &file.name)))
                    .unwrap_or(0)
                    .min(file.size)
            })
            .sum()
    }

    pub fn mark_usable(&self, model: &Model) -> io::Result<()> {
        fs::write(self.model_dir(model).join(VERIFIED), &model.revision)
    }

    /// Removes the model, every revision of it, and any partial download.
    pub fn delete(&self, model: &Model) -> io::Result<()> {
        match fs::remove_dir_all(self.root.join(&model.id)) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// The selected model id for each role. Falls back to the list's default
    /// when nothing was chosen or the choice is no longer in the list.
    pub fn selected(&self, catalog: &Catalog, role: Role) -> String {
        let saved = self.read_selection();
        saved
            .get(role)
            .and_then(|id| catalog.get(id))
            .filter(|model| model.role == role)
            .unwrap_or_else(|| catalog.default_for(role))
            .id
            .clone()
    }

    /// Selects a model from the list for its role.
    pub fn select(&self, catalog: &Catalog, id: &str) -> Result<(), SelectError> {
        let model = catalog.get(id).ok_or(SelectError::NotInList)?;
        let mut selection = self.read_selection();
        selection.set(model.role, model.id.clone());
        fs::create_dir_all(&self.root).map_err(SelectError::Io)?;
        let json = serde_json::to_vec_pretty(&selection).expect("selection serializes");
        let tmp = self.root.join(format!("{SELECTION}.tmp"));
        fs::write(&tmp, json)
            .and_then(|()| fs::rename(&tmp, self.root.join(SELECTION)))
            .map_err(SelectError::Io)
    }

    fn read_selection(&self) -> Selection {
        fs::read(self.root.join(SELECTION))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub enum SelectError {
    NotInList,
    Io(io::Error),
}

fn file_len(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::catalog::test_catalog::model;
    use crate::notes::test_dir::TestDir;

    fn catalog() -> Catalog {
        let mut a = model("asr-a", Role::Transcribe, "http://127.0.0.1:8000/a", b"a");
        a.default = true;
        let b = model("asr-b", Role::Transcribe, "http://127.0.0.1:8000/b", b"b");
        let mut c = model("sum-c", Role::Summarize, "http://127.0.0.1:8000/c", b"c");
        c.default = true;
        let catalog = Catalog {
            version: 1,
            models: vec![a, b, c],
        };
        catalog.check().unwrap();
        catalog
    }

    #[test]
    fn models_are_stored_by_id_and_revision() {
        let store = Store::new("/models");
        let catalog = catalog();
        let model = catalog.get("asr-a").unwrap();
        assert_eq!(
            store.model_dir(model),
            PathBuf::from("/models/asr-a/0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn files_without_the_verified_marker_are_not_usable() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        let model = catalog.get("asr-a").unwrap();
        fs::create_dir_all(store.model_dir(model)).unwrap();
        fs::write(store.model_dir(model).join("asr-a.gguf"), b"a").unwrap();
        assert!(!store.is_usable(model));

        store.mark_usable(model).unwrap();
        assert!(store.is_usable(model));
    }

    #[test]
    fn a_missing_or_resized_file_makes_the_model_unusable() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        let model = catalog.get("asr-a").unwrap();
        let file = store.model_dir(model).join("asr-a.gguf");
        fs::create_dir_all(store.model_dir(model)).unwrap();
        fs::write(&file, b"a").unwrap();
        store.mark_usable(model).unwrap();

        fs::write(&file, b"ab").unwrap();
        assert!(!store.is_usable(model));
        fs::remove_file(&file).unwrap();
        assert!(!store.is_usable(model));
    }

    #[test]
    fn delete_removes_every_revision_and_partial_file() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        let model = catalog.get("asr-a").unwrap();
        fs::create_dir_all(store.model_dir(model)).unwrap();
        fs::write(store.model_dir(model).join("asr-a.gguf.part"), b"").unwrap();
        fs::create_dir_all(dir.path().join("asr-a/old-revision")).unwrap();
        let other = catalog.get("asr-b").unwrap();
        fs::create_dir_all(store.model_dir(other)).unwrap();

        store.delete(model).unwrap();

        assert!(!dir.path().join("asr-a").exists());
        assert!(store.model_dir(other).exists());
        store.delete(model).unwrap();
    }

    #[test]
    fn downloaded_bytes_counts_finished_and_partial_files() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let mut catalog = catalog();
        let model = &mut catalog.models[0];
        model.files[0].size = 10;
        let mut second = model.files[0].clone();
        second.name = "second.gguf".into();
        model.files.push(second);
        let model = &catalog.models[0];
        let model_dir = store.model_dir(model);
        fs::create_dir_all(&model_dir).unwrap();
        assert_eq!(store.downloaded_bytes(model), 0);

        fs::write(model_dir.join("asr-a.gguf"), [0; 10]).unwrap();
        fs::write(Store::part_path(&model_dir, "second.gguf"), [0; 4]).unwrap();
        assert_eq!(store.downloaded_bytes(model), 14);
    }

    #[test]
    fn selection_defaults_to_the_list_default() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        assert_eq!(store.selected(&catalog, Role::Transcribe), "asr-a");
        assert_eq!(store.selected(&catalog, Role::Summarize), "sum-c");
    }

    #[test]
    fn selecting_a_model_replaces_the_choice_for_its_role_only() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        store.select(&catalog, "asr-b").unwrap();
        assert_eq!(store.selected(&catalog, Role::Transcribe), "asr-b");
        assert_eq!(store.selected(&catalog, Role::Summarize), "sum-c");
        assert_eq!(
            Store::new(dir.path()).selected(&catalog, Role::Transcribe),
            "asr-b"
        );
    }

    #[test]
    fn only_models_in_the_list_can_be_selected() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let catalog = catalog();
        assert!(matches!(
            store.select(&catalog, "not-in-the-list"),
            Err(SelectError::NotInList)
        ));
        assert_eq!(store.selected(&catalog, Role::Transcribe), "asr-a");
    }

    #[test]
    fn a_saved_choice_that_left_the_list_falls_back_to_the_default() {
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        fs::write(
            dir.path().join(SELECTION),
            r#"{"transcribe":"removed","summarize":"asr-b"}"#,
        )
        .unwrap();
        let catalog = catalog();
        assert_eq!(store.selected(&catalog, Role::Transcribe), "asr-a");
        // A transcription model saved as the summary choice is ignored.
        assert_eq!(store.selected(&catalog, Role::Summarize), "sum-c");
    }
}
