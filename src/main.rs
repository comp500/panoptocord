use std::{error, io, thread};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use anyhow::{format_err, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use futures::executor::block_on;
use futures::future::try_join_all;
use random_color::RandomColor;
use serde::{Deserialize, Serialize};

mod webhook;

#[tokio::main]
async fn main() -> Result<()> {
	let config_path = std::env::args().nth(1).unwrap_or("config.json".to_string());
	println!("Loading configuration from file: {}", config_path);

	fn read_cache() -> Result<CacheFile> {
		let reader = File::open(Path::new("panoptocord-cache.json"))?;
		Ok(serde_json::from_reader(reader)?)
	}

	let mut config: Config = serde_json::from_reader(File::open(Path::new(&config_path))?)?;
	let mut cache: CacheFile = read_cache()
		.or_else(|_err| -> Result<CacheFile> {
			let new_file = CacheFile {
				// TODO: make this utc::now?
				last_updated: Utc.ymd(2020, 1, 1).and_hms(0, 0, 0),
				refresh_token: config.refresh_token.clone(),
				access_token: config.access_token.clone(),
				access_token_expires: Utc.ymd(2020, 1, 1).and_hms(0, 0, 0),
				color_map: HashMap::new(),
				last_changed_refresh_token: config.refresh_token.clone(),
				last_changed_access_token: config.access_token.clone()
			};
			Ok(new_file)
		})?;

	// If the config was updated after the cache was last updated, refresh access tokens
	if cache.last_changed_refresh_token.secret() != config.refresh_token.secret() || cache.last_changed_access_token.secret() != config.access_token.secret() {
		println!("Token invalidated, refreshing...");
		cache.last_changed_access_token = config.access_token.clone();
		cache.last_changed_refresh_token = config.refresh_token.clone();
		cache.access_token = config.access_token.clone();
		cache.refresh_token = config.refresh_token.clone();
		refresh_token(&mut cache, &config).await?;
		println!("Token refreshed!");
	}
	// TODO: do this
	// To save:
	// let _ = serde_json::to_writer_pretty(File::create(Path::new("panoptocord-cache.json"))?, &new_file)?;

	println!("Starting request loop...");

	let mut interval = tokio::time::interval(Duration::seconds(20).to_std()?);
	loop {
		interval.tick().await;
		if cache.access_token_expires.lt(&Utc::now()) {
			println!("Token expired, refreshing...");
			if let Err(err) = refresh_token(&mut cache, &config).await {
				eprintln!("Error refreshing access token: {}", err);
				let _ = webhook::post_message(config.webhook_url.clone(), "Failed to refresh access token!".to_string()).await;
			}
		}

		if let Err(err) = make_requests(&cache, &config).await {
			eprintln!("Error making requests: {}", err);
		}
	}
}

async fn make_requests(cache: &CacheFile, config: &Config) -> Result<Vec<()>, Box<dyn error::Error>> {
	let res = make_request().await?;
	Ok(try_join_all(res.results.into_iter()
		.map(|session| send_discord_message(config.webhook_url.clone(), config.panopto_base.clone(), session)))
		.await?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheFile {
	pub last_updated: DateTime<Utc>,
	pub refresh_token: oauth2::RefreshToken,
	pub access_token: oauth2::AccessToken,
	pub access_token_expires: DateTime<Utc>,
	pub color_map: HashMap<String, [u32; 3]>,
	pub last_changed_refresh_token: oauth2::RefreshToken,
	pub last_changed_access_token: oauth2::AccessToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
	pub authorization_url: oauth2::AuthUrl,
	pub access_token_url: oauth2::TokenUrl,
	pub client_id: oauth2::ClientId,
	pub client_secret: oauth2::ClientSecret,
	pub refresh_token: oauth2::RefreshToken,
	pub access_token: oauth2::AccessToken,
	pub folders: Vec<String>,
	pub webhook_url: String,
	pub panopto_base: String
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanoptoResponse {
	#[serde(rename = "Results")]
	pub results: Vec<PanoptoSession>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanoptoSession {
	#[serde(rename = "Description")]
	pub description: Option<String>,
	#[serde(rename = "StartTime")]
	pub start_time: Option<DateTime<Utc>>,
	#[serde(rename = "Duration")]
	pub duration: f64,
	#[serde(rename = "MostRecentViewPosition")]
	pub most_recent_view_position: Option<f64>,
	#[serde(rename = "CreatedBy")]
	pub created_by: CreatedBy,
	#[serde(rename = "Urls")]
	pub urls: Urls,
	#[serde(rename = "Folder")]
	pub folder: String,
	#[serde(rename = "FolderDetails")]
	pub folder_details: FolderDetails,
	#[serde(rename = "Id")]
	pub id: String,
	#[serde(rename = "Name")]
	pub name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedBy {
	#[serde(rename = "Id")]
	pub id: String,
	#[serde(rename = "Username")]
	pub username: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Urls {
	#[serde(rename = "ViewerUrl")]
	pub viewer_url: String,
	#[serde(rename = "EmbedUrl")]
	pub embed_url: Option<String>,
	#[serde(rename = "ShareSettingsUrl")]
	pub share_settings_url: Option<String>,
	#[serde(rename = "DownloadUrl")]
	pub download_url: Option<String>,
	#[serde(rename = "CaptionDownloadUrl")]
	pub caption_download_url: Option<String>,
	#[serde(rename = "EditorUrl")]
	pub editor_url: Option<String>,
	#[serde(rename = "ThumbnailUrl")]
	pub thumbnail_url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDetails {
	#[serde(rename = "Id")]
	pub id: String,
	#[serde(rename = "Name")]
	pub name: String,
}

async fn make_request() -> Result<PanoptoResponse> {
	Err(format_err!("Not yet implemented!!"))
}

async fn refresh_token(cache: &mut CacheFile, config: &Config) -> Result<()> {
	Err(format_err!("Not yet implemented!!"))
}

async fn send_discord_message(webhook_url: String, panopto_base: String, session: PanoptoSession) -> Result<()> {
	webhook::post_recording(
		session.name,
		session.folder_details.name,
		webhook_url,
		// TODO: use random color from cache
		RandomColor::new().to_rgb_array(),
		session.start_time.unwrap_or(Utc::now()),
		session.urls.viewer_url,
		session.urls.thumbnail_url,
		format!("{}Panopto/Pages/Sessions/List.aspx#folderID=%22{}%22", panopto_base, session.folder_details.id),
		chrono::Duration::seconds(session.duration as i64),
		session.description
	).await
}