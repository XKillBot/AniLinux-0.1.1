//! External players. mpv confirms actual playback over its private JSON IPC socket;
//! VLC supports native, Snap and Flatpak installations for both local files and HTTP/HLS streams.
use crate::{
    network,
    resolver::{installed, Media},
};
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    process::{Child, Command},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Resolve,
    MissingPlayer,
    Launch,
    Open,
    Timeout,
    Headers,
    LocalFile,
}
impl Failure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Resolve => "No se pudo obtener el vídeo de este servidor. Prueba con otro servidor.",
            Self::MissingPlayer => "No se encontró el reproductor seleccionado. Instala mpv o VLC, o cámbialo en Opciones.",
            Self::Launch => "No se pudo iniciar el reproductor. Comprueba que pueda abrirse desde tu sesión de escritorio.",
            Self::Open => "El reproductor no pudo abrir el vídeo. Prueba otro servidor o reproduce un vídeo local para comprobarlo.",
            Self::Timeout => "El reproductor no confirmó la reproducción a tiempo. Comprueba la conexión y prueba otro servidor.",
            Self::Headers => "Este flujo requiere cabeceras de autenticación o un reproductor de streaming (elige mpv o VLC).",
            Self::LocalFile => "El archivo local ya no está disponible o no se puede leer.",
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for Failure {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// IPC playback-restart: mpv started playback, not just a running process.
    Playing,
    /// VLC/xdg-open launched; no claim that video frames have been displayed.
    Launched,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VlcTarget {
    Native(PathBuf),
    Flatpak,
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn find_vlc() -> Option<VlcTarget> {
    if let Some(bin) = find_in_path("vlc") {
        return Some(VlcTarget::Native(bin));
    }
    let mut fallbacks = vec![
        PathBuf::from("/snap/bin/vlc"),
        PathBuf::from("/usr/bin/vlc"),
        PathBuf::from("/usr/local/bin/vlc"),
        PathBuf::from("/var/lib/flatpak/exports/bin/org.videolan.VLC"),
    ];
    if let Some(data) = dirs::data_local_dir() {
        fallbacks.push(data.join("flatpak/exports/bin/org.videolan.VLC"));
    }
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join(".local/share/flatpak/exports/bin/org.videolan.VLC"));
    }
    if let Some(bin) = fallbacks.into_iter().find(|p| is_executable(p)) {
        return Some(VlcTarget::Native(bin));
    }
    if installed("flatpak") {
        let mut app_dirs = vec![PathBuf::from("/var/lib/flatpak/app/org.videolan.VLC")];
        if let Some(data) = dirs::data_local_dir() {
            app_dirs.push(data.join("flatpak/app/org.videolan.VLC"));
        }
        if let Some(home) = dirs::home_dir() {
            app_dirs.push(home.join(".local/share/flatpak/app/org.videolan.VLC"));
        }
        if app_dirs.iter().any(|d| d.is_dir()) {
            return Some(VlcTarget::Flatpak);
        }
    }
    None
}

pub fn player_available(name: &str) -> bool {
    match name {
        "vlc" => find_vlc().is_some(),
        other => installed(other),
    }
}

fn choose(preference: &str) -> Result<&str, Failure> {
    if preference != "auto" {
        if !["mpv", "vlc", "xdg-open"].contains(&preference) || !player_available(preference) {
            return Err(Failure::MissingPlayer);
        }
        return Ok(preference);
    }
    ["mpv", "vlc", "xdg-open"]
        .into_iter()
        .find(|p| player_available(p))
        .ok_or(Failure::MissingPlayer)
}

fn software_mode() -> bool {
    std::env::var("LIBGL_ALWAYS_SOFTWARE").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn mpv_command(socket: &Path, media: Option<&Media>, software: bool) -> Command {
    let mut cmd = Command::new("mpv");
    cmd.arg("--idle=once")
        .arg("--force-window=immediate")
        .arg("--network-timeout=30")
        .arg("--msg-level=all=warn")
        .arg(format!("--input-ipc-server={}", socket.display()));
    if software {
        cmd.args(["--hwdec=no", "--vo=gpu", "--gpu-api=opengl", "--gpu-sw=yes"]);
    }
    if let Some(media) = media {
        let agent = media
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(network::USER_AGENT);
        // mpv REQUIRES --option=value. '--referrer URL' is a fatal CLI error.
        if !media.referer.trim().is_empty() {
            cmd.arg(format!("--referrer={}", media.referer));
        }
        cmd.arg(format!("--user-agent={agent}"));
        let headers: Vec<_> = media
            .headers
            .iter()
            .filter(|(k, _)| !k.eq_ignore_ascii_case("user-agent"))
            .map(|(k, v)| {
                format!("{k}: {v}")
                    .replace('\\', "\\\\")
                    .replace(',', "\\,")
            })
            .collect();
        if !headers.is_empty() {
            cmd.arg(format!("--http-header-fields={}", headers.join(",")));
        }
    }
    cmd
}

async fn terminate(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// Wait for the IPC socket without blocking egui; opening the media happens only
/// AFTER connecting, so file-loaded/end-file/playback-restart cannot be missed.
async fn connect(child: &mut Child, socket: &Path) -> Result<UnixStream, Failure> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        match UnixStream::connect(socket).await {
            Ok(stream) => return Ok(stream),
            Err(e)
                if !matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                return Err(Failure::Launch)
            }
            Err(_) => {}
        }
        if child.try_wait().map_err(|_| Failure::Launch)?.is_some() {
            return Err(Failure::Launch);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Failure::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[derive(PartialEq, Debug)]
enum Signal {
    Waiting,
    Playing,
    Failed,
}

fn signal(event: &Value) -> Signal {
    if event["event"] == "playback-restart" {
        return Signal::Playing;
    }
    if event["event"] == "end-file" && event["reason"] != "redirect" {
        return Signal::Failed;
    }
    if event["request_id"] == 1 && event["error"].as_str().is_some_and(|e| e != "success") {
        return Signal::Failed;
    }
    Signal::Waiting
}

async fn confirm(child: &mut Child, stream: &mut UnixStream, source: &str) -> Result<(), Failure> {
    let command =
        json!({"command":["loadfile",source,"replace"],"request_id":1}).to_string() + "\n";
    stream
        .write_all(command.as_bytes())
        .await
        .map_err(|_| Failure::Open)?;
    let mut reader = BufReader::new(stream);
    let wait = async {
        loop {
            // Bounded IPC messages even with unexpected metadata.
            let mut bytes = Vec::new();
            let mut limited = (&mut reader).take(256 * 1024);
            let n = tokio::select! {
                status = child.wait() => { let _ = status; return Err(Failure::Open); },
                n = limited.read_until(b'\n', &mut bytes) => n.map_err(|_| Failure::Open)?,
            };
            if n == 0 || n >= 256 * 1024 {
                return Err(Failure::Open);
            }
            let Ok(event) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            match signal(&event) {
                Signal::Playing => return Ok(()),
                Signal::Failed => return Err(Failure::Open),
                Signal::Waiting => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(45), wait)
        .await
        .map_err(|_| Failure::Timeout)?
}

async fn mpv(source: &str, media: Option<&Media>) -> Result<Confirmation, Failure> {
    let dir = tempfile::Builder::new()
        .prefix("anilinux-player-")
        .tempdir()
        .map_err(|_| Failure::Launch)?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| Failure::Launch)?;
    let socket = dir.path().join("ipc.sock");
    let mut child = mpv_command(&socket, media, software_mode())
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            eprintln!("AniLinux: no se pudo iniciar mpv: {e}");
            Failure::Launch
        })?;
    let result = async {
        let mut stream = connect(&mut child, &socket).await?;
        confirm(&mut child, &mut stream, source).await?;
        Ok::<_, Failure>(stream)
    }
    .await;
    match result {
        Ok(mut stream) => {
            tokio::spawn(async move {
                // Drain subscribed events until exit; keep the private socket alive.
                let mut sink = tokio::io::sink();
                tokio::select! {
                    _ = child.wait() => {},
                    _ = tokio::io::copy(&mut stream, &mut sink) => { let _ = child.wait().await; },
                }
                drop(dir);
            });
            Ok(Confirmation::Playing)
        }
        Err(error) => {
            terminate(&mut child).await;
            Err(error)
        }
    }
}

async fn launch_unconfirmed(mut command: Command) -> Result<Confirmation, Failure> {
    // Do not suppress diagnostics and do not reuse an invisible VLC instance.
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            eprintln!("AniLinux: no se pudo iniciar el reproductor: {e}");
            Failure::Launch
        })?;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    if let Some(status) = child.try_wait().map_err(|_| Failure::Launch)? {
        if !status.success() {
            return Err(Failure::Open);
        }
    } else {
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
    }
    Ok(Confirmation::Launched)
}

fn vlc_command_with(
    target: VlcTarget,
    media: Option<&Media>,
    source: &str,
    software: bool,
) -> Result<Command, Failure> {
    let mut cmd = match target {
        VlcTarget::Native(bin) => {
            let is_snap = bin.starts_with("/snap")
                || std::fs::read_link(&bin)
                    .is_ok_and(|l| l.starts_with("/snap") || l.ends_with("snap"));
            let mut c = Command::new(&bin);
            // Prevent Ubuntu Snap / host GTK module symbol clashes on launch.
            for var in ["GTK_PATH", "GIO_EXTRA_MODULES", "LOCPATH"] {
                c.env_remove(var);
            }
            if is_snap {
                for var in ["LD_LIBRARY_PATH", "QT_PLUGIN_PATH", "QML2_IMPORT_PATH"] {
                    c.env_remove(var);
                }
            }
            c
        }
        VlcTarget::Flatpak => {
            let mut c = Command::new("flatpak");
            c.arg("run");
            if media.is_none() {
                if let Some(parent) = Path::new(source).parent() {
                    c.arg(format!("--filesystem={}:ro", parent.display()));
                }
            }
            c.arg("org.videolan.VLC");
            c
        }
    };
    if std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("QT_QPA_PLATFORM").is_none()
    {
        // VLC 3.x Qt5 embeds video reliably via XWayland when both servers are active.
        cmd.env("QT_QPA_PLATFORM", "xcb");
    }
    cmd.arg("--no-one-instance")
        .arg("--no-one-instance-when-started-from-file")
        .arg("--no-playlist-enqueue");
    if software {
        cmd.arg("--avcodec-hw=none")
            .arg("--vout=xcb-x11,xcb-xv,wl,any");
        cmd.env("QT_XCB_GL_INTEGRATION", "none");
    }
    if let Some(media) = media {
        if media.headers.keys().any(|k| {
            k.eq_ignore_ascii_case("authorization") || k.eq_ignore_ascii_case("proxy-authorization")
        }) {
            return Err(Failure::Headers);
        }
        let agent = media
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or(network::USER_AGENT)
            .replace(['\r', '\n'], "");
        let referer = if !media.referer.trim().is_empty() {
            Some(media.referer.replace(['\r', '\n'], ""))
        } else {
            media
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("referer"))
                .map(|(_, v)| v.replace(['\r', '\n'], ""))
                .filter(|v| !v.trim().is_empty())
        };
        if let Some(ref r) = referer {
            cmd.arg(format!("--http-referrer={r}"));
        }
        cmd.arg(format!("--http-user-agent={agent}"));
        cmd.arg("--").arg(source);
        // Item-scoped options follow the MRL so libvlc_set_user_agent in bin/vlc.c
        // cannot overwrite the custom User-Agent.
        if let Some(ref r) = referer {
            cmd.arg(format!(":http-referrer={r}"));
        }
        cmd.arg(format!(":http-user-agent={agent}"));
        cmd.arg(":network-caching=3000");
    } else {
        cmd.arg("--").arg(source);
    }
    Ok(cmd)
}

fn vlc_command(media: Option<&Media>, source: &str) -> Result<Command, Failure> {
    let target = find_vlc().unwrap_or_else(|| VlcTarget::Native(PathBuf::from("vlc")));
    vlc_command_with(target, media, source, software_mode())
}

pub async fn play(media: &Media, preference: &str) -> Result<Confirmation, Failure> {
    network::web_url(&media.url).map_err(|_| Failure::Resolve)?;
    match choose(preference)? {
        "mpv" => mpv(&media.url, Some(media)).await,
        "vlc" => launch_unconfirmed(vlc_command(Some(media), &media.url)?).await,
        _ => Err(Failure::Headers),
    }
}

pub async fn play_file(path: &Path, preference: &str) -> Result<Confirmation, Failure> {
    let path = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| Failure::LocalFile)?;
    if !tokio::fs::metadata(&path)
        .await
        .map_err(|_| Failure::LocalFile)?
        .is_file()
    {
        return Err(Failure::LocalFile);
    }
    let source = path.to_str().ok_or(Failure::LocalFile)?;
    match choose(preference)? {
        "mpv" => mpv(source, None).await,
        "vlc" => launch_unconfirmed(vlc_command(None, source)?).await,
        _ => {
            let mut cmd = Command::new("xdg-open");
            cmd.arg(&path);
            launch_unconfirmed(cmd).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::MediaKind;
    use std::collections::BTreeMap;

    #[test]
    fn mpv_options_use_equals_not_separate_arguments() {
        let media = Media {
            url: "https://example.org/video.mp4".into(),
            referer: "https://example.org/watch".into(),
            headers: Default::default(),
            kind: MediaKind::Direct,
        };
        let cmd = mpv_command(Path::new("/tmp/private/ipc.sock"), Some(&media), true);
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--referrer=https://example.org/watch".into()));
        assert!(args.contains(&format!("--user-agent={}", network::USER_AGENT)));
        assert!(args.contains(&"--hwdec=no".into()));
        assert!(args.contains(&"--gpu-sw=yes".into()));
        assert!(!args.contains(&"--referrer".into()));
        assert!(!args.contains(&"--user-agent".into()));
        assert!(!args.contains(&media.url)); // Sent only after IPC connection.
    }

    #[test]
    fn process_running_or_loadfile_ack_is_not_playback_confirmation() {
        assert_eq!(
            signal(&json!({"error":"success","request_id":1})),
            Signal::Waiting
        );
        assert_eq!(signal(&json!({"event":"file-loaded"})), Signal::Waiting);
        assert_eq!(
            signal(&json!({"event":"playback-restart"})),
            Signal::Playing
        );
        assert_eq!(
            signal(&json!({"event":"end-file","reason":"error"})),
            Signal::Failed
        );
    }

    #[test]
    fn vlc_starts_a_new_instance_and_supports_software_vout() {
        let cmd = vlc_command_with(
            VlcTarget::Native(PathBuf::from("vlc")),
            None,
            "/tmp/test.mp4",
            true,
        )
        .expect("local VLC command");
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--no-one-instance".into()));
        assert!(args.contains(&"--no-one-instance-when-started-from-file".into()));
        assert!(args.contains(&"--avcodec-hw=none".into()));
        assert!(args.contains(&"--vout=xcb-x11,xcb-xv,wl,any".into()));
        assert!(!args.contains(&"--play-and-exit".into()));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/test.mp4"));
    }

    #[test]
    fn vlc_accepts_standard_browser_headers_and_attaches_item_options() {
        let mut headers = BTreeMap::new();
        headers.insert("User-Agent".into(), "CustomUA/1.0".into());
        headers.insert("Accept".into(), "*/*".into());
        headers.insert("Accept-Language".into(), "es-ES,es;q=0.9".into());
        headers.insert("Sec-Fetch-Mode".into(), "navigate".into());
        let media = Media {
            url: "https://example.org/live/master.m3u8".into(),
            referer: "https://example.org/player".into(),
            headers,
            kind: MediaKind::Hls,
        };
        let cmd = vlc_command_with(
            VlcTarget::Native(PathBuf::from("vlc")),
            Some(&media),
            &media.url,
            false,
        )
        .expect("VLC streaming command should accept standard headers");
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let url_pos = args
            .iter()
            .position(|a| a == "https://example.org/live/master.m3u8")
            .expect("stream URL present");
        assert!(args[url_pos + 1..].contains(&":http-referrer=https://example.org/player".into()));
        assert!(args[url_pos + 1..].contains(&":http-user-agent=CustomUA/1.0".into()));
        assert!(args[url_pos + 1..].contains(&":network-caching=3000".into()));
    }
}
