//! Declarative, non-executable source definitions. All activation is transactional.
use crate::{network, storage};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/XKillBot/AniLinux/main/extension_manifest.json";
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Definition {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub version_code: Option<u64>,
    pub base_url: String,
    pub script_marker: String,
    #[serde(default)]
    pub selectors: Selectors,
    #[serde(default)]
    pub routes: Routes,
    #[serde(default)]
    pub fields: Fields,
    #[serde(default)]
    pub assets: Assets,
    #[serde(default = "default_resolvers")]
    pub resolvers: Vec<Resolver>,
    // Compatible with v1 of the user's extension. Optional regular expressions may
    // override episode/link extraction without installing executable code.
    #[serde(default)]
    pub patterns: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Selectors {
    pub scripts: String,
    pub cards: String,
    pub image: String,
}
impl Default for Selectors {
    fn default() -> Self {
        Self {
            scripts: "script".into(),
            cards: "a[href*='/media/']".into(),
            image: "img".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Routes {
    pub catalog: String,
    pub media: String,
    pub episode: String,
    pub search_param: String,
    pub page_param: String,
    pub genre_param: String,
    pub status_param: String,
    pub airing_value: String,
}
impl Default for Routes {
    fn default() -> Self {
        Self {
            catalog: "/catalogo".into(),
            media: "/media/{slug}".into(),
            episode: "/media/{slug}/{number}".into(),
            search_param: "search".into(),
            page_param: "page".into(),
            genre_param: "genre".into(),
            status_param: "status".into(),
            airing_value: "emision".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Fields {
    pub results: String,
    pub latest: String,
    pub recent: String,
    pub media: String,
    pub episodes: String,
    pub embeds: String,
    pub downloads: String,
    pub genres: String,
    pub pagination: String,
}
impl Default for Fields {
    fn default() -> Self {
        Self {
            results: "results".into(),
            latest: "latestMedia".into(),
            recent: "latestEpisodes".into(),
            media: "media".into(),
            episodes: "episodes".into(),
            embeds: "embeds".into(),
            downloads: "downloads".into(),
            genres: "genresIdsMap".into(),
            pagination: "pagination".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Assets {
    pub cover: String,
    pub banner: String,
    pub thumbnail: String,
}
impl Default for Assets {
    fn default() -> Self {
        Self {
            cover: "https://cdn.animeav1.com/covers/{id}.jpg".into(),
            banner: "https://cdn.animeav1.com/backdrops/{id}.jpg".into(),
            thumbnail: "https://cdn.animeav1.com/screenshots/{id}/{number}.jpg".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Resolver {
    pub host: String,
    pub path_pattern: String,
    pub replacement: String,
    pub kind: String,
}
fn default_resolvers() -> Vec<Resolver> {
    vec![Resolver {
        host: "player.zilla-networks.com".into(),
        path_pattern: "^/play/([a-fA-F0-9]{32})/?$".into(),
        replacement: "/m3u8/$1".into(),
        kind: "hls".into(),
    }]
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub version_code: Option<u64>,
    #[serde(alias = "url")]
    pub extension_url: String,
    #[serde(default)]
    pub sha256: String,
}
pub fn bundled() -> Result<Definition> {
    parse(include_bytes!("../extension/animeav1.json"))
}
pub fn parse(bytes: &[u8]) -> Result<Definition> {
    let d: Definition = serde_json::from_slice(bytes)?;
    ensure!(d.id == "animeav1", "Fuente no reconocida");
    semver::Version::parse(&d.version)?;
    let base = network::web_url(&d.base_url)?;
    ensure!(base.scheme() == "https", "La fuente debe usar HTTPS");
    ensure!(!d.script_marker.is_empty(), "Marcador vacío");
    for sel in [&d.selectors.scripts, &d.selectors.cards, &d.selectors.image] {
        scraper::Selector::parse(sel).map_err(|e| anyhow::anyhow!("Selector inválido: {e:?}"))?;
    }
    for r in &d.resolvers {
        regex::Regex::new(&r.path_pattern)?;
        ensure!(
            r.kind == "hls" || r.kind == "direct",
            "Tipo de resolvedor inválido"
        );
    }
    for pattern in d.patterns.values() {
        regex::Regex::new(pattern)?;
    }
    Ok(d)
}
fn active_path(root: &Path) -> PathBuf {
    root.join("extension.json")
}
pub async fn load(root: &Path) -> Result<Definition> {
    for name in ["extension.json", "extension.previous.json"] {
        if let Ok(bytes) = tokio::fs::read(root.join(name)).await {
            if let Ok(d) = parse(&bytes) {
                return Ok(d);
            }
        }
    }
    bundled()
}
pub fn is_newer(remote: &Manifest, local: &Definition) -> bool {
    match (remote.version_code, local.version_code) {
        (Some(r), Some(l)) => r > l,
        _ => match (
            semver::Version::parse(&remote.version),
            semver::Version::parse(&local.version),
        ) {
            (Ok(r), Ok(l)) => r > l,
            _ => false,
        },
    }
}
pub async fn check(client: &reqwest::Client, local: &Definition) -> Result<Option<Manifest>> {
    let bytes = network::bytes(client, MANIFEST_URL, "https://animeav1.com/", 256 * 1024).await?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(manifest.id == local.id, "Manifiesto de otra fuente");
    semver::Version::parse(&manifest.version)?;
    ensure!(
        network::web_url(&manifest.extension_url)?.scheme() == "https",
        "Se requiere HTTPS"
    );
    Ok(is_newer(&manifest, local).then_some(manifest))
}
pub fn verify_hash(bytes: &[u8], expected: &str) -> Result<()> {
    let expected = expected.trim();
    if expected.is_empty() {
        return Ok(());
    }
    ensure!(
        expected.len() == 64 && expected.bytes().all(|c| c.is_ascii_hexdigit()),
        "Huella inválida"
    );
    let actual = format!("{:x}", Sha256::digest(bytes));
    ensure!(
        actual.eq_ignore_ascii_case(expected),
        "La descarga no superó la verificación de integridad"
    );
    Ok(())
}
pub async fn apply(
    client: &reqwest::Client,
    root: &Path,
    manifest: &Manifest,
) -> Result<Definition> {
    ensure!(
        network::web_url(&manifest.extension_url)?.scheme() == "https",
        "Se requiere HTTPS"
    );
    let bytes = network::bytes(
        client,
        &manifest.extension_url,
        "https://animeav1.com/",
        1024 * 1024,
    )
    .await?;
    install(root, manifest, &bytes).await
}
/// Validate and commit already-downloaded bytes, also used by offline tests.
pub async fn install(root: &Path, manifest: &Manifest, bytes: &[u8]) -> Result<Definition> {
    ensure!(bytes.len() <= 1024 * 1024, "Definición demasiado grande");
    verify_hash(bytes, &manifest.sha256)?;
    let definition = parse(bytes).context("Definición de fuente inválida")?;
    ensure!(
        definition.id == manifest.id
            && definition.version == manifest.version
            && definition.version_code == manifest.version_code,
        "Las versiones no coinciden"
    );
    let old = load(root).await?;
    if !is_newer(manifest, &old) {
        bail!("La actualización ya no es más reciente");
    }
    // Back up the last VALID definition, including the bundled one if needed.
    storage::atomic_write(
        &root.join("extension.previous.json"),
        &serde_json::to_vec_pretty(&old)?,
    )
    .await?;
    storage::atomic_write(&active_path(root), bytes).await?;
    Ok(definition)
}
