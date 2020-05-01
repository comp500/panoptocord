use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use anyhow::{Context, format_err, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use failure::Fail;
use futures::future::try_join_all;
use oauth2::{AsyncRefreshTokenRequest, AuthType, Scope, TokenResponse};
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
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

	let config: Config = serde_json::from_reader(File::open(Path::new(&config_path))?)?;
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
		let _ = serde_json::to_writer_pretty(File::create(Path::new("panoptocord-cache.json"))?, &cache)?;
		println!("Token refreshed!");
	}

	println!("Starting request loop...");

	let mut interval = tokio::time::interval(Duration::minutes(10).to_std()?);
	let client = reqwest::Client::new();
	loop {
		interval.tick().await;
		if cache.access_token_expires.lt(&Utc::now()) {
			println!("Token expired, refreshing...");
			if let Err(err) = refresh_token(&mut cache, &config).await {
				eprintln!("Error refreshing access token: {:?}", err);
				let _ = webhook::post_message(config.webhook_url.clone(), "Failed to refresh access token!".to_string()).await;
			} else {
				// Save the file
				let _ = serde_json::to_writer_pretty(File::create(Path::new("panoptocord-cache.json"))?, &cache)?;
			}
		}

		if let Err(err) = make_requests(&cache, &config, &client).await {
			eprintln!("Error making requests: {:?}", err);
		} else {
			cache.last_updated = Utc::now();
			let _ = serde_json::to_writer_pretty(File::create(Path::new("panoptocord-cache.json"))?, &cache)?;
		}
	}
}

async fn make_requests(cache: &CacheFile, config: &Config, client: &reqwest::Client) -> Result<()> {
	try_join_all(config.folders.iter()
		.map(|folder| make_request_and_publish(
			&cache.access_token, &folder,
			&config.panopto_base, &config.webhook_url, client, &cache.last_updated))).await?;
	Ok(())
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

async fn make_request_and_publish(access_token: &oauth2::AccessToken, folder_id: &String, panopto_base: &String,
								  webhook_url: &String, client: &reqwest::Client, last_query_time: &DateTime<Utc>) -> Result<()> {
	let res = make_request(access_token, &folder_id, &panopto_base, client).await?;
	try_join_all(res.results.into_iter()
		.filter(|session| session.start_time.map_or(false, |t| t.gt(last_query_time)))
		.map(|session| send_discord_message(webhook_url.clone(), panopto_base.clone(), session)))
		.await?;
	Ok(())
}

async fn make_request(access_token: &oauth2::AccessToken, folder_id: &String, panopto_base: &String, client: &reqwest::Client) -> Result<PanoptoResponse> {
	Ok(client.get(&format!("{}Panopto/api/v1/folders/{}/sessions?sortField=CreatedDate&sortOrder=Desc", panopto_base, folder_id))
		.bearer_auth(access_token.secret())
		.send()
		.await?
		.json::<PanoptoResponse>().await?)
}

async fn refresh_token(cache: &mut CacheFile, config: &Config) -> Result<()> {
	let client = BasicClient::new(
		config.client_id.clone(),
		Some(config.client_secret.clone()),
		config.authorization_url.clone(),
		Some(config.access_token_url.clone())
	).set_auth_type(AuthType::RequestBody);

	match client.exchange_refresh_token(&cache.refresh_token)
		.add_scope(Scope::new("api".to_string()))
		.add_scope(Scope::new("offline_access".to_string()))
		.request_async(async_http_client).await {
		Ok(res) => {
			cache.access_token = res.access_token().clone();
			if let Some(refresh_token) = res.refresh_token() {
				cache.refresh_token = refresh_token.clone();
			}
			if let Some(expires_in) = res.expires_in() {
				cache.access_token_expires = Utc::now() + Duration::from_std(expires_in)?;
			}
			Ok(())
		}
		Err(err) => {
			match err {
				oauth2::RequestTokenError::ServerResponse(err) => {
					let err_string = err.error_description().map(|s| s.clone())
						.unwrap_or(format!("{:?}", err.error()));
					Err(format_err!(err_string)).context("Returned error by server")
				},
				oauth2::RequestTokenError::Request(err) => Err(err.compat()).context("Failed to send/recv request"),
				oauth2::RequestTokenError::Parse(err, _data) => Err(err).context("Failed to parse JSON response"),
				oauth2::RequestTokenError::Other(err) => Err(format_err!(err)).context("Unexpected response")
			}
		}
	}
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