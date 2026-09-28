//! The list of models that ships with the app, `resources/models.json`.
//!
//! Users pick only from this list. Each entry pins a revision and every
//! file's size and sha256, so a download is either exactly what we shipped
//! or it is rejected.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

const SHIPPED: &str = include_str!("../../resources/models.json");

/// Download host for shipped models. Must match `privacy/allowed-urls.txt`.
pub const DOWNLOAD_PREFIX: &str = "https://huggingface.co/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Transcribe,
    Summarize,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Transcribe, Role::Summarize];
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub role: Role,
    pub engine: String,
    pub display_name: String,
    pub languages: Vec<String>,
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub min_ram_gb: u32,
    pub license: String,
    pub default: bool,
}

impl Model {
    /// Total download size in bytes.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub models: Vec<Model>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CatalogError(pub String);

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid model list: {}", self.0)
    }
}

impl std::error::Error for CatalogError {}

impl Catalog {
    /// The list compiled into the app. Its tests guarantee it parses.
    pub fn shipped() -> Catalog {
        Catalog::parse(SHIPPED).expect("resources/models.json is valid")
    }

    /// Parses and checks a list with the rules for shipped lists.
    pub fn parse(json: &str) -> Result<Catalog, CatalogError> {
        let catalog: Catalog =
            serde_json::from_str(json).map_err(|err| CatalogError(err.to_string()))?;
        catalog.check()?;
        for model in &catalog.models {
            check_shipped_source(model)?;
        }
        Ok(catalog)
    }

    /// Rules for any list, including the ones tests build with local
    /// addresses: safe folder names, well-formed checksums, one default per
    /// role.
    pub fn check(&self) -> Result<(), CatalogError> {
        let fail = |message: String| Err(CatalogError(message));
        let mut ids = HashSet::new();
        for model in &self.models {
            let id = &model.id;
            if !is_path_component(id) {
                return fail(format!("id {id:?} is not a safe folder name"));
            }
            if !ids.insert(id) {
                return fail(format!("{id} is listed twice"));
            }
            if !is_path_component(&model.revision) {
                return fail(format!("{id}: revision is not a safe folder name"));
            }
            if model.display_name.trim().is_empty() || model.languages.is_empty() {
                return fail(format!("{id}: needs a display name and languages"));
            }
            if model.license.trim().is_empty() {
                return fail(format!("{id}: needs a license"));
            }
            if model.min_ram_gb == 0 {
                return fail(format!("{id}: min_ram_gb must be at least 1"));
            }
            if model.files.is_empty() {
                return fail(format!("{id}: has no files"));
            }
            let mut names = HashSet::new();
            for file in &model.files {
                let name = &file.name;
                if !is_path_component(name) || name.ends_with(".part") || name.starts_with('.') {
                    return fail(format!("{id}: file name {name:?} is not allowed"));
                }
                if !names.insert(name) {
                    return fail(format!("{id}: {name} is listed twice"));
                }
                if !is_lower_hex(&file.sha256, 64) {
                    return fail(format!("{id}: {name} needs a lowercase sha256"));
                }
                if file.size == 0 {
                    return fail(format!("{id}: {name} needs its size"));
                }
            }
        }
        for role in Role::ALL {
            let defaults = self
                .models
                .iter()
                .filter(|model| model.role == role && model.default)
                .count();
            if defaults != 1 {
                return fail(format!(
                    "{role:?} needs exactly one default, has {defaults}"
                ));
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|model| model.id == id)
    }

    pub fn default_for(&self, role: Role) -> &Model {
        self.models
            .iter()
            .find(|model| model.role == role && model.default)
            .expect("check() guarantees one default per role")
    }
}

/// Shipped entries download from Hugging Face at a pinned commit, never a
/// branch that can move.
fn check_shipped_source(model: &Model) -> Result<(), CatalogError> {
    let id = &model.id;
    if model.engine != "llama_cpp" {
        return Err(CatalogError(format!(
            "{id}: unknown engine {}",
            model.engine
        )));
    }
    if !is_lower_hex(&model.revision, 40) {
        return Err(CatalogError(format!(
            "{id}: revision must be a full commit hash"
        )));
    }
    for file in &model.files {
        let expected_tail = format!("/resolve/{}/{}", model.revision, file.name);
        let url = &file.url;
        let repo = url
            .strip_prefix(DOWNLOAD_PREFIX)
            .and_then(|rest| rest.strip_suffix(&expected_tail));
        let valid_repo = repo.is_some_and(|repo| {
            let parts: Vec<&str> = repo.split('/').collect();
            parts.len() == 2 && parts.iter().all(|part| is_path_component(part))
        });
        if !valid_repo {
            return Err(CatalogError(format!(
                "{id}: {url} must be {DOWNLOAD_PREFIX}<owner>/<repo>{expected_tail}"
            )));
        }
    }
    Ok(())
}

fn is_path_component(text: &str) -> bool {
    !text.is_empty()
        && text != "."
        && text != ".."
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

#[cfg(test)]
pub(crate) mod test_catalog {
    use super::*;

    /// A one-file model; `url` is usually a local test server.
    pub fn model(id: &str, role: Role, url: &str, data: &[u8]) -> Model {
        use sha2::{Digest, Sha256};
        Model {
            id: id.into(),
            role,
            engine: "llama_cpp".into(),
            display_name: format!("Model {id}"),
            languages: vec!["English".into()],
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
            files: vec![ModelFile {
                name: format!("{id}.gguf"),
                url: url.into(),
                sha256: format!("{:x}", Sha256::digest(data)),
                size: data.len() as u64,
            }],
            min_ram_gb: 8,
            license: "Apache-2.0".into(),
            default: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped_json() -> serde_json::Value {
        serde_json::from_str(SHIPPED).unwrap()
    }

    fn parse_value(value: &serde_json::Value) -> Result<Catalog, CatalogError> {
        Catalog::parse(&value.to_string())
    }

    #[test]
    fn the_shipped_list_parses_and_passes_every_rule() {
        let catalog = Catalog::shipped();
        assert!(catalog.models.len() >= 3, "the list offers alternatives");
    }

    #[test]
    fn the_shipped_defaults_are_the_models_in_the_spec() {
        let catalog = Catalog::shipped();
        assert_eq!(
            catalog.default_for(Role::Transcribe).display_name,
            "Qwen3-ASR 1.7B"
        );
        assert_eq!(
            catalog.default_for(Role::Summarize).display_name,
            "Qwen3-4B Instruct 2507"
        );
    }

    #[test]
    fn every_shipped_model_has_an_alternative_for_its_role() {
        let catalog = Catalog::shipped();
        for role in Role::ALL {
            let count = catalog.models.iter().filter(|m| m.role == role).count();
            assert!(count >= 2, "{role:?} has {count} models");
        }
    }

    #[test]
    fn a_moving_branch_is_rejected() {
        let mut json = shipped_json();
        let model = &mut json["models"][0];
        let revision = model["revision"].as_str().unwrap().to_string();
        let url = model["files"][0]["url"]
            .as_str()
            .unwrap()
            .replace(&revision, "main");
        model["files"][0]["url"] = url.into();
        model["revision"] = "main".into();
        let err = parse_value(&json).unwrap_err();
        assert!(err.0.contains("full commit hash"), "{err}");
    }

    #[test]
    fn a_url_that_does_not_match_the_pinned_revision_is_rejected() {
        let mut json = shipped_json();
        let file = &mut json["models"][0]["files"][0];
        let url = file["url"]
            .as_str()
            .unwrap()
            .replace("/resolve/", "/resolve/x");
        file["url"] = url.into();
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn a_url_on_another_host_is_rejected() {
        let mut json = shipped_json();
        let file = &mut json["models"][0]["files"][0];
        let url = file["url"]
            .as_str()
            .unwrap()
            .replace("https://huggingface.co/", "http://127.0.0.1:8000/");
        file["url"] = url.into();
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn a_path_outside_the_model_folder_is_rejected() {
        for field in ["id", "revision"] {
            let mut json = shipped_json();
            json["models"][0][field] = "..".into();
            assert!(parse_value(&json).is_err(), "{field}");
        }
        let mut json = shipped_json();
        json["models"][0]["files"][0]["name"] = "../evil".into();
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn a_missing_or_malformed_checksum_is_rejected() {
        let mut json = shipped_json();
        json["models"][0]["files"][0]["sha256"] = "ABC".into();
        assert!(parse_value(&json).is_err());
        let mut json = shipped_json();
        json["models"][0]["files"][0]
            .as_object_mut()
            .unwrap()
            .remove("sha256");
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn a_missing_license_is_rejected() {
        let mut json = shipped_json();
        json["models"][0]["license"] = "".into();
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn each_role_needs_exactly_one_default() {
        let mut json = shipped_json();
        for model in json["models"].as_array_mut().unwrap() {
            model["default"] = true.into();
        }
        assert!(parse_value(&json).is_err());
        for model in json["models"].as_array_mut().unwrap() {
            model["default"] = false.into();
        }
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected_so_typos_do_not_pass() {
        let mut json = shipped_json();
        json["models"][0]["min_ram"] = 4.into();
        assert!(parse_value(&json).is_err());
    }

    #[test]
    fn model_size_is_the_sum_of_its_files() {
        let catalog = Catalog::shipped();
        let model = catalog.default_for(Role::Transcribe);
        assert_eq!(
            model.size(),
            model.files.iter().map(|file| file.size).sum::<u64>()
        );
        assert!(model.files.len() >= 2, "main model plus audio projector");
    }
}
