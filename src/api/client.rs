// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use super::auth::refresh_token_async;
use super::models::*;

const BASE: &str = "https://api.tidal.com/v1";
const CLIENT_VERSION: &str = "2025.7.16";
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 12; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/91.0.4472.114 Safari/537.36";

pub struct ApiClient {
    http: reqwest::Client,
    token: RwLock<String>,
    config: Config,
}

impl ApiClient {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .user_agent(USER_AGENT)
            .build()
            .expect("failed to build HTTP client");
        let token = config.access_token.clone().unwrap_or_default();
        Self {
            http,
            token: RwLock::new(token),
            config,
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        // Build base params that Tidal requires on every request
        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }
        all_params.extend_from_slice(params);

        let resp = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .header("x-tidal-client-version", CLIENT_VERSION)
            .query(&all_params)
            .send()
            .await
            .context("HTTP request failed")?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            let new_token = refresh_token_async(&self.config, &self.http).await?;
            let new_access = new_token.access_token.clone();
            *self.token.write().await = new_access.clone();

            return Ok(self
                .http
                .get(&url)
                .bearer_auth(&new_access)
                .header("x-tidal-client-version", CLIENT_VERSION)
                .query(&all_params)
                .send()
                .await?
                .error_for_status()?
                .json::<T>()
                .await?);
        }

        let bytes = resp.error_for_status()?.bytes().await?;
        serde_json::from_slice::<T>(&bytes).map_err(|e| {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
            anyhow::anyhow!("{e} — body: {snippet}")
        })
    }

    fn cc(&self) -> String {
        self.config.country_code.clone()
    }

    fn uid(&self) -> Result<u64> {
        self.config.user_id.context("user_id not set — re-run to re-authenticate")
    }

    // ── Artists ───────────────────────────────────────────────────────────────

    pub async fn get_favorite_artists(&self, offset: u32, limit: u32) -> Result<Page<FavoriteArtistEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/artists"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn get_artist_top_tracks(&self, artist_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/artists/{artist_id}/toptracks"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_albums(&self, artist_id: u64, limit: u32) -> Result<Page<Album>> {
        self.get(
            &format!("/artists/{artist_id}/albums"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_bio(&self, artist_id: u64) -> Result<ArtistBioResponse> {
        self.get(&format!("/artists/{artist_id}/bio"), &[]).await
    }

    // ── Playlists ─────────────────────────────────────────────────────────────

    pub async fn get_user_playlists(&self, offset: u32, limit: u32) -> Result<Page<Playlist>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/playlists"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    pub async fn get_playlist_tracks(&self, uuid: &str, offset: u32, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/playlists/{uuid}/tracks"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    // ── Favorites ─────────────────────────────────────────────────────────────

    pub async fn get_favorite_tracks(&self, offset: u32, limit: u32) -> Result<Page<FavoriteTrackEntry>> {
        let uid = self.uid()?;
        self.get(
            &format!("/users/{uid}/favorites/tracks"),
            &[
                ("limit", limit.to_string()),
                ("offset", offset.to_string()),
            ],
        )
        .await
    }

    // ── Search ────────────────────────────────────────────────────────────────

    pub async fn search(&self, query: &str, limit: u32) -> Result<SearchResponse> {
        self.get(
            "/search",
            &[
                ("query", query.to_string()),
                ("types", "ARTISTS,ALBUMS,TRACKS,PLAYLISTS".to_string()),
                ("limit", limit.to_string()),
            ],
        )
        .await
    }

    // ── Albums ────────────────────────────────────────────────────────────────

    pub async fn get_album_tracks(&self, album_id: u64) -> Result<Page<Track>> {
        self.get(
            &format!("/albums/{album_id}/tracks"),
            &[("limit", "50".to_string())],
        )
        .await
    }

    // ── Lyrics ───────────────────────────────────────────────────────────────

    pub async fn get_track_lyrics(&self, track_id: u64) -> Result<LyricsResponse> {
        self.get(&format!("/tracks/{track_id}/lyrics"), &[]).await
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    /// Fetch raw bytes from a public URL (e.g. Tidal's cover art CDN).
    pub async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        Ok(self.http.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
    }

    pub async fn get_stream_url(&self, track_id: u64) -> Result<String> {
        let resp: StreamUrlResponse = self.get(
            &format!("/tracks/{track_id}/urlpostpaywall"),
            &[
                ("urlusagemode", "STREAM".to_string()),
                ("audioquality", "HIGH".to_string()),
                ("assetpresentation", "FULL".to_string()),
            ],
        )
        .await?;

        resp.urls.into_iter().next().context("empty URL list from Tidal")
    }
}
