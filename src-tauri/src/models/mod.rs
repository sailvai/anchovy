//! The shipped model list, the models downloaded to this Mac, and their
//! downloads. Running a model is not here; that is `engines/`.
//!
//! `Models` ties the parts together for the interface: it lists every model
//! with its memory label and state, runs one download per model on its own
//! thread, and refuses anything not in the shipped list.

pub mod catalog;
pub mod commands;
pub mod download;
pub mod fit;
pub mod mac;
pub mod store;
#[cfg(test)]
mod test_server;

use crate::engines::ModelFiles;
use catalog::{Catalog, Model, Role};
use download::{DownloadError, Progress};
use fit::Fit;
use serde::Serialize;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use store::{SelectError, Store};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelsView {
    pub memory_bytes: u64,
    pub models: Vec<ModelView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelView {
    pub id: String,
    pub role: Role,
    pub display_name: String,
    pub languages: Vec<String>,
    pub size_bytes: u64,
    pub min_ram_gb: u32,
    pub license: String,
    pub fit: Fit,
    pub selected: bool,
    pub state: ModelState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelState {
    NotDownloaded,
    /// Part of the model is on disk from an earlier attempt.
    Paused {
        downloaded: u64,
    },
    Downloading {
        downloaded: u64,
    },
    Failed {
        downloaded: u64,
        error: String,
    },
    Ready,
}

/// What the download thread reports while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Progress {
        id: String,
        downloaded: u64,
    },
    /// The download stopped: finished, failed, or cancelled.
    Finished {
        id: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ModelsError {
    NotInList,
    Busy,
    Disk(String),
}

impl fmt::Display for ModelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelsError::NotInList => write!(f, "That model is not in the list."),
            ModelsError::Busy => write!(f, "That model is downloading."),
            ModelsError::Disk(err) => write!(f, "Could not change the model files: {err}"),
        }
    }
}

impl std::error::Error for ModelsError {}

struct Job {
    cancel: Arc<AtomicBool>,
    downloaded: u64,
}

pub struct Models {
    catalog: Catalog,
    store: Store,
    memory_bytes: u64,
    jobs: Mutex<HashMap<String, Job>>,
    /// The last failure per model, until the next attempt or delete.
    failures: Mutex<HashMap<String, String>>,
}

impl Models {
    pub fn new(catalog: Catalog, store: Store, memory_bytes: u64) -> Self {
        Models {
            catalog,
            store,
            memory_bytes,
            jobs: Mutex::new(HashMap::new()),
            failures: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> ModelsView {
        let selected: Vec<String> = Role::ALL
            .iter()
            .map(|role| self.store.selected(&self.catalog, *role))
            .collect();
        let models = self
            .catalog
            .models
            .iter()
            .map(|model| ModelView {
                id: model.id.clone(),
                role: model.role,
                display_name: model.display_name.clone(),
                languages: model.languages.clone(),
                size_bytes: model.size(),
                min_ram_gb: model.min_ram_gb,
                license: model.license.clone(),
                fit: fit::fit(self.memory_bytes, model.min_ram_gb),
                selected: selected.contains(&model.id),
                state: self.state(model),
            })
            .collect();
        ModelsView {
            memory_bytes: self.memory_bytes,
            models,
        }
    }

    fn state(&self, model: &Model) -> ModelState {
        if let Some(job) = self.jobs.lock().unwrap().get(&model.id) {
            return ModelState::Downloading {
                downloaded: job.downloaded,
            };
        }
        if self.store.is_usable(model) {
            return ModelState::Ready;
        }
        let downloaded = self.store.downloaded_bytes(model);
        if let Some(error) = self.failures.lock().unwrap().get(&model.id) {
            return ModelState::Failed {
                downloaded,
                error: error.clone(),
            };
        }
        if downloaded > 0 {
            ModelState::Paused { downloaded }
        } else {
            ModelState::NotDownloaded
        }
    }

    pub fn select(&self, id: &str) -> Result<(), ModelsError> {
        self.store
            .select(&self.catalog, id)
            .map_err(|err| match err {
                SelectError::NotInList => ModelsError::NotInList,
                SelectError::Io(err) => ModelsError::Disk(err.to_string()),
            })
    }

    /// Starts downloading a model on its own thread, or resumes it. Does
    /// nothing if it is already downloading or ready.
    pub fn download(
        self: &Arc<Self>,
        id: &str,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<(), ModelsError> {
        let model = self.catalog.get(id).ok_or(ModelsError::NotInList)?.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.contains_key(id) || self.store.is_usable(&model) {
                return Ok(());
            }
            jobs.insert(
                model.id.clone(),
                Job {
                    cancel: cancel.clone(),
                    downloaded: self.store.downloaded_bytes(&model),
                },
            );
        }
        self.failures.lock().unwrap().remove(id);

        let models = Arc::clone(self);
        thread::spawn(move || {
            let client = download::client();
            let mut last_percent = None;
            let mut report = |progress: Progress| {
                if let Some(job) = models.jobs.lock().unwrap().get_mut(&model.id) {
                    job.downloaded = progress.downloaded;
                }
                // At most one event per percent, so the interface is not
                // flooded during a multi-gigabyte download.
                let percent = progress.downloaded * 100 / progress.total.max(1);
                if last_percent != Some(percent) {
                    last_percent = Some(percent);
                    on_event(Event::Progress {
                        id: model.id.clone(),
                        downloaded: progress.downloaded,
                    });
                }
            };
            let result =
                download::download_model(&client, &models.store, &model, &cancel, &mut report);
            match result {
                Ok(()) | Err(DownloadError::Cancelled) => {}
                Err(err) => {
                    let mut failures = models.failures.lock().unwrap();
                    failures.insert(model.id.clone(), err.to_string());
                }
            }
            models.jobs.lock().unwrap().remove(&model.id);
            on_event(Event::Finished { id: model.id });
        });
        Ok(())
    }

    /// Stops a download. The partial files stay so it can resume.
    pub fn cancel(&self, id: &str) {
        if let Some(job) = self.jobs.lock().unwrap().get(id) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The model selected for `role`, with its files on this Mac, and
    /// whether every file passed its checksum.
    pub fn selected_files(&self, role: Role) -> (ModelFiles, bool) {
        let id = self.store.selected(&self.catalog, role);
        let model = self.catalog.get(&id).expect("the selection is in the list");
        let dir = self.store.model_dir(model);
        let files = ModelFiles {
            id: model.id.clone(),
            engine: model.engine.clone(),
            display_name: model.display_name.clone(),
            files: model
                .files
                .iter()
                .map(|file| dir.join(&file.name))
                .collect(),
            size_bytes: model.size(),
        };
        (files, self.store.is_usable(model))
    }

    /// Deletes a model's files. Not allowed while it downloads.
    pub fn delete(&self, id: &str) -> Result<(), ModelsError> {
        let model = self.catalog.get(id).ok_or(ModelsError::NotInList)?;
        let jobs = self.jobs.lock().unwrap();
        if jobs.contains_key(id) {
            return Err(ModelsError::Busy);
        }
        self.failures.lock().unwrap().remove(id);
        self.store
            .delete(model)
            .map_err(|err| ModelsError::Disk(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::catalog::test_catalog::model;
    use super::test_server::TestServer;
    use super::*;
    use crate::notes::test_dir::TestDir;
    use std::sync::mpsc;
    use std::time::Duration;

    const GIB: u64 = fit::GIB;

    struct Setup {
        server: TestServer,
        _dir: TestDir,
        models: Arc<Models>,
    }

    /// Two transcription models and one summary model, served locally.
    /// "big" needs 16 GB; "bad" is served with a wrong checksum.
    fn setup(memory_gb: u64) -> Setup {
        let server = TestServer::start();
        let good = vec![7; 64_000];
        server.put("small.gguf", &good);
        server.put("bad.gguf", &[1; 1_000]);
        let mut small = model("small", Role::Transcribe, &server.url("small.gguf"), &good);
        small.default = true;
        small.min_ram_gb = 4;
        let mut big = model("big", Role::Transcribe, &server.url("small.gguf"), &good);
        big.min_ram_gb = 16;
        let mut bad = model("bad", Role::Summarize, &server.url("bad.gguf"), &[2; 1_000]);
        bad.default = true;
        let catalog = Catalog {
            version: 1,
            models: vec![small, big, bad],
        };
        catalog.check().unwrap();
        let dir = TestDir::new();
        let models = Arc::new(Models::new(
            catalog,
            Store::new(dir.path()),
            memory_gb * GIB,
        ));
        Setup {
            server,
            _dir: dir,
            models,
        }
    }

    #[test]
    fn the_pipeline_gets_the_selected_models_files_and_whether_they_are_here() {
        let s = setup(16);
        let (model, ready) = s.models.selected_files(Role::Transcribe);
        assert_eq!(model.id, "small");
        assert!(!ready);
        assert_eq!(model.files.len(), 1);
        assert!(model.files[0].ends_with("small.gguf"));
        assert_eq!(model.size_bytes, 64_000);

        download_and_wait(&s.models, "small");
        s.models.select("big").unwrap();
        let (model, ready) = s.models.selected_files(Role::Transcribe);
        assert_eq!(model.id, "big");
        assert!(!ready, "big is not downloaded");
        s.models.select("small").unwrap();
        assert!(s.models.selected_files(Role::Transcribe).1);
    }

    fn view<'a>(view: &'a ModelsView, id: &str) -> &'a ModelView {
        view.models.iter().find(|model| model.id == id).unwrap()
    }

    fn download_and_wait(models: &Arc<Models>, id: &str) -> Vec<Event> {
        let (tx, rx) = mpsc::channel();
        models
            .download(id, move |event| tx.send(event).unwrap())
            .unwrap();
        let mut events = Vec::new();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let done = matches!(event, Event::Finished { .. });
            events.push(event);
            if done {
                return events;
            }
        }
    }

    #[test]
    fn lists_every_model_with_its_memory_label() {
        let list = setup(8).models.list();
        assert_eq!(list.memory_bytes, 8 * GIB);
        assert_eq!(view(&list, "small").fit, Fit::Recommended);
        assert_eq!(view(&list, "big").fit, Fit::TooLarge);
        assert_eq!(view(&list, "bad").fit, Fit::Fits);

        let list = setup(32).models.list();
        assert_eq!(view(&list, "big").fit, Fit::Recommended);
    }

    #[test]
    fn defaults_are_selected_and_nothing_is_downloaded_at_first() {
        let setup = setup(16);
        let list = setup.models.list();
        assert!(view(&list, "small").selected);
        assert!(!view(&list, "big").selected);
        assert!(view(&list, "bad").selected);
        for model in &list.models {
            assert_eq!(model.state, ModelState::NotDownloaded);
        }
    }

    #[test]
    fn a_finished_download_is_ready() {
        let setup = setup(16);
        let events = download_and_wait(&setup.models, "small");
        assert_eq!(events.last(), Some(&Event::Finished { id: "small".into() }));
        assert!(events.contains(&Event::Progress {
            id: "small".into(),
            downloaded: 64_000
        }));
        assert_eq!(view(&setup.models.list(), "small").state, ModelState::Ready);
    }

    #[test]
    fn a_bad_checksum_shows_as_failed_and_not_ready() {
        let setup = setup(16);
        download_and_wait(&setup.models, "bad");
        assert_eq!(
            view(&setup.models.list(), "bad").state,
            ModelState::Failed {
                downloaded: 0,
                error: "bad.gguf did not match its checksum and was removed. Download it again."
                    .into()
            }
        );
    }

    #[test]
    fn an_interrupted_download_shows_as_failed_then_resumes_to_ready() {
        let setup = setup(16);
        setup.server.cut_next_response_after(20_000);
        download_and_wait(&setup.models, "small");
        let state = view(&setup.models.list(), "small").state.clone();
        assert!(
            matches!(
                state,
                ModelState::Failed {
                    downloaded: 20_000,
                    ..
                }
            ),
            "{state:?}"
        );

        download_and_wait(&setup.models, "small");

        assert_eq!(view(&setup.models.list(), "small").state, ModelState::Ready);
        let ranges: Vec<_> = setup
            .server
            .requests()
            .into_iter()
            .map(|request| request.range)
            .collect();
        assert_eq!(ranges, vec![None, Some("bytes=20000-".into())]);
    }

    #[test]
    fn a_partial_download_from_an_earlier_run_shows_as_paused() {
        let setup = setup(16);
        setup.server.cut_next_response_after(10_000);
        download_and_wait(&setup.models, "small");
        let catalog = setup.models.catalog.clone();
        let root = setup._dir.path();
        let relaunched = Models::new(catalog, Store::new(root), 16 * GIB);
        assert_eq!(
            view(&relaunched.list(), "small").state,
            ModelState::Paused { downloaded: 10_000 }
        );
    }

    #[test]
    fn deleting_a_model_makes_it_not_downloaded() {
        let setup = setup(16);
        download_and_wait(&setup.models, "small");
        setup.models.delete("small").unwrap();
        assert_eq!(
            view(&setup.models.list(), "small").state,
            ModelState::NotDownloaded
        );
    }

    #[test]
    fn only_models_in_the_list_can_be_selected_downloaded_or_deleted() {
        let setup = setup(16);
        let models = &setup.models;
        let other = "not-in-the-list";
        assert_eq!(models.select(other), Err(ModelsError::NotInList));
        assert_eq!(models.download(other, |_| {}), Err(ModelsError::NotInList));
        assert_eq!(models.delete(other), Err(ModelsError::NotInList));
    }

    #[test]
    fn a_too_large_model_can_still_be_selected() {
        let setup = setup(8);
        setup.models.select("big").unwrap();
        let list = setup.models.list();
        assert!(view(&list, "big").selected);
        assert!(!view(&list, "small").selected);
    }

    #[test]
    fn a_model_cannot_be_deleted_while_it_downloads() {
        let setup = setup(16);
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        // Hold the download at its first progress report.
        setup
            .models
            .download("small", move |event| {
                let first = matches!(event, Event::Progress { .. });
                tx.send(event).unwrap();
                if first {
                    let _ = release_rx.lock().unwrap().recv();
                }
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap();

        assert!(matches!(
            view(&setup.models.list(), "small").state,
            ModelState::Downloading { .. }
        ));
        assert_eq!(setup.models.delete("small"), Err(ModelsError::Busy));

        setup.models.cancel("small");
        drop(release_tx);
        while !matches!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Finished { .. }
        ) {}
        assert!(setup.models.delete("small").is_ok());
    }

    #[test]
    fn states_serialize_for_the_interface() {
        let json = serde_json::to_value(ModelState::Failed {
            downloaded: 5,
            error: "x".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "failed", "downloaded": 5, "error": "x"})
        );
    }
}
