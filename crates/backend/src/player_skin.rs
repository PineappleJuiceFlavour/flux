//! Look up another player's skin (and cape) by name without saving anything, so the skins page can
//! preview it before the user keeps or applies it.

use std::sync::Arc;

use bridge::message::PlayerSkinPreview;
use schema::{minecraft_profile::SkinVariant, unique_bytes::UniqueBytes};

use crate::{BackendState, skin_manager::SkinManager};

impl BackendState {
    async fn get_text(&self, url: &str, what: &str) -> Result<String, Arc<str>> {
        let response = self.http_client_provider.client().get(url).send().await.map_err(|err| {
            log::error!("Player skin: failed to request {what}: {err:?}");
            Arc::<str>::from(format!("Couldn't reach {what}"))
        })?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err("Player not found".into());
        }
        if !response.status().is_success() {
            return Err(format!("{what} returned status {}", response.status()).into());
        }
        response.text().await.map_err(|_| format!("Couldn't read the {what} response").into())
    }

    async fn get_png(&self, url: &str) -> Result<Vec<u8>, Arc<str>> {
        let url = url::Url::parse(url).map_err(|_| Arc::<str>::from("Invalid texture URL"))?;
        let response = self.http_client_provider.redirecting().get(url).send().await
            .map_err(|_| Arc::<str>::from("Couldn't download the texture"))?;
        if !response.status().is_success() {
            return Err(format!("Texture download returned status {}", response.status()).into());
        }
        Ok(response.bytes().await.map_err(|_| Arc::<str>::from("Couldn't download the texture"))?.to_vec())
    }

    pub async fn fetch_player_skin(&self, username: &str) -> Result<PlayerSkinPreview, Arc<str>> {
        use base64::Engine;

        let username = username.trim();
        if username.is_empty() {
            return Err("Type a player name".into());
        }
        let lookup = self.get_text(&format!("https://api.mojang.com/minecraft/profile/lookup/name/{username}"), "the Mojang API").await
            .map_err(|err| if &*err == "Player not found" { Arc::from(format!("Player '{username}' not found")) } else { err })?;
        let lookup: serde_json::Value = serde_json::from_str(&lookup).map_err(|_| Arc::<str>::from("Unexpected Mojang API response"))?;
        let (Some(uuid), Some(name)) = (lookup["id"].as_str(), lookup["name"].as_str()) else {
            return Err("Unexpected Mojang API response".into());
        };

        let profile = self.get_text(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"), "the Mojang session server").await?;
        let profile: serde_json::Value = serde_json::from_str(&profile).map_err(|_| Arc::<str>::from("Unexpected session server response"))?;
        let textures = profile["properties"].as_array().into_iter().flatten()
            .find(|prop| prop["name"].as_str() == Some("textures"))
            .and_then(|prop| prop["value"].as_str())
            .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
            .and_then(|json| serde_json::from_slice::<serde_json::Value>(&json).ok())
            .ok_or_else(|| Arc::<str>::from(format!("Player '{name}' has no skin")))?;

        let skin = &textures["textures"]["SKIN"];
        let skin_url = skin["url"].as_str().ok_or_else(|| Arc::<str>::from(format!("Player '{name}' has no skin")))?;
        let variant = if skin["metadata"]["model"].as_str() == Some("slim") { SkinVariant::Slim } else { SkinVariant::Classic };

        let skin_bytes = self.get_png(skin_url).await?;
        let image = image::load_from_memory_with_format(&skin_bytes, image::ImageFormat::Png)
            .map_err(|_| Arc::<str>::from("The player's skin is not a valid PNG"))?;
        if !SkinManager::is_valid_size(&image) {
            return Err("The player's skin has invalid dimensions (must be 64x64 or 64x32)".into());
        }

        // The cape is a nice-to-have for the preview; a failed cape download doesn't fail the lookup.
        let cape = match textures["textures"]["CAPE"]["url"].as_str() {
            Some(url) => self.get_png(url).await.ok().map(|bytes| UniqueBytes::new(&bytes)),
            None => None,
        };

        Ok(PlayerSkinPreview { name: name.into(), skin: UniqueBytes::new(&skin_bytes), variant, cape })
    }
}
