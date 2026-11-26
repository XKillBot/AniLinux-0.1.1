use anilinux::{
    downloader,
    resolver::{Media, MediaKind},
    storage,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
fn server(body: Vec<u8>, content_type: &'static str) -> anyhow::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/video.mp4", listener.local_addr()?);
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut req = [0u8; 4096];
            let _ = stream.read(&mut req);
            let header=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",body.len(),content_type);
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Ok(url)
}
#[tokio::test]
async fn direct_download_writes_video_and_rejects_overwrite() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("test.mp4");
    let bytes = b"\x00\x00\x00\x18ftypmp42test-data".to_vec();
    let url = server(bytes.clone(), "video/mp4")?;
    let media = Media {
        url,
        referer: "https://animeav1.com/".into(),
        headers: Default::default(),
        kind: MediaKind::Direct,
    };
    downloader::run(media.clone(), &path, CancellationToken::new(), |_| {}).await?;
    assert_eq!(tokio::fs::read(&path).await?, bytes);
    assert!(
        downloader::run(media, &path, CancellationToken::new(), |_| {})
            .await
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn html_is_not_saved_as_video() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("test.mp4");
    for content_type in ["text/html", "application/octet-stream"] {
        let media = Media {
            url: server(b"<!doctype html>not a video".to_vec(), content_type)?,
            referer: "https://animeav1.com/".into(),
            headers: Default::default(),
            kind: MediaKind::Direct,
        };
        assert!(
            downloader::run(media, &path, CancellationToken::new(), |_| {})
                .await
                .is_err()
        );
        assert!(!path.exists());
        assert!(!path.with_extension("part").exists());
    }
    Ok(())
}
#[test]
fn filenames_cannot_escape_download_folder() {
    let name = storage::safe_name("../../escape/evil | & $(touch test)");
    assert!(!name.contains('/'));
    assert!(!name.starts_with('.'));
    assert_eq!(storage::safe_name("..."), "episodio");
    assert_ne!(
        downloader::key("https://example.org/1", "SUB"),
        downloader::key("https://example.org/1", "DUB")
    );
}
