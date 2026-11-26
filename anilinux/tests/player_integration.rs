//! Explicit opt-in tests with a real mpv, display and a locally generated video.
use anilinux::{
    player::{self, Confirmation, Failure},
    resolver::{Media, MediaKind},
};

#[tokio::test]
#[ignore = "requires mpv, a graphical session and ANILINUX_TEST_VIDEO"]
async fn actual_mpv_confirms_local_playback() -> anyhow::Result<()> {
    let path = std::env::var("ANILINUX_TEST_VIDEO")?;
    assert_eq!(
        player::play_file(std::path::Path::new(&path), "mpv").await?,
        Confirmation::Playing
    );
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    Ok(())
}
#[tokio::test]
#[ignore = "requires mpv, a graphical session and ANILINUX_TEST_STREAM"]
async fn actual_mpv_confirms_http_stream_with_headers() -> anyhow::Result<()> {
    let media = Media {
        url: std::env::var("ANILINUX_TEST_STREAM")?,
        referer: "https://example.org/watch".into(),
        headers: Default::default(),
        kind: MediaKind::Direct,
    };
    assert_eq!(player::play(&media, "mpv").await?, Confirmation::Playing);
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    Ok(())
}
#[tokio::test]
#[ignore = "requires mpv, a graphical session and ANILINUX_TEST_STREAM"]
async fn actual_mpv_rejects_http_error_instead_of_claiming_success() -> anyhow::Result<()> {
    let url = url::Url::parse(&std::env::var("ANILINUX_TEST_STREAM")?)?
        .join("missing.mp4")?
        .to_string();
    let media = Media {
        url,
        referer: "https://example.org/watch".into(),
        headers: Default::default(),
        kind: MediaKind::Direct,
    };
    assert_eq!(player::play(&media, "mpv").await, Err(Failure::Open));
    Ok(())
}
