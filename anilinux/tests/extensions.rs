use anilinux::extension_manager::*;
use sha2::{Digest, Sha256};
#[test]
fn hashes_empty_whitespace_valid_invalid() -> anyhow::Result<()> {
    verify_hash(b"payload", "")?;
    verify_hash(b"payload", "   \n")?;
    let expected = format!("{:x}", Sha256::digest(b"payload"));
    verify_hash(b"payload", &expected.to_uppercase())?;
    assert!(verify_hash(b"changed", &expected).is_err());
    assert!(verify_hash(b"payload", "not-a-hash").is_err());
    Ok(())
}
#[test]
fn manifest_code_and_semantic_versions() -> anyhow::Result<()> {
    let mut local = bundled()?;
    let mut remote = Manifest {
        id: "animeav1".into(),
        version: "1.0.1".into(),
        version_code: Some(2),
        extension_url: "https://example.org/source.json".into(),
        sha256: String::new(),
    };
    assert!(is_newer(&remote, &local));
    remote.version_code = Some(1);
    assert!(!is_newer(&remote, &local));
    local.version_code = None;
    assert!(is_newer(&remote, &local));
    remote.version = "0.9.0".into();
    assert!(!is_newer(&remote, &local));
    Ok(())
}
#[test]
fn legacy_remote_format_has_safe_defaults() -> anyhow::Result<()> {
    let d=parse(br#"{"id":"animeav1","version":"1.1.0","version_code":2,"base_url":"https://animeav1.com","script_marker":"node_ids","patterns":{"episodes":"\\{id:(\\d+),number:([0-9.]+)\\}"}}"#)?;
    assert_eq!(d.routes.search_param, "search");
    assert!(!d.resolvers.is_empty());
    assert!(parse(br#"{"id":"wrong"}"#).is_err());
    Ok(())
}
#[tokio::test]
async fn corrupted_local_uses_previous_then_bundled() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    tokio::fs::write(root.path().join("extension.json"), "broken").await?;
    assert_eq!(load(root.path()).await?.version, "1.0.0");
    let mut d = bundled()?;
    d.version = "1.2.0".into();
    d.version_code = Some(4);
    tokio::fs::write(
        root.path().join("extension.previous.json"),
        serde_json::to_vec(&d)?,
    )
    .await?;
    assert_eq!(load(root.path()).await?.version, "1.2.0");
    Ok(())
}

#[tokio::test]
async fn activation_is_atomic_and_keeps_valid_previous() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let mut d = bundled()?;
    d.version = "1.1.0".into();
    d.version_code = Some(2);
    let bytes = serde_json::to_vec(&d)?;
    let mut m = Manifest {
        id: d.id.clone(),
        version: d.version.clone(),
        version_code: d.version_code,
        extension_url: "https://example.org/source.json".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    install(root.path(), &m, &bytes).await?;
    assert_eq!(load(root.path()).await?.version, "1.1.0");
    let previous = tokio::fs::read(root.path().join("extension.previous.json")).await?;
    assert_eq!(parse(&previous)?.version, "1.0.0");
    m.version = "1.2.0".into();
    m.version_code = Some(3);
    assert!(install(root.path(), &m, b"corrupted").await.is_err());
    assert_eq!(load(root.path()).await?.version, "1.1.0");
    m.sha256 = "  ".into();
    assert!(install(root.path(), &m, &bytes).await.is_err()); // Manifest/payload mismatch.
    assert_eq!(load(root.path()).await?.version, "1.1.0");
    Ok(())
}
