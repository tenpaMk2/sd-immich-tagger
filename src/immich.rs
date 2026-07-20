use anyhow::{bail, Context, Result};
use chrono::{DateTime, FixedOffset};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::tags::truncate_tag_name;

pub(crate) const PAGE_SIZE: u32 = 1000;
pub const DEFAULT_CUTOFF_DATE: &str = "2026-06-20T00:00:00+09:00";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub id: String,
    #[serde(default, rename = "originalFileName")]
    pub original_file_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "exifInfo")]
    pub exif_info: Option<ExifInfo>,
    #[serde(default, rename = "isFavorite")]
    pub is_favorite: bool,
    #[serde(default, rename = "fileCreatedAt")]
    pub file_created_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExifInfo {
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    assets: SearchAssets,
}

#[derive(Debug, Deserialize)]
pub struct SearchAssets {
    #[allow(dead_code)]
    pub total: u32,
    pub count: u32,
    pub items: Vec<Asset>,
    #[serde(default, rename = "nextPage")]
    pub next_page: Option<String>,
}

#[derive(Debug, Serialize)]
struct MetadataSearchRequest {
    #[serde(rename = "type")]
    asset_type: String,
    #[serde(rename = "isFavorite")]
    is_favorite: bool,
    #[serde(rename = "takenBefore")]
    taken_before: String,
    #[serde(rename = "withExif")]
    with_exif: bool,
    page: u32,
    size: u32,
}

#[derive(Debug, Serialize)]
struct UpdateAssetsRequest {
    ids: Vec<String>,
    description: String,
}

#[derive(Debug, Serialize)]
struct CreateTagRequest {
    name: String,
    #[serde(rename = "type")]
    tag_type: String,
}

#[derive(Debug, Deserialize)]
struct Tag {
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct LinkTagsRequest {
    #[serde(rename = "tagIds")]
    tag_ids: Vec<String>,
    #[serde(rename = "assetIds")]
    asset_ids: Vec<String>,
}

pub struct ImmichClient {
    base_url: String,
    api_key: String,
    client: Client,
}

impl ImmichClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            client,
        })
    }

    fn auth_headers(&self) -> Result<reqwest::header::HeaderMap> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            self.api_key
                .parse()
                .context("invalid API key header value")?,
        );
        headers.insert("Accept", "application/json".parse().unwrap());
        Ok(headers)
    }

    fn json_headers(&self) -> Result<reqwest::header::HeaderMap> {
        let mut headers = self.auth_headers()?;
        headers.insert("Content-Type", "application/json".parse().unwrap());
        Ok(headers)
    }

    pub fn search_image_assets(&self, page: u32, taken_before: &str) -> Result<SearchAssets> {
        let body = MetadataSearchRequest {
            asset_type: "IMAGE".to_string(),
            is_favorite: true,
            taken_before: taken_before.to_string(),
            with_exif: true,
            page,
            size: PAGE_SIZE,
        };

        let response = self
            .client
            .post(format!("{}/api/search/metadata", self.base_url))
            .headers(self.json_headers()?)
            .json(&body)
            .send()
            .context("search request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            bail!("search failed ({status}): {text}");
        }

        let parsed: SearchResponse = response.json().context("failed to parse search response")?;
        Ok(parsed.assets)
    }

    pub fn download_original(&self, asset_id: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(format!("{}/api/assets/{asset_id}/original", self.base_url))
            .headers(self.auth_headers()?)
            .send()
            .context("download request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            bail!("download failed ({status}): {text}");
        }

        response
            .bytes()
            .context("failed to read asset bytes")
            .map(|b| b.to_vec())
    }

    pub fn update_description(&self, asset_id: &str, description: &str) -> Result<()> {
        let body = UpdateAssetsRequest {
            ids: vec![asset_id.to_string()],
            description: description.to_string(),
        };

        let response = self
            .client
            .put(format!("{}/api/assets", self.base_url))
            .headers(self.json_headers()?)
            .json(&body)
            .send()
            .context("description update request failed")?;

        if matches!(
            response.status(),
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT
        ) {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            bail!("description update failed ({status}): {text}");
        }
    }

    pub fn get_or_create_tag_ids(&self, tag_names: &[String]) -> Result<Vec<String>> {
        if tag_names.is_empty() {
            return Ok(Vec::new());
        }

        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .headers(self.auth_headers()?)
            .send()
            .context("tag list request failed")?;

        let mut existing_tags: HashMap<String, String> = HashMap::new();
        if response.status().is_success() {
            let tags: Vec<Tag> = response.json().context("failed to parse tag list")?;
            for tag in tags {
                existing_tags.insert(tag.name, tag.id);
            }
        }

        let mut tag_ids = Vec::new();
        for raw_name in tag_names {
            let name = truncate_tag_name(raw_name);
            if let Some(id) = existing_tags.get(&name) {
                tag_ids.push(id.clone());
                continue;
            }

            let body = CreateTagRequest {
                name: name.clone(),
                tag_type: "CUSTOM".to_string(),
            };

            let create_res = self
                .client
                .post(format!("{}/api/tags", self.base_url))
                .headers(self.json_headers()?)
                .json(&body)
                .send()
                .context("tag create request failed")?;

            if matches!(create_res.status(), StatusCode::OK | StatusCode::CREATED) {
                let new_tag: Tag = create_res.json().context("failed to parse created tag")?;
                existing_tags.insert(new_tag.name.clone(), new_tag.id.clone());
                tag_ids.push(new_tag.id);
            } else {
                let status = create_res.status();
                let text = create_res.text().unwrap_or_default();
                eprintln!("[warn] failed to create tag '{name}' ({status}): {text}");
            }
        }

        Ok(tag_ids)
    }

    pub fn link_tags(&self, asset_id: &str, tag_ids: &[String]) -> Result<()> {
        if tag_ids.is_empty() {
            return Ok(());
        }

        let body = LinkTagsRequest {
            tag_ids: tag_ids.to_vec(),
            asset_ids: vec![asset_id.to_string()],
        };

        let response = self
            .client
            .put(format!("{}/api/tags/assets", self.base_url))
            .headers(self.json_headers()?)
            .json(&body)
            .send()
            .context("tag link request failed")?;

        if matches!(
            response.status(),
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT
        ) {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            bail!("tag link failed ({status}): {text}");
        }
    }
}

pub fn is_png_asset(asset: &Asset) -> bool {
    asset
        .original_file_name
        .as_ref()
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".png"))
}

pub fn has_empty_description(asset: &Asset) -> bool {
    let description = asset
        .exif_info
        .as_ref()
        .and_then(|info| info.description.as_deref())
        .or(asset.description.as_deref());

    match description {
        None => true,
        Some(text) => text.trim().is_empty(),
    }
}

pub fn is_favorite_asset(asset: &Asset) -> bool {
    asset.is_favorite
}

pub fn is_before_cutoff(asset: &Asset, cutoff: &DateTime<FixedOffset>) -> bool {
    let Some(file_created_at) = asset.file_created_at.as_deref() else {
        return false;
    };

    parse_datetime(file_created_at)
        .map(|created_at| created_at < *cutoff)
        .unwrap_or(false)
}

fn parse_datetime(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(
        name: &str,
        description: Option<&str>,
        is_favorite: bool,
        file_created_at: Option<&str>,
    ) -> Asset {
        Asset {
            id: "id".to_string(),
            original_file_name: Some(name.to_string()),
            description: description.map(str::to_string),
            exif_info: None,
            is_favorite,
            file_created_at: file_created_at.map(str::to_string),
        }
    }

    fn asset_with_exif_description(name: &str, exif_description: Option<&str>) -> Asset {
        Asset {
            id: "id".to_string(),
            original_file_name: Some(name.to_string()),
            description: None,
            exif_info: Some(ExifInfo {
                description: exif_description.map(str::to_string),
            }),
            is_favorite: true,
            file_created_at: Some("2026-06-19T12:00:00+09:00".to_string()),
        }
    }

    #[test]
    fn detects_png_and_empty_description() {
        assert!(is_png_asset(&asset("foo.PNG", None, true, None)));
        assert!(!is_png_asset(&asset("foo.jpg", None, true, None)));
        assert!(has_empty_description(&asset("foo.png", None, true, None)));
        assert!(!has_empty_description(&asset(
            "foo.png",
            Some("exists"),
            true,
            None
        )));
    }

    #[test]
    fn detects_favorite_assets() {
        assert!(is_favorite_asset(&asset("foo.png", None, true, None)));
        assert!(!is_favorite_asset(&asset("foo.png", None, false, None)));
    }

    #[test]
    fn detects_assets_before_cutoff() {
        let cutoff = DateTime::parse_from_rfc3339(DEFAULT_CUTOFF_DATE).unwrap();

        assert!(is_before_cutoff(
            &asset(
                "foo.png",
                None,
                true,
                Some("2026-06-19T23:59:59+09:00")
            ),
            &cutoff
        ));
        assert!(!is_before_cutoff(
            &asset(
                "foo.png",
                None,
                true,
                Some("2026-06-20T00:00:00+09:00")
            ),
            &cutoff
        ));
        assert!(!is_before_cutoff(&asset("foo.png", None, true, None), &cutoff));
        assert!(!is_before_cutoff(
            &asset("foo.png", None, true, Some("not-a-date")),
            &cutoff
        ));
    }

    #[test]
    fn detects_empty_description_from_exif_info() {
        assert!(has_empty_description(&asset_with_exif_description(
            "foo.png", None
        )));
        assert!(has_empty_description(&asset_with_exif_description(
            "foo.png",
            Some("")
        )));
        assert!(has_empty_description(&asset_with_exif_description(
            "foo.png",
            Some("   ")
        )));
        assert!(!has_empty_description(&asset_with_exif_description(
            "foo.png",
            Some("already set")
        )));
    }

    #[test]
    fn treats_missing_exif_info_as_empty_description() {
        assert!(has_empty_description(&asset("foo.png", None, true, None)));
    }

    #[test]
    fn search_request_includes_favorite_and_cutoff_filters() {
        let body = MetadataSearchRequest {
            asset_type: "IMAGE".to_string(),
            is_favorite: true,
            taken_before: DEFAULT_CUTOFF_DATE.to_string(),
            with_exif: true,
            page: 1,
            size: PAGE_SIZE,
        };

        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["type"], "IMAGE");
        assert_eq!(json["isFavorite"], true);
        assert_eq!(json["takenBefore"], DEFAULT_CUTOFF_DATE);
        assert!(json.get("createdBefore").is_none());
        assert_eq!(json["withExif"], true);
        assert_eq!(json["page"], 1);
        assert_eq!(json["size"], PAGE_SIZE);
    }
}
