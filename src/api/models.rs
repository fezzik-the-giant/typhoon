// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use serde::{Deserialize, Serialize};

// ── Pagination envelope ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Page<T> {
    #[serde(rename = "totalNumberOfItems")]
    pub total: u32,
    pub items: Vec<T>,
}

// ── References ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ArtistRef {
    pub name: String,
}

// ── Artists ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Artist {
    pub id: u64,
    pub name: String,
    pub picture: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FavoriteArtistEntry {
    pub item: Artist,
}

// ── Albums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone, Default)]
pub struct MediaMetadata {
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Album {
    pub id: u64,
    pub title: String,
    #[serde(rename = "numberOfTracks")]
    pub number_of_tracks: Option<u32>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
    pub cover: Option<String>,
    pub artist: Option<ArtistRef>,
    #[serde(rename = "audioQuality", default)]
    pub audio_quality: Option<String>,
    #[serde(rename = "mediaMetadata", default)]
    pub media_metadata: Option<MediaMetadata>,
}

impl Album {
    pub fn quality_badge(&self) -> Option<&'static str> {
        let tags = self.media_metadata.as_ref().map(|m| m.tags.as_slice()).unwrap_or(&[]);
        if tags.iter().any(|t| t == "HIRES_LOSSLESS") {
            return Some("MAX");
        }
        if tags.iter().any(|t| t == "LOSSLESS") {
            return Some("HI-FI");
        }
        match self.audio_quality.as_deref() {
            Some("HI_RES") => Some("MQA"),
            Some("HIGH")   => Some("320"),
            _              => None,
        }
    }
}

// ── Tracks ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Track {
    pub id: u64,
    pub title: String,
    pub duration: u32,
    /// Present on most endpoints; absent on search results which use `artists`.
    pub artist: Option<ArtistRef>,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    pub album: Album,
    #[serde(rename = "audioQuality")]
    pub audio_quality: Option<String>,
}

impl Track {
    pub fn duration_display(&self) -> String {
        let m = self.duration / 60;
        let s = self.duration % 60;
        format!("{m}:{s:02}")
    }

    pub fn artist_name(&self) -> &str {
        self.artist.as_ref()
            .or_else(|| self.artists.first())
            .map(|a| a.name.as_str())
            .unwrap_or("")
    }

    pub fn quality_display(&self) -> &str {
        match self.audio_quality.as_deref() {
            Some("HI_RES_LOSSLESS") => "Hi-Res",
            Some("HI_RES")          => "MQA",
            Some("LOSSLESS")        => "FLAC",
            Some("HIGH")            => "AAC 320",
            Some("LOW")             => "AAC 96",
            Some(other)             => other,
            None                    => "",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FavoriteTrackEntry {
    pub item: Track,
}

// ── Playlists ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct Playlist {
    pub uuid: String,
    pub title: String,
    #[serde(rename = "numberOfTracks")]
    pub number_of_tracks: u32,
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub artists: Option<Page<Artist>>,
    pub tracks: Option<Page<Track>>,
    pub playlists: Option<Page<Playlist>>,
}

// ── Artist bio ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ArtistBioResponse {
    pub text: Option<String>,
    pub summary: Option<String>,
}

// ── Lyrics ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LyricsResponse {
    pub lyrics: Option<String>,
    /// LRC-format timed subtitles, when available.
    pub subtitles: Option<String>,
}

// ── Stream URL ────────────────────────────────────────────────────────────────


/// Response from /tracks/{id}/playbackinfopostpaywall
#[derive(Debug, Deserialize)]
pub struct PlaybackInfo {
    #[serde(rename = "manifestMimeType")]
    pub manifest_mime_type: String,
    pub manifest: String,
}

/// Decoded content of a `application/vnd.tidal.bts` manifest
#[derive(Debug, Deserialize)]
pub struct BtsManifest {
    pub urls: Vec<String>,
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// Response from GET /sessions — needed after every fresh auth.
#[derive(Debug, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "userId")]
    pub user_id: u64,
    #[serde(rename = "countryCode")]
    pub country_code: String,
}


// ── OAuth ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    pub device_code: String,
    #[serde(rename = "userCode")]
    pub user_code: String,
    #[serde(rename = "verificationUriComplete")]
    pub verification_uri_complete: String,
    pub interval: u32,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub user: Option<TokenUser>,
}

#[derive(Debug, Deserialize)]
pub struct TokenUser {
    #[serde(rename = "userId")]
    pub user_id: u64,
    #[serde(rename = "countryCode")]
    pub country_code: String,
}

// ── Config (persisted to disk) ────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// Override the OAuth client ID. Falls back to the built-in default when absent.
    pub client_id: Option<String>,
    /// Override the OAuth client secret. Falls back to the built-in default when absent.
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// RFC 3339 expiry timestamp
    pub expires_at: Option<String>,
    pub user_id: Option<u64>,
    pub country_code: String,
    /// Tidal session UUID — required as `sessionId` query param on all v1 requests.
    pub session_id: Option<String>,
}
