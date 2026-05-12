// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2025 Ryan Cohan

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const PORT: u16 = 16_384;

pub async fn run_server() {
    let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{PORT}")).await else {
        return;
    };
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { continue };
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract path from "GET /438882313.m3u8 HTTP/1.1"
            let filename = request
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|p| p.strip_prefix('/'))
                .filter(|f| f.ends_with(".m3u8") && f[..f.len() - 5].chars().all(|c| c.is_ascii_digit()));

            let response = match filename.and_then(|f| {
                std::fs::read_to_string(format!("/tmp/riptide_hls_{f}")).ok()
            }) {
                Some(body) => format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: application/vnd.apple.mpegurl\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    body.len(),
                    body
                ),
                None => "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n".to_owned(),
            };

            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}
