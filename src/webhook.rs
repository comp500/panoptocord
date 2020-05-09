use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscordRequest {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub content: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub embeds: Option<Vec<Embed>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Embed {
	pub title: String,
	pub description: Option<String>,
	pub url: String,
	pub color: u32,
	pub timestamp: DateTime<Utc>,
	pub footer: Footer,
	pub image: Image,
	pub author: Author,
	pub fields: Vec<Field>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Footer {
	pub text: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Image {
	pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Author {
	pub name: String,
	pub url: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Field {
	pub name: String,
	pub value: String,
}


pub async fn post_recording(name: String, folder_name: String, webhook_url: String, color: [u32; 3],
					  start_time: DateTime<Utc>, viewer_url: String, image_url: String,
					  folder_url: String, duration: Duration, description: Option<String>) -> Result<()> {
	post_webhook(webhook_url, DiscordRequest {
		embeds: Some(vec![
			Embed {
				title: name,
				description,
				url: viewer_url,
				color: serenity::utils::Color::from_rgb(color[0] as u8, color[1] as u8, color[2] as u8).0,
				timestamp: start_time,
				footer: Footer {
					text: "panoptocord".to_string()
				},
				image: Image {
					url: image_url
				},
				author: Author {
					name: folder_name,
					url: folder_url
				},
				fields: vec![
					Field {
						name: "Duration".to_string(),
						value: humantime::format_duration(duration.to_std()?).to_string()
					}
				]
			}
		]),
		content: None
	}).await
}

pub async fn post_message(webhook_url: String, message: String) -> Result<()> {
	post_webhook(webhook_url, DiscordRequest {
		content: Some(message),
		embeds: None
	}).await
}

async fn post_webhook(webhook_url: String, req: DiscordRequest) -> Result<()> {
	let client = reqwest::Client::new();
	let new_url = webhook_url.clone() + "?wait=true";
	client.post(&new_url).json(&req).send().await?;
	Ok(())
}