use crate::{extension_manager::Definition, models::Server, network};
use anyhow::{bail, ensure, Context, Result};
use futures_util::StreamExt;
use serde_json::Value;
use std::{collections::BTreeMap, process::Stdio, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Direct,
    Hls,
}
#[derive(Clone, Debug)]
pub struct Media {
    pub url: String,
    pub referer: String,
    pub headers: BTreeMap<String, String>,
    pub kind: MediaKind,
}
pub fn installed(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|p| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p.join(name))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}
pub fn classify_path(path: &str) -> Option<MediaKind> {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".m3u8") || lower.ends_with(".m3u") {
        return Some(MediaKind::Hls);
    }
    if [
        ".mp4", ".mkv", ".webm", ".m4v", ".mov", ".ogv", ".avi", ".ts", ".m2ts",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
    {
        return Some(MediaKind::Direct);
    }
    None
}
pub fn classify_probe(content_type: &str, sample: &[u8]) -> Option<MediaKind> {
    let text = String::from_utf8_lossy(sample);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let lower_prefix = trimmed.to_ascii_lowercase();
    if lower_prefix.starts_with("#extm3u") {
        return Some(MediaKind::Hls);
    }
    if lower_prefix.starts_with('<') || lower_prefix.starts_with('{') {
        return None;
    }
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if matches!(
        mime.as_str(),
        "application/vnd.apple.mpegurl"
            | "application/x-mpegurl"
            | "audio/mpegurl"
            | "audio/x-mpegurl"
    ) {
        return Some(MediaKind::Hls);
    }
    if mime.starts_with("video/")
        || matches!(mime.as_str(), "application/ogg" | "application/x-matroska")
    {
        return Some(MediaKind::Direct);
    }
    None
}
async fn probe_open_media(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
) -> Option<(String, MediaKind)> {
    let mut req = client.get(url);
    if !referer.trim().is_empty() {
        req = req.header(reqwest::header::REFERER, referer);
    }
    let response = tokio::time::timeout(Duration::from_secs(10), req.send())
        .await
        .ok()?
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let final_url = network::web_url(response.url().as_str()).ok()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let mut stream = response.bytes_stream();
    let mut sample = Vec::with_capacity(512);
    while sample.len() < 512 {
        let next = tokio::time::timeout(Duration::from_secs(6), stream.next())
            .await
            .ok()?;
        let Some(Ok(chunk)) = next else {
            break;
        };
        let take = (512 - sample.len()).min(chunk.len());
        sample.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    if let Some(kind) = classify_probe(&content_type, &sample) {
        return Some((final_url.to_string(), kind));
    }
    let lower_ct = content_type.to_ascii_lowercase();
    let prefix = String::from_utf8_lossy(&sample);
    let trimmed = prefix.trim_start_matches('\u{feff}').trim_start();
    if (lower_ct.starts_with("application/octet-stream")
        || lower_ct.starts_with("binary/octet-stream"))
        && !trimmed.starts_with('<')
        && !trimmed.starts_with('{')
    {
        if let Some(kind) = classify_path(final_url.path()) {
            return Some((final_url.to_string(), kind));
        }
    }
    None
}
pub async fn resolve(
    client: &reqwest::Client,
    d: &Definition,
    server: &Server,
    referer: &str,
) -> Result<Media> {
    let url = network::web_url(&server.url)?;
    let default_referer = if referer.trim().is_empty() {
        format!("{}://{}/", url.scheme(), url.host_str().unwrap_or_default())
    } else {
        referer.into()
    };
    let mut media = Media {
        url: url.to_string(),
        referer: default_referer,
        headers: BTreeMap::new(),
        kind: MediaKind::Direct,
    };
    for rule in &d.resolvers {
        if url.host_str() != Some(rule.host.as_str()) {
            continue;
        }
        let re = regex::Regex::new(&rule.path_pattern)?;
        if !re.is_match(url.path()) {
            continue;
        }
        let mut target = url.clone();
        target.set_path(&re.replace(url.path(), rule.replacement.as_str()));
        target.set_query(None);
        target.set_fragment(None);
        media.url = target.to_string();
        media.referer = url.to_string();
        if rule.kind == "hls" {
            let data = network::bytes(client, &media.url, &media.referer, 1024 * 1024).await?;
            ensure!(
                String::from_utf8_lossy(&data)
                    .trim_start_matches('\u{feff}')
                    .trim_start()
                    .starts_with("#EXTM3U"),
                "El servidor no entregó un vídeo disponible"
            );
            media.kind = MediaKind::Hls;
        }
        return Ok(media);
    }
    if let Some(kind) = classify_path(url.path()) {
        media.kind = kind;
        return Ok(media);
    }
    if let Some((resolved_url, kind)) = probe_open_media(client, url.as_str(), &media.referer).await
    {
        media.url = resolved_url;
        media.kind = kind;
        return Ok(media);
    }
    ensure!(
        installed("yt-dlp"),
        "Este servidor necesita yt-dlp. Instálalo o elige otro servidor"
    );
    // No shell and no remote code, config files, playlists or credentials.
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args([
        "--ignore-config",
        "--no-playlist",
        "--no-warnings",
        "--no-call-home",
        "--socket-timeout",
        "20",
        "-f",
        "best",
        "--dump-single-json",
        "--referer",
        &media.referer,
        "--user-agent",
        network::USER_AGENT,
        "--",
        &server.url,
    ])
    .stdin(Stdio::null())
    .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(75), cmd.output())
        .await
        .context("El servidor tardó demasiado en responder")??;
    ensure!(
        output.status.success(),
        "No se pudo resolver este servidor; prueba con otro o actualiza yt-dlp"
    );
    ensure!(
        output.stdout.len() < 4 * 1024 * 1024,
        "Respuesta de servidor demasiado grande"
    );
    let data: Value = serde_json::from_slice(&output.stdout)?;
    let Some(link) = data["url"].as_str() else {
        bail!("Este servidor no ofrece un formato de vídeo compatible");
    };
    network::web_url(link)?;
    media.url = link.into();
    if let Some(headers) = data["http_headers"].as_object() {
        for (key, value) in headers {
            let Some(value) = value.as_str() else {
                continue;
            };
            reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
            reqwest::header::HeaderValue::from_str(value)?;
            if key.eq_ignore_ascii_case("referer") {
                media.referer = value.into();
            } else {
                media.headers.insert(key.clone(), value.into());
            }
        }
    }
    let protocol = data["protocol"].as_str().unwrap_or("");
    if protocol.contains("m3u8") || link.contains(".m3u8") {
        media.kind = MediaKind::Hls;
    }
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_manager;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn open_extensions_classify_hls_and_direct_media() {
        assert_eq!(classify_path("/live/master.m3u8"), Some(MediaKind::Hls));
        assert_eq!(classify_path("/playlist.M3U"), Some(MediaKind::Hls));
        assert_eq!(classify_path("/video/ep01.mp4"), Some(MediaKind::Direct));
        assert_eq!(classify_path("/video/ep01.mkv"), Some(MediaKind::Direct));
        assert_eq!(classify_path("/video/ep01.webm"), Some(MediaKind::Direct));
        assert_eq!(classify_path("/video/ep01.ogv"), Some(MediaKind::Direct));
        assert_eq!(classify_path("/embed/abc123"), None);
    }

    #[tokio::test]
    async fn extensionless_open_hls_and_http_video_resolve_without_ytdlp() -> Result<()> {
        let client = network::client()?;
        let d = extension_manager::bundled()?;

        let hls_listener = TcpListener::bind("127.0.0.1:0")?;
        let hls_url = format!("http://{}/open/stream", hls_listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = hls_listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body = b"#EXTM3U\n#EXT-X-VERSION:3\nsegment0.ts\n";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.apple.mpegurl\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(body);
            }
        });
        let resolved_hls = resolve(
            &client,
            &d,
            &Server {
                server: "HLS".into(),
                url: hls_url.clone(),
            },
            "",
        )
        .await?;
        assert_eq!(resolved_hls.kind, MediaKind::Hls);
        assert_eq!(resolved_hls.url, hls_url);

        let mp4_listener = TcpListener::bind("127.0.0.1:0")?;
        let mp4_url = format!("http://{}/open/file", mp4_listener.local_addr()?);
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = mp4_listener.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let body = b"\x00\x00\x00\x18ftypmp42open-video";
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(body);
            }
        });
        let resolved_mp4 = resolve(
            &client,
            &d,
            &Server {
                server: "HTTP".into(),
                url: mp4_url.clone(),
            },
            "",
        )
        .await?;
        assert_eq!(resolved_mp4.kind, MediaKind::Direct);
        assert_eq!(resolved_mp4.url, mp4_url);
        Ok(())
    }
}
