use anyhow::{bail, Result};
use futures_util::StreamExt;
use std::time::Duration;
use url::Url;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0";
pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()?)
}
pub fn web_url(s: &str) -> Result<Url> {
    let url = Url::parse(s)?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Dirección no admitida");
    }
    Ok(url)
}
pub async fn bytes(
    client: &reqwest::Client,
    url: &str,
    referer: &str,
    limit: usize,
) -> Result<Vec<u8>> {
    web_url(url)?;
    let mut response = client
        .get(url)
        .header(reqwest::header::REFERER, referer)
        .send()
        .await?
        .error_for_status()?
        .bytes_stream();
    let mut result = Vec::new();
    while let Some(chunk) = response.next().await {
        let chunk = chunk?;
        if result.len() + chunk.len() > limit {
            bail!("La respuesta supera el límite permitido");
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}
pub async fn text(client: &reqwest::Client, url: &str, referer: &str) -> Result<String> {
    Ok(String::from_utf8(
        bytes(client, url, referer, 8 * 1024 * 1024).await?,
    )?)
}
