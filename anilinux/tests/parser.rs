use anilinux::{extension_manager, models::Browse, parser};
fn page(data: &str) -> String {
    format!(
        r#"<html><script>kit.start(x, {{ node_ids: [0,2,9], data: [{data}] }});</script></html>"#
    )
}

#[test]
fn catalog_aliases_quotes_and_pagination() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let html = page(
        r#"(function(a){return {data:{results:[{id:'42',slug:'fixture-a',title:'Serie de prueba',synopsis:'Una llave } y un corchete ]',category:a}],genresIdsMap:{'3':{name:'Prueba'}},pagination:{currentPage:2,totalPages:7}}}}({}))"#,
    );
    let c = parser::catalog(&html, &d, false)?;
    assert_eq!(c.sections[0].1.len(), 1);
    assert_eq!(c.sections[0].1[0].synopsis, "Una llave } y un corchete ]");
    assert_eq!(c.genres[0].1, "Prueba");
    assert_eq!(c.page, 2);
    assert_eq!(c.total_pages, 7);
    Ok(())
}
#[test]
fn detail_episodes_sorted_not_dependent_on_property_order() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let html = page(
        r#"{data:{media:{title:'Prueba',id:42,slug:'prueba',startDate:'2024-10-01',category:{name:'TV'},genres:[{name:'Prueba'}],episodes:[{title:'Especial',number:2.5,id:12},{number:1,id:9},{id:9,number:1}],optional:void 0}}}"#,
    );
    let a = parser::detail(&html, &d)?;
    assert_eq!(a.episodes.len(), 2);
    assert_eq!(a.episodes[0].number, 1.0);
    assert_eq!(a.episodes[1].number, 2.5);
    assert_eq!(a.year.as_deref(), Some("2024"));
    assert!(a.episodes[1].url.ends_with("/2.5"));
    Ok(())
}
#[test]
fn all_languages_parsed_and_empty_dub_hidden() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let html = page(
        r#"{embeds:{SUB:[{server:'HLS',url:'https://example.org/a.m3u8?x=1&y=2'}],DUB:[]},downloads:{DUB:[{url:'https://example.org/video.mp4',server:'Directo'}]}}"#,
    );
    let s = parser::sources(&html, &d)?;
    assert_eq!(s.languages(), vec!["SUB", "DUB"]);
    assert_eq!(s.downloads["DUB"][0].server, "Directo");
    let sub = parser::sources(
        &page(r#"{embeds:{SUB:[{server:'A',url:'https://example.org/a'}],DUB:[]}}"#),
        &d,
    )?;
    assert_eq!(sub.languages(), vec!["SUB"]);
    let dub = parser::sources(
        &page(r#"{embeds:{DUB:[{server:'A',url:'https://example.org/a'}]}}"#),
        &d,
    )?;
    assert_eq!(dub.languages(), vec!["DUB"]);
    Ok(())
}
#[test]
fn dynamic_home_badge_and_no_demo_data() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let c = parser::catalog(
        &page(
            r#"{latestEpisodes:[{media:{id:5,slug:'test',title:'Prueba'},number:3}],latestMedia:[{id:6,slug:'other',title:'Otra prueba'}]}"#,
        ),
        &d,
        true,
    )?;
    assert_eq!(c.sections.len(), 2);
    assert_eq!(c.sections[0].1[0].badge.as_deref(), Some("EP 3"));
    Ok(())
}
#[test]
fn dom_fallback_and_empty_search() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let c = parser::catalog(
        r#"<a href='/media/fixture'><img src='/cover.jpg' alt='Portada de Prueba'></a>"#,
        &d,
        false,
    )?;
    assert_eq!(c.sections[0].1[0].title, "Prueba");
    let empty = parser::catalog(&page("{results:[]}"), &d, false)?;
    assert!(empty.sections.is_empty());
    assert!(parser::catalog("<html>Access denied</html>", &d, false).is_err());
    Ok(())
}
#[test]
fn unsafe_schemes_and_malformed_external_input() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    assert!(parser::sources(
        &page(r#"{embeds:{SUB:[{server:'bad',url:'file:///etc/passwd'}]}}"#),
        &d
    )
    .is_err());
    for bad in [
        "",
        "node_ids",
        "<script>node_ids {episodes:[{</script>",
        "<script>node_ids {media:{title:'bad}}</script>",
    ] {
        assert!(parser::detail(bad, &d).is_err());
    }
    Ok(())
}
#[test]
fn search_and_genre_are_url_encoded() -> anyhow::Result<()> {
    let d = extension_manager::bundled()?;
    let b = Browse {
        search: "a & b / ñ".into(),
        genre: "3".into(),
        page: 2,
        ..Default::default()
    };
    let u = url::Url::parse(&parser::browse_url(&d, &b)?)?;
    let pairs: std::collections::HashMap<_, _> = u.query_pairs().collect();
    assert_eq!(pairs.get("search").map(|s| s.as_ref()), Some("a & b / ñ"));
    assert_eq!(pairs.get("genre").map(|s| s.as_ref()), Some("3"));
    Ok(())
}
