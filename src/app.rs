// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use tokio::sync::{mpsc, watch};

use crate::api::{ApiRequest, ApiResponse};
use crate::api::models::*;
use crate::mpris::MprisState;
use crate::player::{PlayerCmd, PlayerEvent};

// ── Tab ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Artists,
    Albums,
    Playlists,
    Favorites,
    Search,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Favorites, Tab::Artists, Tab::Albums, Tab::Playlists, Tab::Search];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Favorites => "Favorites",
            Tab::Artists => "Artists",
            Tab::Albums => "Albums",
            Tab::Playlists => "Playlists",
            Tab::Search => "Search",
        }
    }

}

// ── StatefulList ──────────────────────────────────────────────────────────────

pub struct StatefulList<T> {
    pub items: Vec<T>,
    pub selected: usize,
    pub loading: bool,
    pub exhausted: bool,
    pub next_offset: u32,
    pub total: u32,
}

impl<T> Default for StatefulList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            loading: false,
            exhausted: false,
            next_offset: 0,
            total: 0,
        }
    }
}

impl<T> StatefulList<T> {
    pub fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.items.len() - 1);
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.items.get(self.selected)
    }

    pub fn should_load_more(&self) -> bool {
        !self.loading
            && !self.exhausted
            && !self.items.is_empty()
            && self.selected + 10 >= self.items.len()
    }

    pub fn append(&mut self, new_items: Vec<T>, total: u32) {
        self.next_offset = (self.items.len() + new_items.len()) as u32;
        self.total = total;
        self.exhausted = self.next_offset >= total;
        self.items.extend(new_items);
        self.loading = false;
    }

}

// ── Artist detail ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistDetailFocus {
    Tracks,
    Albums,
    Bio,
}

pub struct ArtistDetail {
    pub artist: Artist,
    pub tracks: StatefulList<Track>,
    pub albums: StatefulList<Album>,
    pub focus: ArtistDetailFocus,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub art_cache: std::cell::RefCell<Option<(u16, u16, ArtPayload)>>,
    pub bio: Option<String>,
    pub bio_loading: bool,
    pub bio_scroll: u16,
}

// ── Playlist detail ───────────────────────────────────────────────────────────

pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub tracks: StatefulList<Track>,
}

// ── Album detail ──────────────────────────────────────────────────────────────

pub enum ArtPayload {
    HalfBlocks(Vec<ratatui::text::Line<'static>>),
    KittySeq(String),
}

pub struct AlbumDetail {
    pub album: Album,
    pub tracks: StatefulList<Track>,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    /// Cached render: (cols, rows) keyed to terminal cell dimensions.
    pub art_cache: std::cell::RefCell<Option<(u16, u16, ArtPayload)>>,
}

// ── View stack ────────────────────────────────────────────────────────────────

pub enum View {
    ArtistDetail(ArtistDetail),
    PlaylistDetail(PlaylistDetail),
    AlbumDetail(AlbumDetail),
}

// ── Search state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchPane {
    Tracks,
    Artists,
    Playlists,
}

pub struct SearchState {
    pub active: bool,
    pub query: String,
    pub tracks: Vec<Track>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
    pub pane: SearchPane,
    pub track_sel: usize,
    pub artist_sel: usize,
    pub playlist_sel: usize,
    pub loading: bool,
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            active: false,
            query: String::new(),
            tracks: Vec::new(),
            artists: Vec::new(),
            playlists: Vec::new(),
            pane: SearchPane::Tracks,
            track_sel: 0,
            artist_sel: 0,
            playlist_sel: 0,
            loading: false,
        }
    }
}

impl SearchState {
    pub fn total_results(&self) -> usize {
        self.tracks.len() + self.artists.len() + self.playlists.len()
    }

    pub fn pane_next(&mut self) {
        let len = self.pane_len();
        if len == 0 { return; }
        match self.pane {
            SearchPane::Tracks   => self.track_sel = (self.track_sel + 1).min(len - 1),
            SearchPane::Artists  => self.artist_sel = (self.artist_sel + 1).min(len - 1),
            SearchPane::Playlists => self.playlist_sel = (self.playlist_sel + 1).min(len - 1),
        }
    }

    pub fn pane_prev(&mut self) {
        match self.pane {
            SearchPane::Tracks    => { if self.track_sel > 0 { self.track_sel -= 1; } }
            SearchPane::Artists   => { if self.artist_sel > 0 { self.artist_sel -= 1; } }
            SearchPane::Playlists => { if self.playlist_sel > 0 { self.playlist_sel -= 1; } }
        }
    }

    pub fn pane_len(&self) -> usize {
        match self.pane {
            SearchPane::Tracks    => self.tracks.len(),
            SearchPane::Artists   => self.artists.len(),
            SearchPane::Playlists => self.playlists.len(),
        }
    }

    pub fn next_pane(&mut self) {
        self.pane = match self.pane {
            SearchPane::Tracks    => SearchPane::Artists,
            SearchPane::Artists   => SearchPane::Playlists,
            SearchPane::Playlists => SearchPane::Tracks,
        };
    }

    pub fn prev_pane(&mut self) {
        self.pane = match self.pane {
            SearchPane::Tracks    => SearchPane::Playlists,
            SearchPane::Artists   => SearchPane::Tracks,
            SearchPane::Playlists => SearchPane::Artists,
        };
    }
}

// ── Command palette ───────────────────────────────────────────────────────────

pub struct CommandState {
    pub active: bool,
    pub input: String,
    pub selected: usize,
}

impl Default for CommandState {
    fn default() -> Self {
        Self { active: false, input: String::new(), selected: 0 }
    }
}

impl CommandState {
    pub const COMMANDS: &'static [&'static str] =
        &["favorites", "artists", "albums", "playlists", "search"];

    pub fn matches(&self) -> Vec<&'static str> {
        let q = self.input.to_lowercase();
        Self::COMMANDS.iter()
            .filter(|&&c| c.starts_with(q.as_str()))
            .copied()
            .collect()
    }
}

// ── Now playing ───────────────────────────────────────────────────────────────

pub struct NowPlaying {
    pub track: Option<Track>,
    /// True only after mpv fires TrackStarted; false on startup and after the queue empties.
    pub active: bool,
    pub paused: bool,
    pub position: f64,
    pub duration: f64,
    pub queue: Vec<Track>,
    pub queue_index: usize,
    pub art_bytes: Option<Vec<u8>>,
    pub art_loading: bool,
    pub art_cache: std::cell::RefCell<Option<(u16, u16, ArtPayload)>>,
    pub lyrics_synced: Vec<(f64, String)>,
    pub lyrics_plain: Vec<String>,
    pub lyrics_loading: bool,
    pub sample_rate: Option<u32>,
    pub codec: Option<String>,
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self {
            track: None,
            active: false,
            paused: true,
            position: 0.0,
            duration: 0.0,
            queue: Vec::new(),
            queue_index: 0,
            art_bytes: None,
            art_loading: false,
            art_cache: std::cell::RefCell::new(None),
            lyrics_synced: Vec::new(),
            lyrics_plain: Vec::new(),
            lyrics_loading: false,
            sample_rate: None,
            codec: None,
        }
    }
}

impl NowPlaying {
    pub fn progress_ratio(&self) -> f64 {
        if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn position_display(&self) -> String {
        fmt_secs(self.position as u32)
    }

    pub fn duration_display(&self) -> String {
        fmt_secs(self.duration as u32)
    }
}

fn fmt_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

// ── Status message ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Error,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub should_quit: bool,
    pub current_tab: Tab,
    pub view_stack: Vec<View>,

    pub artists: StatefulList<Artist>,
    pub fav_albums: StatefulList<Album>,
    pub playlists: StatefulList<Playlist>,
    pub favorites: StatefulList<Track>,
    pub search: SearchState,
    pub command: CommandState,
    pub now_playing: NowPlaying,

    pub queue_focused: bool,
    pub queue_cursor: usize,

    pub tick: u64,
    /// (message, level, tick when set) — cleared automatically after ~5 s
    pub status: Option<(String, StatusLevel, u64)>,

    pub api_tx: mpsc::UnboundedSender<ApiRequest>,
    pub player_tx: mpsc::UnboundedSender<PlayerCmd>,
    pub mpris_tx: watch::Sender<MprisState>,
}

impl App {
    pub fn new(
        api_tx: mpsc::UnboundedSender<ApiRequest>,
        player_tx: mpsc::UnboundedSender<PlayerCmd>,
        mpris_tx: watch::Sender<MprisState>,
    ) -> Self {
        let mut app = Self {
            should_quit: false,
            current_tab: Tab::Favorites,
            view_stack: Vec::new(),
            artists: StatefulList::default(),
            fav_albums: StatefulList::default(),
            playlists: StatefulList::default(),
            favorites: StatefulList::default(),
            search: SearchState::default(),
            command: CommandState::default(),
            now_playing: NowPlaying::default(),
            queue_focused: false,
            queue_cursor: 0,
            tick: 0,
            status: None,
            api_tx,
            player_tx,
            mpris_tx,
        };
        // Kick off initial data loads
        app.load_artists();
        app.load_fav_albums();
        app.load_playlists();
        app.load_favorites();
        app
    }

    // ── Data loading helpers ──────────────────────────────────────────────────

    pub fn load_artists(&mut self) {
        if self.artists.loading || self.artists.exhausted {
            return;
        }
        self.artists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadArtists {
            offset: self.artists.next_offset,
        });
    }

    pub fn load_fav_albums(&mut self) {
        if self.fav_albums.loading || self.fav_albums.exhausted {
            return;
        }
        self.fav_albums.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadFavAlbums {
            offset: self.fav_albums.next_offset,
        });
    }

    pub fn load_playlists(&mut self) {
        if self.playlists.loading || self.playlists.exhausted {
            return;
        }
        self.playlists.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadPlaylists {
            offset: self.playlists.next_offset,
        });
    }

    pub fn load_favorites(&mut self) {
        if self.favorites.loading || self.favorites.exhausted {
            return;
        }
        self.favorites.loading = true;
        let _ = self.api_tx.send(ApiRequest::LoadFavorites {
            offset: self.favorites.next_offset,
        });
    }

    // ── API response handler ──────────────────────────────────────────────────

    pub fn handle_api_response(&mut self, resp: ApiResponse) {
        match resp {
            ApiResponse::Artists(items, total) => {
                self.artists.append(items, total);
                self.artists.items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }

            ApiResponse::FavAlbums(items, total) => {
                let existing_ids: std::collections::HashSet<u64> =
                    self.fav_albums.items.iter().map(|a| a.id).collect();
                let unique: Vec<Album> = items.into_iter()
                    .filter(|a| !existing_ids.contains(&a.id))
                    .collect();
                self.fav_albums.append(unique, total);
                self.load_fav_albums();
            }

            ApiResponse::AlbumFavorited => {}

            ApiResponse::AlbumUnfavorited { album_id } => {
                self.fav_albums.items.retain(|a| a.id != album_id);
                self.fav_albums.total = self.fav_albums.total.saturating_sub(1);
                self.fav_albums.selected = self.fav_albums.selected
                    .min(self.fav_albums.items.len().saturating_sub(1));
            }

            ApiResponse::Playlists(items, total) => {
                self.playlists.append(items, total);
            }

            ApiResponse::Favorites(items, total) => {
                let was_empty = self.favorites.items.is_empty();
                // Deduplicate: skip any track ID already in the list.
                let existing_ids: std::collections::HashSet<u64> =
                    self.favorites.items.iter().map(|t| t.id).collect();
                let unique: Vec<Track> = items.into_iter()
                    .filter(|t| !existing_ids.contains(&t.id))
                    .collect();
                self.favorites.append(unique, total);
                // On first load, preview the first track: show its art in the sidebar
                // without starting playback.
                if was_empty && self.now_playing.track.is_none() {
                    if let Some(first) = self.favorites.items.first().cloned() {
                        self.now_playing.track = Some(first);
                        self.fetch_now_playing_metadata();
                    }
                }
                // Eagerly fetch remaining pages so the full list is available without scrolling.
                self.load_favorites();
            }

            ApiResponse::ArtistTopTracks { artist_id, tracks } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let n = tracks.len() as u32;
                        let total = detail.tracks.total.max(n);
                        detail.tracks.append(tracks, total);
                        detail.tracks.exhausted = true;
                    }
                }
            }

            ApiResponse::ArtistAlbums { artist_id, albums } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        let n = albums.len() as u32;
                        let total = detail.albums.total.max(n);
                        detail.albums.append(albums, total);
                        detail.albums.items.sort_by(|a, b| {
                            b.release_date.as_deref().cmp(&a.release_date.as_deref())
                        });
                        detail.albums.exhausted = true;
                    }
                }
            }

            ApiResponse::AlbumLoaded { album } => {
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album.id {
                        detail.album = album;
                    }
                }
            }

            ApiResponse::AlbumTracks { album_id, tracks } => {
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album_id {
                        let n = tracks.len() as u32;
                        detail.tracks.append(tracks, n);
                        detail.tracks.exhausted = true;
                    }
                }
            }

            ApiResponse::AlbumArt { album_id, image_data } => {
                let is_now_playing = self.now_playing.track.as_ref()
                    .map(|t| t.album.id) == Some(album_id);
                if is_now_playing {
                    self.now_playing.art_bytes = Some(image_data.clone());
                    self.now_playing.art_loading = false;
                    *self.now_playing.art_cache.borrow_mut() = None;
                }
                if let Some(View::AlbumDetail(detail)) = self.view_stack.last_mut() {
                    if detail.album.id == album_id {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                        *detail.art_cache.borrow_mut() = None;
                    }
                }
            }

            ApiResponse::ArtistArt { artist_id, image_data } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        detail.art_bytes = Some(image_data);
                        detail.art_loading = false;
                        *detail.art_cache.borrow_mut() = None;
                    }
                }
            }

            ApiResponse::ArtistBio { artist_id, text } => {
                if let Some(View::ArtistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.artist.id == artist_id {
                        detail.bio = if text.is_empty() { None } else { Some(text) };
                        detail.bio_loading = false;
                    }
                }
            }

            ApiResponse::PlaylistTracks { uuid, tracks, total } => {
                if let Some(View::PlaylistDetail(detail)) = self.view_stack.last_mut() {
                    if detail.playlist.uuid == uuid {
                        detail.tracks.append(tracks, total);
                    }
                }
            }

            ApiResponse::SearchResults(results) => {
                self.search.loading = false;
                self.search.tracks = results.tracks.map(|p| p.items).unwrap_or_default();
                self.search.artists = results.artists.map(|p| p.items).unwrap_or_default();
                self.search.playlists = results.playlists.map(|p| p.items).unwrap_or_default();
                self.search.track_sel = 0;
                self.search.artist_sel = 0;
                self.search.playlist_sel = 0;
                self.search.pane = SearchPane::Tracks;
            }

            ApiResponse::StreamUrl { track_id, url } => {
                let idx = self.now_playing.queue_index;
                if self.now_playing.queue.get(idx).map(|t| t.id) == Some(track_id) {
                    // Current track — play it, then immediately pre-fetch the next one
                    // so mpv can append it and advance gaplessly.
                    let _ = self.player_tx.send(PlayerCmd::Play(url));
                    if let Some(next) = self.now_playing.queue.get(idx + 1) {
                        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
                    }
                } else if self.now_playing.queue.get(idx + 1).map(|t| t.id) == Some(track_id) {
                    // Pre-fetched next track — append to mpv's playlist.
                    let _ = self.player_tx.send(PlayerCmd::Append(url));
                }
            }

            ApiResponse::Lyrics { track_id, synced, plain } => {
                if self.now_playing.track.as_ref().map(|t| t.id) == Some(track_id) {
                    self.now_playing.lyrics_synced = synced;
                    self.now_playing.lyrics_plain = plain;
                    self.now_playing.lyrics_loading = false;
                }
            }

            ApiResponse::FavoriteAdded | ApiResponse::ArtistFollowed => {}

            ApiResponse::FavoriteRemoved { track_id } => {
                self.favorites.items.retain(|t| t.id != track_id);
                self.favorites.total = self.favorites.total.saturating_sub(1);
                self.favorites.selected = self.favorites.selected.min(
                    self.favorites.items.len().saturating_sub(1)
                );
            }

            ApiResponse::ArtistUnfollowed { artist_id } => {
                self.artists.items.retain(|a| a.id != artist_id);
                self.artists.total = self.artists.total.saturating_sub(1);
                self.artists.selected = self.artists.selected.min(
                    self.artists.items.len().saturating_sub(1)
                );
            }

            ApiResponse::RadioTracks { tracks } => {
                if tracks.is_empty() {
                    self.set_status("No radio tracks available".to_string(), StatusLevel::Error);
                } else {
                    self.play_tracks(tracks, 0);
                }
            }

            ApiResponse::Error(msg) => {
                self.set_status(msg, StatusLevel::Error);
            }
        }
    }

    // ── Player event handler ──────────────────────────────────────────────────

    pub fn handle_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackStarted => {
                self.now_playing.active = true;
                self.now_playing.paused = false;
                self.now_playing.sample_rate = None;
                self.now_playing.codec = None;
                if let Some(track) = &self.now_playing.track {
                    let title = format!("{} — {}", track.artist_name(), track.title);
                    let _ = self.player_tx.send(PlayerCmd::SetMediaTitle(title));
                }
                self.push_mpris_state();
            }
            PlayerEvent::TrackEnded => {
                // mpv has already auto-advanced to the appended next track in its
                // playlist. Update our index to match, then pre-fetch the one after
                // so mpv can keep the chain going.
                if self.now_playing.queue_index + 1 < self.now_playing.queue.len() {
                    self.now_playing.queue_index += 1;
                    self.now_playing.track =
                        self.now_playing.queue.get(self.now_playing.queue_index).cloned();
                    let next_idx = self.now_playing.queue_index + 1;
                    if let Some(next) = self.now_playing.queue.get(next_idx) {
                        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
                    }
                    self.fetch_now_playing_metadata();
                } else {
                    self.now_playing.active = false;
                    self.push_mpris_state();
                }
                self.now_playing.position = 0.0;
            }
            PlayerEvent::Position(p) => {
                self.now_playing.position = p;
            }
            PlayerEvent::Duration(d) => {
                self.now_playing.duration = d;
            }
            PlayerEvent::Paused(p) => {
                self.now_playing.paused = p;
                self.push_mpris_state();
            }
            PlayerEvent::SampleRate(r) => {
                self.now_playing.sample_rate = Some(r);
            }
            PlayerEvent::Codec(c) => {
                self.now_playing.codec = Some(c);
            }
            PlayerEvent::Error(e) => {
                self.set_status(format!("Player: {e}"), StatusLevel::Error);
            }
        }
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    pub fn play_track(&mut self, track: Track) {
        let id = track.id;
        self.now_playing.queue = vec![track.clone()];
        self.now_playing.queue_index = 0;
        self.now_playing.track = Some(track);
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
    }

    pub fn play_tracks(&mut self, tracks: Vec<Track>, start_index: usize) {
        if tracks.is_empty() {
            return;
        }
        let track_id = tracks.get(start_index).map(|t| t.id);
        self.now_playing.track = tracks.get(start_index).cloned();
        self.now_playing.queue = tracks;
        self.now_playing.queue_index = start_index;
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        if let Some(id) = track_id {
            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
        }
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
    }

    fn favorite_track(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::FavoriteTrack { track_id: track.id });
        // Optimistically insert at top (list is newest-first); skip if already present.
        if !self.favorites.items.iter().any(|t| t.id == track.id) {
            self.favorites.items.insert(0, track.clone());
            self.favorites.total = self.favorites.total.saturating_add(1);
            self.favorites.selected = self.favorites.selected.saturating_add(1);
        }
        self.set_status(format!("Added '{}' to favorites", track.title), StatusLevel::Info);
    }

    fn follow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::FollowArtist { artist_id: artist.id });
        if !self.artists.items.iter().any(|a| a.id == artist.id) {
            let pos = self.artists.items.partition_point(|a| a.name.to_lowercase() < artist.name.to_lowercase());
            self.artists.items.insert(pos, artist.clone());
            self.artists.total = self.artists.total.saturating_add(1);
            if pos <= self.artists.selected {
                self.artists.selected = self.artists.selected.saturating_add(1);
            }
        }
        self.set_status(format!("Following {}", artist.name), StatusLevel::Info);
    }

    pub fn toggle_favorite_track(&mut self, track: &Track) {
        if self.favorites.items.iter().any(|t| t.id == track.id) {
            self.unfavorite_track(track);
        } else {
            self.favorite_track(track);
        }
    }

    pub fn toggle_follow_artist(&mut self, artist: &Artist) {
        if self.artists.items.iter().any(|a| a.id == artist.id) {
            self.unfollow_artist(artist);
        } else {
            self.follow_artist(artist);
        }
    }

    fn favorite_album(&mut self, album: &Album) {
        let _ = self.api_tx.send(ApiRequest::FavoriteAlbum { album_id: album.id });
        if !self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.fav_albums.items.insert(0, album.clone());
            self.fav_albums.total = self.fav_albums.total.saturating_add(1);
            self.fav_albums.selected = self.fav_albums.selected.saturating_add(1);
        }
        self.set_status(format!("Added '{}' to albums", album.title), StatusLevel::Info);
    }

    fn unfavorite_album(&mut self, album: &Album) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteAlbum { album_id: album.id });
        self.set_status(format!("Removed '{}' from albums", album.title), StatusLevel::Info);
    }

    pub fn toggle_favorite_album(&mut self, album: &Album) {
        if self.fav_albums.items.iter().any(|a| a.id == album.id) {
            self.unfavorite_album(album);
        } else {
            self.favorite_album(album);
        }
    }

    fn unfavorite_track(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::UnfavoriteTrack { track_id: track.id });
        self.set_status(format!("Removed '{}' from favorites", track.title), StatusLevel::Info);
    }

    fn unfollow_artist(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::UnfollowArtist { artist_id: artist.id });
        self.set_status(format!("Unfollowed {}", artist.name), StatusLevel::Info);
    }

    pub fn start_track_radio(&mut self, track: &Track) {
        let _ = self.api_tx.send(ApiRequest::TrackRadio { track_id: track.id });
        self.set_status(format!("Loading radio for '{}'…", track.title), StatusLevel::Info);
    }

    pub fn start_artist_radio(&mut self, artist: &Artist) {
        let _ = self.api_tx.send(ApiRequest::ArtistRadio { artist_id: artist.id });
        self.set_status(format!("Loading radio for {}…", artist.name), StatusLevel::Info);
    }

    pub fn add_to_queue(&mut self, track: Track) {
        if self.now_playing.track.is_none() {
            self.play_track(track);
            return;
        }
        let title = track.title.clone();
        self.now_playing.queue.push(track);
        // If this track is immediately after the current one, pre-fetch for gapless.
        let qi = self.now_playing.queue_index;
        let new_idx = self.now_playing.queue.len() - 1;
        if new_idx == qi + 1 {
            let id = self.now_playing.queue[new_idx].id;
            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: id });
        }
        self.set_status(format!("Queued: {title}"), StatusLevel::Info);
    }

    pub fn focus_queue(&mut self) {
        if self.now_playing.queue.is_empty() {
            return;
        }
        self.queue_focused = true;
        self.queue_cursor = self.now_playing.queue_index;
    }

    pub fn unfocus_queue(&mut self) {
        self.queue_focused = false;
    }

    pub fn play_from_queue(&mut self, idx: usize) {
        if idx >= self.now_playing.queue.len() {
            return;
        }
        self.now_playing.queue_index = idx;
        self.now_playing.track = self.now_playing.queue.get(idx).cloned();
        self.now_playing.active = false;
        self.now_playing.position = 0.0;
        if let Some(track) = self.now_playing.queue.get(idx) {
            let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
        }
        self.fetch_now_playing_metadata();
        self.push_mpris_state();
        self.queue_focused = false;
    }

    pub fn remove_from_queue(&mut self, idx: usize) {
        let len = self.now_playing.queue.len();
        if idx >= len {
            return;
        }
        let qi = self.now_playing.queue_index;

        if idx == qi {
            self.now_playing.queue.remove(idx);
            if self.now_playing.queue.is_empty() {
                self.now_playing.track = None;
                self.now_playing.active = false;
                self.now_playing.queue_index = 0;
                let _ = self.player_tx.send(PlayerCmd::Stop);
                self.push_mpris_state();
                self.queue_focused = false;
                return;
            }
            let new_idx = idx.min(self.now_playing.queue.len() - 1);
            self.now_playing.queue_index = new_idx;
            self.now_playing.track = self.now_playing.queue.get(new_idx).cloned();
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(new_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            self.fetch_now_playing_metadata();
        } else if idx == qi + 1 {
            self.now_playing.queue.remove(idx);
            let _ = self.player_tx.send(PlayerCmd::RemoveNext);
            if let Some(next) = self.now_playing.queue.get(qi + 1) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: next.id });
            }
        } else {
            self.now_playing.queue.remove(idx);
            if idx < qi {
                self.now_playing.queue_index -= 1;
            }
        }

        if self.queue_cursor >= self.now_playing.queue.len() && !self.now_playing.queue.is_empty() {
            self.queue_cursor = self.now_playing.queue.len() - 1;
        }
        if self.now_playing.queue.is_empty() {
            self.queue_focused = false;
        }
    }

    pub fn toggle_pause(&mut self) {
        let _ = self.player_tx.send(PlayerCmd::TogglePause);
    }

    pub fn next_track(&mut self) {
        let next_idx = self.now_playing.queue_index + 1;
        if next_idx < self.now_playing.queue.len() {
            self.now_playing.queue_index = next_idx;
            self.now_playing.track = self.now_playing.queue.get(next_idx).cloned();
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(next_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            self.fetch_now_playing_metadata();
            self.push_mpris_state();
        }
    }

    pub fn prev_track(&mut self) {
        if self.now_playing.queue_index > 0 {
            let prev_idx = self.now_playing.queue_index - 1;
            self.now_playing.queue_index = prev_idx;
            self.now_playing.track = self.now_playing.queue.get(prev_idx).cloned();
            self.now_playing.active = false;
            self.now_playing.position = 0.0;
            if let Some(track) = self.now_playing.queue.get(prev_idx) {
                let _ = self.api_tx.send(ApiRequest::ResolveStreamUrl { track_id: track.id });
            }
            self.fetch_now_playing_metadata();
            self.push_mpris_state();
        }
    }

    pub fn push_mpris_state(&self) {
        let state = match &self.now_playing.track {
            Some(t) => MprisState {
                title: t.title.clone(),
                artist: t.artist_name().to_owned(),
                album: t.album.title.clone(),
                art_url: t.album.cover.as_deref()
                    .map(|id| format!(
                        "https://resources.tidal.com/images/{}/320x320.jpg",
                        id.replace('-', "/")
                    ))
                    .unwrap_or_default(),
                duration_us: t.duration as i64 * 1_000_000,
                paused: self.now_playing.paused,
                active: self.now_playing.active,
            },
            None => MprisState::default(),
        };
        let _ = self.mpris_tx.send(state);
    }

    fn fetch_now_playing_metadata(&mut self) {
        self.fetch_now_playing_art();
        self.fetch_lyrics();
    }

    fn fetch_now_playing_art(&mut self) {
        let (album_id, cover_id) = match &self.now_playing.track {
            Some(t) => (t.album.id, t.album.cover.clone()),
            None => return,
        };
        self.now_playing.art_bytes = None;
        *self.now_playing.art_cache.borrow_mut() = None;
        if let Some(cover_id) = cover_id {
            self.now_playing.art_loading = true;
            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        } else {
            self.now_playing.art_loading = false;
        }
    }

    fn fetch_lyrics(&mut self) {
        let Some(track) = &self.now_playing.track else { return };
        let track_id = track.id;
        self.now_playing.lyrics_synced = Vec::new();
        self.now_playing.lyrics_plain = Vec::new();
        self.now_playing.lyrics_loading = true;
        let _ = self.api_tx.send(ApiRequest::FetchLyrics { track_id });
    }

    // ── Tab switching ─────────────────────────────────────────────────────────

    pub fn next_tab(&mut self) {
        self.current_tab = match self.current_tab {
            Tab::Favorites => Tab::Artists,
            Tab::Artists => Tab::Albums,
            Tab::Albums => Tab::Playlists,
            Tab::Playlists => Tab::Search,
            Tab::Search => Tab::Favorites,
        };
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.active = true;
            self.search.query.clear();
        }
    }

    pub fn set_tab(&mut self, tab: Tab) {
        self.current_tab = tab;
        self.view_stack.clear();
        if self.current_tab == Tab::Search {
            self.search.active = true;
            self.search.query.clear();
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    pub fn go_back(&mut self) {
        if self.search.active {
            self.search.active = false;
            return;
        }
        if !self.view_stack.is_empty() {
            self.view_stack.pop();
        }
    }

    pub fn open_selected_artist(&mut self) {
        let Some(artist) = self.artists.selected_item().cloned() else {
            return;
        };
        self.open_artist(artist);
    }

    pub fn open_album(&mut self, album: Album) {
        let album_id = album.id;
        let cover = album.cover.clone();
        let has_cover = cover.is_some();
        self.view_stack.push(View::AlbumDetail(AlbumDetail {
            album,
            tracks: StatefulList::default(),
            art_bytes: None,
            art_loading: has_cover,
            art_cache: std::cell::RefCell::new(None),
        }));
        let _ = self.api_tx.send(ApiRequest::LoadAlbum { album_id });
        let _ = self.api_tx.send(ApiRequest::LoadAlbumTracks { album_id });
        if let Some(cover_id) = cover {
            let _ = self.api_tx.send(ApiRequest::FetchAlbumArt { album_id, cover_id });
        }
    }

    pub fn open_selected_album(&mut self) {
        let album = if let Some(View::ArtistDetail(detail)) = self.view_stack.last() {
            detail.albums.selected_item().cloned()
        } else {
            None
        };
        if let Some(album) = album {
            self.open_album(album);
        }
    }

    pub fn open_selected_fav_album(&mut self) {
        if let Some(album) = self.fav_albums.selected_item().cloned() {
            self.open_album(album);
        }
    }

    pub fn open_selected_playlist(&mut self) {
        let Some(playlist) = self.playlists.selected_item().cloned() else {
            return;
        };
        self.open_playlist(playlist);
    }

    pub fn open_artist(&mut self, artist: Artist) {
        let id = artist.id;
        let picture_id = artist.picture.clone();
        let has_picture = picture_id.is_some();
        let detail = ArtistDetail {
            artist,
            tracks: StatefulList::default(),
            albums: StatefulList::default(),
            focus: ArtistDetailFocus::Tracks,
            art_bytes: None,
            art_loading: has_picture,
            art_cache: std::cell::RefCell::new(None),
            bio: None,
            bio_loading: true,
            bio_scroll: 0,
        };
        self.view_stack.push(View::ArtistDetail(detail));
        let _ = self.api_tx.send(ApiRequest::LoadArtistTopTracks { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistAlbums { artist_id: id });
        let _ = self.api_tx.send(ApiRequest::LoadArtistBio { artist_id: id });
        if let Some(picture_id) = picture_id {
            let _ = self.api_tx.send(ApiRequest::FetchArtistArt { artist_id: id, picture_id });
        }
    }

    pub fn open_playlist(&mut self, playlist: Playlist) {
        let uuid = playlist.uuid.clone();
        let detail = PlaylistDetail {
            playlist,
            tracks: StatefulList::default(),
        };
        self.view_stack.push(View::PlaylistDetail(detail));
        let _ = self.api_tx.send(ApiRequest::LoadPlaylistTracks { uuid, offset: 0 });
    }

    // ── Tick ──────────────────────────────────────────────────────────────────

    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        // ~5 s at 16 ms/tick = 312 ticks
        if let Some((_, _, set_at)) = self.status {
            if self.tick.wrapping_sub(set_at) > 312 {
                self.status = None;
            }
        }
    }

    fn set_status(&mut self, msg: String, level: StatusLevel) {
        self.status = Some((msg, level, self.tick));
    }
}
