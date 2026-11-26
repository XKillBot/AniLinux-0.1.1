//! Network smoke test. No persisted anime names or fixture data from the website.
use anilinux::{extension_manager, models::Browse, network, parser, resolver};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = network::client()?;
    let d = extension_manager::bundled()?;
    let url = parser::browse_url(
        &d,
        &Browse {
            page: 1,
            ..Default::default()
        },
    )?;
    let html = network::text(&client, &url, &d.base_url).await?;
    let catalog = parser::catalog(&html, &d, false)?;
    let anime = catalog
        .sections
        .iter()
        .flat_map(|(_, a)| a)
        .next()
        .ok_or_else(|| anyhow::anyhow!("Catálogo vacío"))?;
    println!(
        "Catálogo: {} tarjetas, {} géneros, {} páginas",
        catalog.sections.iter().map(|(_, a)| a.len()).sum::<usize>(),
        catalog.genres.len(),
        catalog.total_pages
    );
    // An optional URL is accepted only by this developer tool, never by the UI.
    let detail_url = std::env::args().nth(1).unwrap_or_else(|| anime.url.clone());
    let detail = parser::detail(&network::text(&client, &detail_url, &d.base_url).await?, &d)?;
    println!("Ficha: {} episodios", detail.episodes.len());
    if let Some(ep) = detail.episodes.first() {
        let sources = parser::sources(&network::text(&client, &ep.url, &d.base_url).await?, &d)?;
        println!("Idiomas reales: {:?}", sources.languages());
        if let Some(lang) = sources.languages().first() {
            if let Some(server) = sources.preferred(lang) {
                let media = resolver::resolve(&client, &d, &server, &ep.url).await?;
                println!(
                    "Servidor resuelto: {}, tipo {:?}",
                    server.server, media.kind
                );
            }
        }
    }
    Ok(())
}
