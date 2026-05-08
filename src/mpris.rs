// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use zbus::{connection, interface};
use zbus::zvariant::{Array, ObjectPath, OwnedValue, Signature, Str, Value};

const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const BUS_NAME: &str = "org.mpris.MediaPlayer2.typhoon";

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct MprisState {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub duration_us: i64,
    pub paused: bool,
    pub active: bool,
}

// ── Commands from MPRIS clients back to the app ───────────────────────────────

pub enum MprisCmd {
    Next,
    Previous,
    PlayPause,
    Play,
    Pause,
    Stop,
}

// ── D-Bus interfaces ──────────────────────────────────────────────────────────

struct RootIface;

#[interface(name = "org.mpris.MediaPlayer2")]
impl RootIface {
    fn raise(&self) {}
    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool { false }

    #[zbus(property)]
    fn can_raise(&self) -> bool { false }

    #[zbus(property)]
    fn has_track_list(&self) -> bool { false }

    #[zbus(property)]
    fn identity(&self) -> &str { "Typhoon" }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> { vec![] }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> { vec![] }
}

struct PlayerIface {
    state: Arc<Mutex<MprisState>>,
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerIface {
    async fn next(&self) { let _ = self.cmd_tx.send(MprisCmd::Next); }
    async fn previous(&self) { let _ = self.cmd_tx.send(MprisCmd::Previous); }
    async fn pause(&self) { let _ = self.cmd_tx.send(MprisCmd::Pause); }
    async fn play_pause(&self) { let _ = self.cmd_tx.send(MprisCmd::PlayPause); }
    async fn stop(&self) { let _ = self.cmd_tx.send(MprisCmd::Stop); }
    async fn play(&self) { let _ = self.cmd_tx.send(MprisCmd::Play); }
    async fn seek(&self, _offset: i64) {}
    async fn set_position(&self, _track_id: zbus::zvariant::ObjectPath<'_>, _position: i64) {}
    async fn open_uri(&self, _uri: String) {}

    #[zbus(property)]
    fn playback_status(&self) -> String {
        let s = self.state.lock().unwrap();
        if !s.active { "Stopped".into() }
        else if s.paused { "Paused".into() }
        else { "Playing".into() }
    }

    #[zbus(property)]
    fn loop_status(&self) -> String { "None".into() }

    #[zbus(property)]
    fn rate(&self) -> f64 { 1.0 }

    #[zbus(property)]
    fn shuffle(&self) -> bool { false }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        build_metadata(&self.state.lock().unwrap())
    }

    #[zbus(property)]
    fn volume(&self) -> f64 { 1.0 }

    #[zbus(property)]
    fn position(&self) -> i64 { 0 }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 { 1.0 }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 { 1.0 }

    #[zbus(property)]
    fn can_go_next(&self) -> bool { true }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool { true }

    #[zbus(property)]
    fn can_play(&self) -> bool { true }

    #[zbus(property)]
    fn can_pause(&self) -> bool { true }

    #[zbus(property)]
    fn can_seek(&self) -> bool { false }

    #[zbus(property)]
    fn can_control(&self) -> bool { true }
}

fn owned_str(s: &str) -> OwnedValue {
    OwnedValue::try_from(Value::Str(Str::from(s.to_owned()))).unwrap()
}

fn str_array_value(items: &[&str]) -> OwnedValue {
    let sig = Signature::try_from("s").unwrap();
    let mut arr = Array::new(sig);
    for &s in items {
        arr.append(Value::Str(Str::from(s.to_owned()))).ok();
    }
    OwnedValue::try_from(Value::Array(arr)).unwrap()
}

fn build_metadata(s: &MprisState) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();

    let path: ObjectPath<'static> = ObjectPath::try_from("/org/typhoon/track/current").unwrap();
    map.insert(
        "mpris:trackid".into(),
        OwnedValue::try_from(Value::ObjectPath(path)).unwrap(),
    );

    if !s.title.is_empty() {
        map.insert("xesam:title".into(), owned_str(&s.title));
    }
    if !s.artist.is_empty() {
        map.insert("xesam:artist".into(), str_array_value(&[s.artist.as_str()]));
    }
    if !s.album.is_empty() {
        map.insert("xesam:album".into(), owned_str(&s.album));
    }
    if !s.art_url.is_empty() {
        map.insert("mpris:artUrl".into(), owned_str(&s.art_url));
    }
    if s.duration_us > 0 {
        map.insert("mpris:length".into(), OwnedValue::from(s.duration_us));
    }

    map
}

// ── Server ────────────────────────────────────────────────────────────────────

pub struct MprisServer {
    state_rx: watch::Receiver<MprisState>,
    cmd_tx: mpsc::UnboundedSender<MprisCmd>,
}

impl MprisServer {
    pub fn new(
        state_rx: watch::Receiver<MprisState>,
        cmd_tx: mpsc::UnboundedSender<MprisCmd>,
    ) -> Self {
        Self { state_rx, cmd_tx }
    }

    pub async fn run(mut self) {
        let shared: Arc<Mutex<MprisState>> = Arc::new(Mutex::new(MprisState::default()));

        let conn = match connection::Builder::session()
            .and_then(|b| b.name(BUS_NAME))
            .and_then(|b| b.serve_at(OBJECT_PATH, RootIface))
            .and_then(|b| b.serve_at(OBJECT_PATH, PlayerIface {
                state: Arc::clone(&shared),
                cmd_tx: self.cmd_tx,
            })) {
            Ok(builder) => match builder.build().await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("MPRIS: failed to connect to D-Bus: {e}");
                    return;
                }
            },
            Err(e) => {
                eprintln!("MPRIS: D-Bus setup failed: {e}");
                return;
            }
        };

        loop {
            if self.state_rx.changed().await.is_err() {
                break;
            }
            *shared.lock().unwrap() = self.state_rx.borrow_and_update().clone();

            if let Ok(iface_ref) = conn
                .object_server()
                .interface::<_, PlayerIface>(OBJECT_PATH)
                .await
            {
                let guard = iface_ref.get().await;
                let ctx = iface_ref.signal_context();
                let _ = PlayerIface::playback_status_changed(&*guard, ctx).await;
                let _ = PlayerIface::metadata_changed(&*guard, ctx).await;
            }
        }
    }
}
