//! Downloads one model's files into its folder, resuming partial files and
//! checking each file's sha256. Knows nothing about the interface.

use super::catalog::{Model, ModelFile};
use super::store::Store;
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, RANGE};
use reqwest::redirect::Policy;
use reqwest::{StatusCode, Url};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Debug)]
pub enum DownloadError {
    Network(String),
    Server { file: String, status: u16 },
    Incomplete { file: String },
    Checksum { file: String },
    Disk(io::Error),
    Cancelled,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::Network(_) => {
                write!(
                    f,
                    "The download was interrupted. Check the connection and resume."
                )
            }
            DownloadError::Server { file, status } => {
                write!(f, "The server refused {file} (HTTP {status}).")
            }
            DownloadError::Incomplete { file } => {
                write!(
                    f,
                    "The download of {file} stopped early. Resume to finish it."
                )
            }
            DownloadError::Checksum { file } => write!(
                f,
                "{file} did not match its checksum and was removed. Download it again."
            ),
            DownloadError::Disk(err) if err.raw_os_error() == Some(libc::ENOSPC) => {
                write!(f, "The disk is full.")
            }
            DownloadError::Disk(err) => write!(f, "Could not save the model: {err}"),
            DownloadError::Cancelled => write!(f, "The download was cancelled."),
        }
    }
}

impl std::error::Error for DownloadError {}

/// An HTTP client for model downloads. Redirects are followed only to the
/// same host or to Hugging Face's own download hosts.
pub fn client() -> Client {
    Client::builder()
        // Applies to connecting and to each read, so a stalled connection
        // fails instead of hanging, while a long download is fine.
        .timeout(STALL_TIMEOUT)
        .redirect(Policy::custom(|attempt| {
            let allowed = attempt
                .previous()
                .first()
                .is_some_and(|original| redirect_allowed(original, attempt.url()));
            if attempt.previous().len() > MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if allowed {
                attempt.follow()
            } else {
                let message = format!("redirect to {} is not allowed", attempt.url());
                attempt.error(message)
            }
        }))
        .build()
        .expect("the download client builds")
}

const STALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 5;
const CHUNK: usize = 256 * 1024;

/// Whether a redirect from `original` to `next` may be followed.
pub fn redirect_allowed(original: &Url, next: &Url) -> bool {
    let same_origin = original.scheme() == next.scheme()
        && original.host_str() == next.host_str()
        && original.port_or_known_default() == next.port_or_known_default();
    let hugging_face = next.scheme() == "https"
        && next.port().is_none()
        && next
            .host_str()
            .is_some_and(|host| host == "huggingface.co" || host.ends_with(".hf.co"));
    same_origin || hugging_face
}

/// Downloads every file of `model` into its folder, then marks it usable.
/// Stops at the first failure; partial files stay for the next attempt.
pub fn download_model(
    client: &Client,
    store: &Store,
    model: &Model,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), DownloadError> {
    let dir = store.model_dir(model);
    fs::create_dir_all(&dir).map_err(DownloadError::Disk)?;
    let total = model.size();
    let mut done_before = 0;
    for file in &model.files {
        let mut report = |file_bytes: u64| {
            progress(Progress {
                downloaded: done_before + file_bytes,
                total,
            })
        };
        download_file(client, &dir, file, cancel, &mut report)?;
        done_before += file.size;
    }
    progress(Progress {
        downloaded: total,
        total,
    });
    store.mark_usable(model).map_err(DownloadError::Disk)
}

/// Downloads one file to `<name>.part`, resuming from its current length,
/// then renames it to `<name>` only if its sha256 matches.
fn download_file(
    client: &Client,
    dir: &Path,
    file: &ModelFile,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), DownloadError> {
    let dest = dir.join(&file.name);
    if fs::metadata(&dest).is_ok_and(|meta| meta.len() == file.size) {
        on_bytes(file.size);
        return Ok(());
    }
    let part = Store::part_path(dir, &file.name);
    let mut have = fs::metadata(&part).map(|meta| meta.len()).unwrap_or(0);
    if have > file.size {
        fs::remove_file(&part).map_err(DownloadError::Disk)?;
        have = 0;
    }
    on_bytes(have);

    if have < file.size {
        let mut request = client.get(&file.url);
        if have > 0 {
            request = request.header(RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().map_err(network)?;
        let status = response.status();
        let resumed =
            status == StatusCode::PARTIAL_CONTENT && content_range_start(&response) == Some(have);
        if !resumed && status != StatusCode::OK {
            return Err(DownloadError::Server {
                file: file.name.clone(),
                status: status.as_u16(),
            });
        }
        // A plain 200 means the server sent the whole file again.
        if !resumed {
            have = 0;
            on_bytes(0);
        }
        let mut out = OpenOptions::new()
            .create(true)
            .append(resumed)
            .write(true)
            .truncate(!resumed)
            .open(&part)
            .map_err(DownloadError::Disk)?;
        let mut buf = vec![0; CHUNK];
        while have < file.size {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let read = response.read(&mut buf).map_err(network)?;
            if read == 0 {
                break;
            }
            let read = read.min((file.size - have) as usize);
            out.write_all(&buf[..read]).map_err(DownloadError::Disk)?;
            have += read as u64;
            on_bytes(have);
        }
        out.sync_all().map_err(DownloadError::Disk)?;
        if have < file.size {
            return Err(DownloadError::Incomplete {
                file: file.name.clone(),
            });
        }
    }

    if sha256_file(&part).map_err(DownloadError::Disk)? != file.sha256 {
        fs::remove_file(&part).map_err(DownloadError::Disk)?;
        return Err(DownloadError::Checksum {
            file: file.name.clone(),
        });
    }
    fs::rename(&part, &dest).map_err(DownloadError::Disk)
}

fn content_range_start(response: &Response) -> Option<u64> {
    // "bytes 100-199/200"
    response
        .headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; CHUNK];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn network(err: impl fmt::Display) -> DownloadError {
    DownloadError::Network(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::catalog::test_catalog::model;
    use crate::models::catalog::Role;
    use crate::models::test_server::TestServer;
    use crate::notes::test_dir::TestDir;
    use std::fs;
    use std::sync::atomic::Ordering;

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn run(store: &Store, model: &Model) -> (Result<(), DownloadError>, Vec<Progress>) {
        let mut seen = Vec::new();
        let result = download_model(
            &client(),
            store,
            model,
            &AtomicBool::new(false),
            &mut |progress| seen.push(progress),
        );
        (result, seen)
    }

    #[test]
    fn a_download_with_matching_checksums_becomes_usable() {
        let server = TestServer::start();
        let bytes = data(300_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let mut model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);
        let second = data(1_000);
        server.put("b.gguf", &second);
        let mut file = model.files[0].clone();
        file.name = "b.gguf".into();
        file.url = server.url("b.gguf");
        file.size = second.len() as u64;
        file.sha256 = model_sha(&second);
        model.files.push(file);

        let (result, progress) = run(&store, &model);

        result.unwrap();
        assert!(store.is_usable(&model));
        let model_dir = store.model_dir(&model);
        assert_eq!(fs::read(model_dir.join("a.gguf")).unwrap(), bytes);
        assert_eq!(fs::read(model_dir.join("b.gguf")).unwrap(), second);
        assert!(!Store::part_path(&model_dir, "a.gguf").exists());
        let last = progress.last().unwrap();
        assert_eq!(last.downloaded, 301_000);
        assert_eq!(last.total, 301_000);
    }

    #[test]
    fn a_bad_checksum_stays_unusable_and_removes_the_file() {
        let server = TestServer::start();
        let bytes = data(50_000);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);
        let mut tampered = bytes.clone();
        tampered[1234] ^= 0xff;
        server.put("a.gguf", &tampered);

        let (result, _) = run(&store, &model);

        assert!(
            matches!(&result, Err(DownloadError::Checksum { file }) if file == "a.gguf"),
            "{result:?}"
        );
        assert!(!store.is_usable(&model));
        let model_dir = store.model_dir(&model);
        assert!(!model_dir.join("a.gguf").exists());
        assert!(!Store::part_path(&model_dir, "a.gguf").exists());
        assert_eq!(store.downloaded_bytes(&model), 0);
    }

    #[test]
    fn one_bad_file_keeps_the_whole_model_unusable() {
        let server = TestServer::start();
        let good = data(2_000);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let mut model = model("a", Role::Transcribe, &server.url("good.gguf"), &good);
        model.files[0].name = "good.gguf".into();
        server.put("good.gguf", &good);
        let mut bad = model.files[0].clone();
        bad.name = "bad.gguf".into();
        bad.url = server.url("bad.gguf");
        model.files.push(bad);
        server.put(
            "bad.gguf",
            &data(1_999).iter().chain(&[0]).copied().collect::<Vec<_>>(),
        );

        let (result, _) = run(&store, &model);

        assert!(matches!(result, Err(DownloadError::Checksum { .. })));
        assert!(!store.is_usable(&model));
    }

    #[test]
    fn an_interrupted_download_resumes_where_it_stopped() {
        let server = TestServer::start();
        let bytes = data(200_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);

        server.cut_next_response_after(70_000);
        let (first, _) = run(&store, &model);
        assert!(first.is_err(), "the cut connection is an error");
        assert!(!store.is_usable(&model));
        let part = Store::part_path(&store.model_dir(&model), "a.gguf");
        assert_eq!(fs::metadata(&part).unwrap().len(), 70_000);
        assert_eq!(store.downloaded_bytes(&model), 70_000);

        let (second, progress) = run(&store, &model);

        second.unwrap();
        let requests = server.requests();
        assert_eq!(requests[0].range, None);
        assert_eq!(requests[1].range.as_deref(), Some("bytes=70000-"));
        assert_eq!(progress.first().unwrap().downloaded, 70_000);
        assert_eq!(
            fs::read(store.model_dir(&model).join("a.gguf")).unwrap(),
            bytes
        );
        assert!(store.is_usable(&model));
    }

    #[test]
    fn a_server_that_ignores_the_range_restarts_the_file_from_zero() {
        let server = TestServer::start();
        let bytes = data(100_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);
        server.cut_next_response_after(40_000);
        let _ = run(&store, &model);

        server.ignore_range();
        let (result, _) = run(&store, &model);

        result.unwrap();
        assert_eq!(
            fs::read(store.model_dir(&model).join("a.gguf")).unwrap(),
            bytes
        );
    }

    #[test]
    fn a_finished_file_is_not_downloaded_again() {
        let server = TestServer::start();
        let bytes = data(10_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);
        run(&store, &model).0.unwrap();

        run(&store, &model).0.unwrap();

        assert_eq!(server.requests().len(), 1);
    }

    #[test]
    fn a_missing_file_on_the_server_is_reported() {
        let server = TestServer::start();
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("gone.gguf"), b"x");

        let (result, _) = run(&store, &model);

        assert!(matches!(
            result,
            Err(DownloadError::Server { status: 404, .. })
        ));
        assert!(!store.is_usable(&model));
    }

    #[test]
    fn cancelling_keeps_the_partial_file_for_later() {
        let server = TestServer::start();
        let bytes = data(500_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model("a", Role::Transcribe, &server.url("a.gguf"), &bytes);
        let cancel = AtomicBool::new(false);

        let result = download_model(&client(), &store, &model, &cancel, &mut |progress| {
            if progress.downloaded > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        });

        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert!(!store.is_usable(&model));
        assert!(store.downloaded_bytes(&model) > 0);
    }

    #[test]
    fn redirects_on_the_same_host_are_followed() {
        let server = TestServer::start();
        let bytes = data(1_000);
        server.put("a.gguf", &bytes);
        let dir = TestDir::new();
        let store = Store::new(dir.path());
        let model = model(
            "a",
            Role::Transcribe,
            &server.url("redirect/a.gguf"),
            &bytes,
        );

        run(&store, &model).0.unwrap();

        assert!(store.is_usable(&model));
    }

    #[test]
    fn redirects_go_only_to_the_same_host_or_hugging_face() {
        let at = |scheme: &str, host: &str| Url::parse(&format!("{scheme}://{host}/x")).unwrap();
        let hf = at("https", "huggingface.co");
        // Hugging Face serves files from its own download hosts.
        for host in [
            "huggingface.co",
            "us.aws.cdn.hf.co",
            "cas-bridge.xethub.hf.co",
        ] {
            assert!(redirect_allowed(&hf, &at("https", host)), "{host}");
        }
        for host in ["example.com", "evilhf.co", "hf.co.example.com"] {
            assert!(!redirect_allowed(&hf, &at("https", host)), "{host}");
        }
        assert!(!redirect_allowed(&hf, &at("http", "us.aws.cdn.hf.co")));
        assert!(!redirect_allowed(
            &hf,
            &at("https", "us.aws.cdn.hf.co:8443")
        ));

        let local = at("http", "127.0.0.1:4000");
        assert!(redirect_allowed(&local, &at("http", "127.0.0.1:4000")));
        assert!(!redirect_allowed(&local, &at("http", "127.0.0.1:4001")));
    }

    #[test]
    fn errors_read_as_plain_sentences() {
        let checksum = DownloadError::Checksum {
            file: "a.gguf".into(),
        };
        assert_eq!(
            checksum.to_string(),
            "a.gguf did not match its checksum and was removed. Download it again."
        );
        let full = DownloadError::Disk(io::Error::from_raw_os_error(libc::ENOSPC));
        assert_eq!(full.to_string(), "The disk is full.");
    }

    fn model_sha(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }
}
