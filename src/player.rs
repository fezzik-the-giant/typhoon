// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use anyhow::{bail, Result};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};

const SOCKET_PATH: &str = "/tmp/typhoon-mpv.sock";

#[derive(Debug)]
pub enum PlayerCmd {
    Play(String),
    Append(String),
    TogglePause,
    Stop,
    RemoveNext,
    SetVolume(u8),
}

#[derive(Debug)]
pub enum PlayerEvent {
    TrackStarted,
    TrackEnded,
    Position(f64),
    Duration(f64),
    Paused(bool),
    Volume(f64),
    Error(String),
}

pub struct PlayerWorker {
    cmd_rx: mpsc::UnboundedReceiver<PlayerCmd>,
    event_tx: mpsc::UnboundedSender<PlayerEvent>,
}

impl PlayerWorker {
    pub fn new(
        cmd_rx: mpsc::UnboundedReceiver<PlayerCmd>,
        event_tx: mpsc::UnboundedSender<PlayerEvent>,
    ) -> Self {
        Self { cmd_rx, event_tx }
    }

    pub async fn run(mut self) {
        // Start mpv and keep the child handle so we can kill it on exit.
        let mut child = match self.start_mpv().await {
            Ok(c) => c,
            Err(e) => {
                let _ = self.event_tx.send(PlayerEvent::Error(e.to_string()));
                return;
            }
        };

        let stream = match self.connect_socket().await {
            Ok(s) => s,
            Err(e) => {
                let _ = self.event_tx.send(PlayerEvent::Error(e.to_string()));
                return;
            }
        };

        let (read_half, write_half) = stream.into_split();

        // Channel for IPC write commands
        let (ipc_tx, ipc_rx) = mpsc::unbounded_channel::<IpcRequest>();

        // Spawn write task
        let write_task = tokio::spawn(write_loop(write_half, ipc_rx));

        // Spawn read task
        let event_tx_clone = self.event_tx.clone();
        let read_task = tokio::spawn(read_loop(read_half, event_tx_clone));

        // Ticker for position polling
        let mut poll_ticker = interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    let cmd = match cmd {
                        Some(c) => c,
                        None => break, // App dropped the sender — time to quit.
                    };
                    match cmd {
                        PlayerCmd::Play(url) => {
                            let _ = ipc_tx.send(IpcRequest::Write(
                                json!({"command": ["loadfile", url, "replace"]}).to_string()
                            ));
                            let _ = ipc_tx.send(IpcRequest::Write(
                                json!({"command": ["set_property", "pause", false]}).to_string()
                            ));
                        }
                        other => {
                            let msg = match other {
                                PlayerCmd::Append(url) => json!({"command": ["loadfile", url, "append"]}),
                                PlayerCmd::TogglePause  => json!({"command": ["cycle", "pause"]}),
                                PlayerCmd::Stop         => json!({"command": ["stop"]}),
                                PlayerCmd::RemoveNext   => json!({"command": ["playlist-remove", 1]}),
                                PlayerCmd::SetVolume(v) => json!({"command": ["set_property", "volume", v]}),
                                PlayerCmd::Play(_)      => unreachable!(),
                            };
                            let _ = ipc_tx.send(IpcRequest::Write(msg.to_string()));
                        }
                    }
                }

                _ = poll_ticker.tick() => {
                    let _ = ipc_tx.send(IpcRequest::Write(
                        json!({"command": ["get_property", "time-pos"], "request_id": 1}).to_string()
                    ));
                    let _ = ipc_tx.send(IpcRequest::Write(
                        json!({"command": ["get_property", "duration"], "request_id": 2}).to_string()
                    ));
                    let _ = ipc_tx.send(IpcRequest::Write(
                        json!({"command": ["get_property", "pause"], "request_id": 3}).to_string()
                    ));
                }
            }
        }

        // Clean up: kill mpv and remove the socket file.
        let _ = child.kill().await;
        let _ = std::fs::remove_file(SOCKET_PATH);
    }

    async fn start_mpv(&self) -> Result<tokio::process::Child> {
        if Path::new(SOCKET_PATH).exists() {
            let _ = std::fs::remove_file(SOCKET_PATH);
        }

        let child = Command::new("mpv")
            .args([
                "--no-video",
                "--idle=yes",
                &format!("--input-ipc-server={SOCKET_PATH}"),
                "--really-quiet",
                "--prefetch-playlist=yes",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        Ok(child)
    }

    async fn connect_socket(&self) -> Result<UnixStream> {
        for _ in 0..50 {
            if Path::new(SOCKET_PATH).exists() {
                match UnixStream::connect(SOCKET_PATH).await {
                    Ok(s) => return Ok(s),
                    Err(_) => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!("mpv IPC socket did not appear – is mpv installed?")
    }
}

// ── IPC write loop ────────────────────────────────────────────────────────────

enum IpcRequest {
    Write(String),
}

async fn write_loop(
    mut writer: tokio::net::unix::OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<IpcRequest>,
) {
    while let Some(req) = rx.recv().await {
        match req {
            IpcRequest::Write(msg) => {
                let line = format!("{msg}\n");
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        }
    }
}

// ── IPC read loop ─────────────────────────────────────────────────────────────

async fn read_loop(
    reader: tokio::net::unix::OwnedReadHalf,
    event_tx: mpsc::UnboundedSender<PlayerEvent>,
) {
    let mut buf = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        match buf.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let Ok(data) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        // Event messages
        if let Some(event) = data.get("event").and_then(|e| e.as_str()) {
            match event {
                "file-loaded" => {
                    let _ = event_tx.send(PlayerEvent::TrackStarted);
                }
                "end-file" => {
                    // Only advance on natural EOF; "stop" means we interrupted
                    // the file with a new loadfile command (user selected a track).
                    let reason = data.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                    if reason == "eof" {
                        let _ = event_tx.send(PlayerEvent::TrackEnded);
                    }
                }
                _ => {}
            }
            continue;
        }

        // Property response messages (request_id 1=pos, 2=dur, 3=pause)
        if let Some(req_id) = data.get("request_id").and_then(|v| v.as_u64()) {
            let value = &data["data"];
            match req_id {
                1 => {
                    if let Some(pos) = value.as_f64() {
                        let _ = event_tx.send(PlayerEvent::Position(pos));
                    }
                }
                2 => {
                    if let Some(dur) = value.as_f64() {
                        let _ = event_tx.send(PlayerEvent::Duration(dur));
                    }
                }
                3 => {
                    if let Some(paused) = value.as_bool() {
                        let _ = event_tx.send(PlayerEvent::Paused(paused));
                    }
                }
                _ => {}
            }
        }
    }
}
