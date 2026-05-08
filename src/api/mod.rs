// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

pub mod auth;
pub mod client;
pub mod models;

use std::sync::Arc;
use tokio::sync::mpsc;

use client::ApiClient;
use models::*;

// ── Request / Response types ──────────────────────────────────────────────────

#[derive(Debug)]
pub enum ApiRequest {
    LoadArtists { offset: u32 },
    LoadPlaylists { offset: u32 },
    LoadFavorites { offset: u32 },
    LoadArtistTopTracks { artist_id: u64 },
    LoadArtistAlbums { artist_id: u64 },
    LoadArtistBio { artist_id: u64 },
    LoadAlbumTracks { album_id: u64 },
    FetchAlbumArt { album_id: u64, cover_id: String },
    FetchArtistArt { artist_id: u64, picture_id: String },
    LoadPlaylistTracks { uuid: String, offset: u32 },
    Search { query: String },
    ResolveStreamUrl { track_id: u64 },
    FetchLyrics { track_id: u64 },
}

#[derive(Debug)]
pub enum ApiResponse {
    Artists(Vec<Artist>, u32 /* total */),
    Playlists(Vec<Playlist>, u32),
    Favorites(Vec<Track>, u32),
    ArtistTopTracks { artist_id: u64, tracks: Vec<Track> },
    ArtistAlbums { artist_id: u64, albums: Vec<Album> },
    AlbumTracks { album_id: u64, tracks: Vec<Track> },
    AlbumArt { album_id: u64, image_data: Vec<u8> },
    ArtistArt { artist_id: u64, image_data: Vec<u8> },
    ArtistBio { artist_id: u64, text: String },
    PlaylistTracks { uuid: String, tracks: Vec<Track>, total: u32 },
    SearchResults(Box<SearchResponse>),
    StreamUrl { track_id: u64, url: String },
    Lyrics {
        track_id: u64,
        /// LRC-parsed timed lines (secs, text). Empty when unavailable.
        synced: Vec<(f64, String)>,
        /// Plain-text lines, used only when `synced` is empty.
        plain: Vec<String>,
    },
    Error(String),
}

// ── Worker ────────────────────────────────────────────────────────────────────

pub struct ApiWorker {
    client: Arc<ApiClient>,
    rx: mpsc::UnboundedReceiver<ApiRequest>,
    tx: mpsc::UnboundedSender<ApiResponse>,
}

impl ApiWorker {
    pub fn new(
        config: Config,
        rx: mpsc::UnboundedReceiver<ApiRequest>,
        tx: mpsc::UnboundedSender<ApiResponse>,
    ) -> Self {
        Self {
            client: Arc::new(ApiClient::new(config)),
            rx,
            tx,
        }
    }

    pub async fn run(mut self) {
        while let Some(req) = self.rx.recv().await {
            let client = Arc::clone(&self.client);
            let tx = self.tx.clone();

            tokio::spawn(async move {
                let resp = handle_request(client, req).await;
                let _ = tx.send(resp);
            });
        }
    }
}

async fn handle_request(client: Arc<ApiClient>, req: ApiRequest) -> ApiResponse {
    match req {
        ApiRequest::LoadArtists { offset } => match client.get_favorite_artists(offset, 50).await {
            Ok(page) => {
                let artists = page.items.into_iter().map(|e| e.item).collect();
                ApiResponse::Artists(artists, page.total)
            }
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadPlaylists { offset } => match client.get_user_playlists(offset, 50).await {
            Ok(page) => ApiResponse::Playlists(page.items, page.total),
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadFavorites { offset } => match client.get_favorite_tracks(offset, 50).await {
            Ok(page) => {
                let tracks = page.items.into_iter().map(|e| e.item).collect();
                ApiResponse::Favorites(tracks, page.total)
            }
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::LoadArtistTopTracks { artist_id } => {
            match client.get_artist_top_tracks(artist_id, 20).await {
                Ok(page) => ApiResponse::ArtistTopTracks { artist_id, tracks: page.items },
                Err(e) => ApiResponse::Error(format!("top tracks: {e}")),
            }
        }

        ApiRequest::LoadArtistAlbums { artist_id } => {
            match client.get_artist_albums(artist_id, 30).await {
                Ok(page) => ApiResponse::ArtistAlbums { artist_id, albums: page.items },
                Err(e) => ApiResponse::Error(format!("albums: {e}")),
            }
        }

        ApiRequest::LoadArtistBio { artist_id } => {
            // A missing bio (404 or empty) is not an error — return empty string.
            let raw = match client.get_artist_bio(artist_id).await {
                Ok(resp) => resp.text
                    .or(resp.summary)
                    .unwrap_or_default(),
                Err(_) => String::new(),
            };
            let text = strip_wimplinks(&raw);
            ApiResponse::ArtistBio { artist_id, text }
        }

        ApiRequest::LoadAlbumTracks { album_id } => {
            match client.get_album_tracks(album_id).await {
                Ok(page) => ApiResponse::AlbumTracks { album_id, tracks: page.items },
                Err(e) => ApiResponse::Error(format!("album tracks: {e}")),
            }
        }

        ApiRequest::FetchAlbumArt { album_id, cover_id } => {
            let url = format!("https://resources.tidal.com/images/{}/320x320.jpg", cover_id.replace('-', "/"));
            match client.fetch_bytes(&url).await {
                Ok(data) => ApiResponse::AlbumArt { album_id, image_data: data },
                Err(e) => ApiResponse::Error(format!("album art: {e}")),
            }
        }

        ApiRequest::FetchArtistArt { artist_id, picture_id } => {
            let url = format!("https://resources.tidal.com/images/{}/320x320.jpg", picture_id.replace('-', "/"));
            match client.fetch_bytes(&url).await {
                Ok(data) => ApiResponse::ArtistArt { artist_id, image_data: data },
                Err(e) => ApiResponse::Error(format!("artist art: {e}")),
            }
        }

        ApiRequest::LoadPlaylistTracks { uuid, offset } => {
            match client.get_playlist_tracks(&uuid, offset, 100).await {
                Ok(page) => ApiResponse::PlaylistTracks {
                    uuid,
                    tracks: page.items,
                    total: page.total,
                },
                Err(e) => ApiResponse::Error(e.to_string()),
            }
        }

        ApiRequest::Search { query } => match client.search(&query, 20).await {
            Ok(results) => ApiResponse::SearchResults(Box::new(results)),
            Err(e) => ApiResponse::Error(e.to_string()),
        },

        ApiRequest::ResolveStreamUrl { track_id } => {
            match client.get_stream_url(track_id).await {
                Ok(url) => ApiResponse::StreamUrl { track_id, url },
                Err(e) => ApiResponse::Error(e.to_string()),
            }
        }

        ApiRequest::FetchLyrics { track_id } => {
            // A 404 (no lyrics) or any other error → return empty; never emit Error.
            let (synced, plain) = match client.get_track_lyrics(track_id).await {
                Ok(resp) => {
                    let synced = resp.subtitles.as_deref()
                        .filter(|s| !s.is_empty())
                        .map(parse_lrc)
                        .unwrap_or_default();
                    let plain = if synced.is_empty() {
                        resp.lyrics.as_deref().unwrap_or("").lines()
                            .map(str::to_string)
                            .filter(|l| !l.is_empty())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    (synced, plain)
                }
                Err(_) => (Vec::new(), Vec::new()),
            };
            ApiResponse::Lyrics { track_id, synced, plain }
        }
    }
}

fn parse_lrc(s: &str) -> Vec<(f64, String)> {
    let mut lines = Vec::new();
    for raw in s.lines() {
        let raw = raw.trim();
        if !raw.starts_with('[') {
            continue;
        }
        let Some(close) = raw.find(']') else { continue };
        let tag = &raw[1..close];
        let text = raw[close + 1..].trim().to_string();
        if text.is_empty() {
            continue;
        }
        if let Some(secs) = parse_lrc_time(tag) {
            lines.push((secs, text));
        }
    }
    lines.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

fn parse_lrc_time(s: &str) -> Option<f64> {
    let colon = s.find(':')?;
    let mins: f64 = s[..colon].parse().ok()?;
    let secs: f64 = s[colon + 1..].parse().ok()?;
    Some(mins * 60.0 + secs)
}

/// Strip Tidal's [wimpLink ...]...[/wimpLink] markup, keeping the inner text.
fn strip_wimplinks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        if let Some(close) = rest.find(']') {
            rest = &rest[close + 1..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}
