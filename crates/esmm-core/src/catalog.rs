//! The official plugin index from `endless-sky/endless-sky-plugins`.

use serde::Deserialize;

pub const CATALOG_URL: &str = "https://raw.githubusercontent.com/endless-sky/endless-sky-plugins/master/generated/plugins.json";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub name: String,
    pub authors: String,
    pub homepage: String,
    pub license: String,
    /// Free-form (tags, commit SHAs, ...). Compare by equality only, never order.
    pub version: String,
    pub short_description: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Direct download for `version`. Not always GitHub.
    pub url: String,
    /// One index entry spells this `iconURL`.
    #[serde(default, alias = "iconURL")]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub autoupdate: Option<Autoupdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Autoupdate {
    /// `tag` or `commit`.
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to download the plugin catalog: {0}")]
    Http(#[from] ureq::Error),
    #[error("plugin catalog is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn parse(json: &str) -> Result<Vec<CatalogEntry>, CatalogError> {
    Ok(serde_json::from_str(json)?)
}

pub fn fetch() -> Result<Vec<CatalogEntry>, CatalogError> {
    let body = ureq::get(CATALOG_URL).call()?.body_mut().read_to_string()?;
    parse(&body)
}
