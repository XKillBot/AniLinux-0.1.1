use crate::{
    network,
    resolver::{installed, Media, MediaKind},
    storage,
};
use anyhow::{ensure, Context, Result};
use futures_util::StreamExt;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

pub fn key(episode_url: &str, lang: &str) -> String {
    storage::hash(&format!("{episode_url}|{lang}"))
}
pub fn target(
    root: &Path,
    title: &str,
    episode: &str,
    language: &str,
    key: &str,
    media: &Media,
) -> PathBuf {
    let ext = match media.kind {
        MediaKind::Hls => "mkv",
        MediaKind::Direct => {
            let lower = media.url.split('?').next().unwrap_or("").to_lowercase();
            if lower.ends_with(".webm") {
                "webm"
            } else if lower.ends_with(".mkv") {
                "mkv"
            } else if lower.ends_with(".ogv") {
                "ogv"
            } else if lower.ends_with(".mov") {
                "mov"
            } else if lower.ends_with(".m4v") {
                "m4v"
            } else {
                "mp4"
            }
        }
    };
    root.join(storage::safe_name(title)).join(format!(
        "{}-{}-{}.{}",
        storage::safe_name(episode),
        storage::safe_name(language),
        &key[..12.min(key.len())],
        ext
    ))
}
pub async fn run(
    media: Media,
    target: &Path,
    cancel: CancellationToken,
    progress: impl Fn(Option<f32>) + Send + Sync,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    ensure!(
        !tokio::fs::try_exists(target).await?,
        "El archivo ya existe; no se sobrescribirá"
    );
    let result = match media.kind {
        MediaKind::Direct => direct(&media, target, &cancel, &progress).await,
        MediaKind::Hls => hls(&media, target, &cancel, &progress).await,
    };
    if result.is_err() {
        let _ = tokio::fs::remove_file(target.with_extension("part")).await;
        // yt-dlp/ffmpeg artifacts have fixed, locally generated names.
        for suffix in [
            "working.mkv",
            "working.mkv.part",
            "working.mkv.ytdl",
            "working.temp.mkv",
        ] {
            let _ = tokio::fs::remove_file(target.with_extension(suffix)).await;
        }
    }
    result
}
async fn direct(
    media: &Media,
    target: &Path,
    cancel: &CancellationToken,
    progress: &(impl Fn(Option<f32>) + Send + Sync),
) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(network::USER_AGENT)
        .connect_timeout(Duration::from_secs(20))
        .build()?;
    let mut req = client.get(&media.url);
    if !media.referer.trim().is_empty() {
        req = req.header("Referer", &media.referer);
    }
    for (k, v) in &media.headers {
        req = req.header(k, v);
    }
    let response = tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("Descarga cancelada"),
        response = tokio::time::timeout(Duration::from_secs(45), req.send()) => response??.error_for_status()?,
    };
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        content_type.starts_with("video/")
            || content_type.starts_with("application/octet-stream")
            || content_type.starts_with("binary/octet-stream")
            || content_type.starts_with("application/x-matroska")
            || content_type.starts_with("application/ogg"),
        "El servidor no entregó un archivo de vídeo"
    );
    let total = response.content_length().filter(|v| *v > 0);
    let tmp = target.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut last = Instant::now();
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("Descarga cancelada"),
            chunk = tokio::time::timeout(Duration::from_secs(60), stream.next()) => chunk.context("El servidor dejó de responder")?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk?;
        if downloaded == 0 {
            // Reject common HTML/JSON error documents even if mislabeled octet-stream.
            let prefix = String::from_utf8_lossy(&chunk[..chunk.len().min(128)])
                .trim_start()
                .to_lowercase();
            ensure!(
                !prefix.starts_with('<')
                    && !prefix.starts_with('{')
                    && !prefix.starts_with("#extm3u"),
                "La respuesta no contiene un vídeo directo"
            );
        }
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            progress(total.map(|total| (downloaded as f64 / total as f64).min(1.0) as f32));
            last = Instant::now();
        }
    }
    ensure!(
        downloaded > 0 && total.is_none_or(|n| n == downloaded),
        "La descarga quedó incompleta"
    );
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(tmp, target).await?;
    progress(Some(1.0));
    Ok(())
}
async fn hls(
    media: &Media,
    target: &Path,
    cancel: &CancellationToken,
    progress: &(impl Fn(Option<f32>) + Send + Sync),
) -> Result<()> {
    ensure!(
        installed("ffmpeg"),
        "Instala FFmpeg para descargar listas HLS (.m3u8)"
    );
    if installed("yt-dlp") {
        yt_dlp_hls(media, target, cancel, progress).await
    } else {
        ffmpeg_hls(media, target, cancel, progress).await
    }
}
async fn yt_dlp_hls(
    media: &Media,
    target: &Path,
    cancel: &CancellationToken,
    progress: &(impl Fn(Option<f32>) + Send + Sync),
) -> Result<()> {
    let working = target.with_extension("working.mkv");
    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.args([
        "--ignore-config",
        "--no-playlist",
        "--no-call-home",
        "--newline",
        "--no-warnings",
        "--no-continue",
        "--socket-timeout",
        "30",
        "--retries",
        "3",
        "--fragment-retries",
        "3",
        "--abort-on-unavailable-fragments",
        "--progress-template",
        "download:anilinux:%(progress._percent_str)s",
        "--user-agent",
        network::USER_AGENT,
        "--remux-video",
        "mkv",
    ]);
    if !media.referer.trim().is_empty() {
        cmd.args(["--referer", &media.referer]);
    }
    cmd.arg("-o")
        .arg(&working)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (k, v) in &media.headers {
        cmd.arg("--add-headers").arg(format!("{k}:{v}"));
    }
    cmd.arg("--").arg(&media.url);
    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("No se pudo iniciar la descarga"))?;
    let mut lines = BufReader::new(stdout).lines();
    progress(None);
    loop {
        let line = tokio::select! {
            _ = cancel.cancelled() => { let _ = child.kill().await; anyhow::bail!("Descarga cancelada"); }
            line = lines.next_line() => line?,
        };
        let Some(line) = line else {
            break;
        };
        if let Some(s) = line.strip_prefix("anilinux:") {
            progress(
                s.trim()
                    .trim_end_matches('%')
                    .parse::<f32>()
                    .ok()
                    .map(|v| (v / 100.0).clamp(0.0, 1.0)),
            );
        }
    }
    let status = tokio::select! {
        _ = cancel.cancelled() => { let _ = child.kill().await; anyhow::bail!("Descarga cancelada"); }
        status = child.wait() => status?,
    };
    ensure!(
        status.success(),
        "El servidor interrumpió la descarga; prueba con otro servidor"
    );
    ensure!(
        tokio::fs::metadata(&working).await?.len() > 0,
        "El vídeo descargado está vacío"
    );
    tokio::fs::rename(working, target).await?;
    progress(Some(1.0));
    Ok(())
}
async fn ffmpeg_hls(
    media: &Media,
    target: &Path,
    cancel: &CancellationToken,
    progress: &(impl Fn(Option<f32>) + Send + Sync),
) -> Result<()> {
    let working = target.with_extension("working.mkv");
    let agent = media
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(network::USER_AGENT);
    let mut raw_headers = String::new();
    if !media.referer.trim().is_empty() {
        let clean = media.referer.replace(['\r', '\n'], "");
        raw_headers.push_str(&format!("Referer: {clean}\r\n"));
    }
    for (k, v) in &media.headers {
        if k.eq_ignore_ascii_case("user-agent") || k.eq_ignore_ascii_case("referer") {
            continue;
        }
        let ck = k.replace(['\r', '\n'], "");
        let cv = v.replace(['\r', '\n'], "");
        raw_headers.push_str(&format!("{ck}: {cv}\r\n"));
    }
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args([
        "-nostdin",
        "-loglevel",
        "error",
        "-y",
        "-rw_timeout",
        "30000000",
        "-user_agent",
        agent,
    ]);
    if !raw_headers.is_empty() {
        cmd.arg("-headers").arg(&raw_headers);
    }
    cmd.args(["-i", &media.url, "-c", "copy", "-f", "matroska"])
        .arg(&working)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    progress(None);
    let status = tokio::select! {
        _ = cancel.cancelled() => { let _ = child.kill().await; anyhow::bail!("Descarga cancelada"); }
        status = child.wait() => status?,
    };
    ensure!(
        status.success(),
        "FFmpeg no pudo completar la descarga del flujo HLS"
    );
    ensure!(
        tokio::fs::metadata(&working).await?.len() > 0,
        "El vídeo descargado está vacío"
    );
    tokio::fs::rename(working, target).await?;
    progress(Some(1.0));
    Ok(())
}
