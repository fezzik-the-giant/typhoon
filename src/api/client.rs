// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{Context, Result};
use base64::Engine as _;
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


    async fn post_form(&self, path: &str, form: &[(&str, String)]) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }

        self.http
            .post(&url)
            .bearer_auth(&token)
            .header("x-tidal-client-version", CLIENT_VERSION)
            .query(&all_params)
            .form(form)
            .send()
            .await
            .context("HTTP POST failed")?
            .error_for_status()?;
        Ok(())
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
                ("order", "DATE".to_string()),
                ("orderDirection", "DESC".to_string()),
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

    pub async fn get_album(&self, album_id: u64) -> Result<Album> {
        self.get(&format!("/albums/{album_id}"), &[]).await
    }

    pub async fn get_album_tracks(&self, album_id: u64) -> Result<Page<Track>> {
        self.get(
            &format!("/albums/{album_id}/tracks"),
            &[("limit", "50".to_string())],
        )
        .await
    }

    // ── Radio ─────────────────────────────────────────────────────────────────

    pub async fn get_track_radio(&self, track_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/tracks/{track_id}/radio"),
            &[("limit", limit.to_string())],
        )
        .await
    }

    pub async fn get_artist_radio(&self, artist_id: u64, limit: u32) -> Result<Page<Track>> {
        self.get(
            &format!("/artists/{artist_id}/radio"),
            &[("limit", limit.to_string())],
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

    async fn delete(&self, path: &str) -> Result<()> {
        let token = self.token.read().await.clone();
        let url = format!("{BASE}{path}");

        let mut all_params: Vec<(&str, String)> = vec![
            ("countryCode", self.config.country_code.clone()),
        ];
        if let Some(sid) = &self.config.session_id {
            all_params.push(("sessionId", sid.clone()));
        }

        self.http
            .delete(&url)
            .bearer_auth(&token)
            .header("x-tidal-client-version", CLIENT_VERSION)
            .query(&all_params)
            .send()
            .await
            .context("HTTP DELETE failed")?
            .error_for_status()?;
        Ok(())
    }

    pub async fn add_favorite_track(&self, track_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.post_form(
            &format!("/users/{uid}/favorites/tracks"),
            &[("trackId", track_id.to_string())],
        ).await
    }

    pub async fn follow_artist(&self, artist_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.post_form(
            &format!("/users/{uid}/favorites/artists"),
            &[("artistId", artist_id.to_string())],
        ).await
    }

    pub async fn remove_favorite_track(&self, track_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.delete(&format!("/users/{uid}/favorites/tracks/{track_id}")).await
    }

    pub async fn unfollow_artist(&self, artist_id: u64) -> Result<()> {
        let uid = self.uid()?;
        self.delete(&format!("/users/{uid}/favorites/artists/{artist_id}")).await
    }

    pub async fn get_stream_url(&self, track_id: u64) -> Result<String> {
        const QUALITIES: &[&str] = &["HI_RES_LOSSLESS", "LOSSLESS", "HIGH"];
        let path = format!("/tracks/{track_id}/playbackinfopostpaywall");

        for &quality in QUALITIES {
            let result: Result<PlaybackInfo> = self.get(
                &path,
                &[
                    ("audioquality", quality.to_string()),
                    ("playbackmode", "STREAM".to_string()),
                    ("assetpresentation", "FULL".to_string()),
                ],
            ).await;

            match result {
                Ok(info) => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(&info.manifest)
                        .context("base64 decode of manifest")?;
                    match info.manifest_mime_type.as_str() {
                        "application/vnd.tidal.bts" => {
                            let manifest: BtsManifest = serde_json::from_slice(&bytes)
                                .context("parse BTS manifest")?;
                            if let Some(url) = manifest.urls.into_iter().next() {
                                return Ok(url);
                            }
                        }
                        "application/dash+xml" => {
                            let xml = String::from_utf8_lossy(&bytes);
                            let path = dash_to_hls(track_id, &xml)
                                .context("convert DASH manifest to HLS")?;
                            return Ok(path);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    let status = e.downcast_ref::<reqwest::Error>()
                        .and_then(|re| re.status());
                    let entitlement_denied = matches!(
                        status,
                        Some(reqwest::StatusCode::UNAUTHORIZED) | Some(reqwest::StatusCode::FORBIDDEN)
                    );
                    if entitlement_denied {
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(anyhow::anyhow!("no stream URL available for track {track_id}"))
    }
}

// ── DASH → HLS conversion ─────────────────────────────────────────────────────

/// Convert a Tidal DASH manifest to an HLS playlist served via local HTTP.
fn dash_to_hls(track_id: u64, xml: &str) -> anyhow::Result<String> {
    let init_url = dash_attr(xml, "initialization")
        .context("no initialization URL in DASH manifest")?;
    let media_tmpl = dash_attr(xml, "media")
        .context("no media template in DASH manifest")?;
    let timescale: f64 = dash_attr(xml, "timescale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let start_num: u64 = dash_attr(xml, "startNumber")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let durations = dash_segment_durations(xml, timescale);
    anyhow::ensure!(!durations.is_empty(), "no segments in DASH manifest");

    let target = durations.iter().cloned().fold(0f64, f64::max).ceil() as u64;
    let mut m3u8 = format!(
        "#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-TARGETDURATION:{target}\n#EXT-X-MAP:URI=\"{init_url}\"\n"
    );
    for (i, dur) in durations.iter().enumerate() {
        m3u8.push_str(&format!("#EXTINF:{dur:.5},\n"));
        m3u8.push_str(&media_tmpl.replace("$Number$", &(start_num + i as u64).to_string()));
        m3u8.push('\n');
    }
    m3u8.push_str("#EXT-X-ENDLIST\n");

    std::fs::write(format!("/tmp/riptide_hls_{track_id}.m3u8"), &m3u8)
        .context("write HLS playlist")?;
    Ok(format!("http://127.0.0.1:{}/{track_id}.m3u8", crate::manifest::PORT))
}

/// Extract an XML attribute value by name, checking that it isn't a substring
/// of a longer attribute name (e.g. `d` must not match `id`).
fn dash_attr(xml: &str, name: &str) -> Option<String> {
    let needle = format!("{}=\"", name);
    let mut haystack = xml;
    while let Some(pos) = haystack.find(&needle) {
        let before = pos
            .checked_sub(1)
            .and_then(|i| haystack.as_bytes().get(i).copied())
            .map(|b| b as char)
            .unwrap_or(' ');
        if !before.is_alphanumeric() && before != '_' && before != '-' {
            let start = pos + needle.len();
            let end = haystack[start..].find('"')? + start;
            return Some(haystack[start..end].to_owned());
        }
        haystack = &haystack[pos + needle.len()..];
    }
    None
}

/// Parse `<S d="..." r="..."/>` elements inside `<SegmentTimeline>`.
fn dash_segment_durations(xml: &str, timescale: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let tl_start = match xml.find("<SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let tl = &xml[tl_start..];
    let tl_end = match tl.find("</SegmentTimeline>") {
        Some(p) => p,
        None => return out,
    };
    let mut rest = &tl[..tl_end];
    while let Some(pos) = rest.find("<S ") {
        let inner_start = pos + 3;
        let inner_end = rest[inner_start..]
            .find("/>")
            .map(|p| p + inner_start)
            .unwrap_or(rest.len());
        let elem = &rest[inner_start..inner_end];
        let d: f64 = dash_attr(elem, "d").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let r: usize = dash_attr(elem, "r").and_then(|s| s.parse().ok()).unwrap_or(0);
        let dur = d / timescale;
        for _ in 0..=r {
            out.push(dur);
        }
        rest = &rest[inner_end..];
    }
    out
}
