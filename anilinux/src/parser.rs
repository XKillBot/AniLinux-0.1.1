//! Reads a safe subset of JS literals; never evaluates a script. Shared JS object
//! references (a, b, void 0) are tolerated as null. Known values remain intact.
use crate::{extension_manager::Definition, models::*, network};
use anyhow::{bail, ensure, Result};
use scraper::{Html, Selector};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug)]
enum Token {
    Word(String),
    Text(String),
    Number(String),
    Punct(char),
}
fn lex(input: &str) -> Result<Vec<Token>> {
    ensure!(input.len() <= 8 * 1024 * 1024, "Datos demasiado grandes");
    let b = input.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_whitespace() => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && &b[i..i + 2] != b"*/" {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            q @ (b'\'' | b'"') => {
                let start = i;
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i = (i + 2).min(b.len());
                        continue;
                    }
                    if b[i] == q {
                        break;
                    }
                    i += 1;
                }
                ensure!(i < b.len(), "Texto incompleto");
                i += 1;
                let text: String = json5::from_str(&input[start..i])?;
                out.push(Token::Text(text));
            }
            b'`' => {
                // Template code is deliberately not interpreted.
                i += 1;
                while i < b.len() && b[i] != b'`' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(b.len());
                out.push(Token::Word("undefined".into()));
            }
            c if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b"_$".contains(&b[i])) {
                    i += 1;
                }
                out.push(Token::Word(input[start..i].into()));
            }
            c if c.is_ascii_digit()
                || (c == b'-' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) =>
            {
                let start = i;
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b".eE+-".contains(&b[i])) {
                    i += 1;
                }
                out.push(Token::Number(input[start..i].into()));
            }
            c => {
                out.push(Token::Punct(c as char));
                i += 1;
            }
        }
    }
    Ok(out)
}
fn value(tokens: &[Token], pos: &mut usize, depth: usize) -> Result<Value> {
    ensure!(depth < 96, "Estructura demasiado profunda");
    let token = tokens
        .get(*pos)
        .ok_or_else(|| anyhow::anyhow!("Datos incompletos"))?;
    *pos += 1;
    Ok(match token {
        Token::Text(s) => Value::String(s.clone()),
        Token::Number(s) => Value::Number(s.parse::<Number>()?),
        Token::Word(s) => match s.as_str() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "void" => {
                if matches!(tokens.get(*pos), Some(Token::Number(_))) {
                    *pos += 1;
                }
                Value::Null
            }
            _ => Value::Null,
        },
        Token::Punct(open @ ('{' | '[')) => {
            let close = if *open == '{' { '}' } else { ']' };
            let mut arr = Vec::new();
            let mut obj = Map::new();
            loop {
                match tokens.get(*pos) {
                    Some(Token::Punct(c)) if *c == close => {
                        *pos += 1;
                        break;
                    }
                    Some(Token::Punct(',')) => {
                        *pos += 1;
                        continue;
                    }
                    None => bail!("Contenedor incompleto"),
                    _ => {}
                }
                if *open == '{' {
                    let key = match tokens.get(*pos) {
                        Some(Token::Word(s) | Token::Text(s) | Token::Number(s)) => s.clone(),
                        _ => bail!("Propiedad no admitida"),
                    };
                    *pos += 1;
                    ensure!(
                        matches!(tokens.get(*pos), Some(Token::Punct(':'))),
                        "Falta separador"
                    );
                    *pos += 1;
                    obj.insert(key, value(tokens, pos, depth + 1)?);
                } else {
                    arr.push(value(tokens, pos, depth + 1)?);
                }
                ensure!(
                    matches!(tokens.get(*pos), Some(Token::Punct(c)) if *c == ',' || *c == close),
                    "Literal no admitido"
                );
            }
            if *open == '{' {
                Value::Object(obj)
            } else {
                Value::Array(arr)
            }
        }
        _ => bail!("Valor no admitido"),
    })
}
fn properties(tokens: &[Token], key: &str) -> Vec<Value> {
    tokens
        .windows(2)
        .enumerate()
        .filter_map(|(i, t)| match (&t[0], &t[1]) {
            (Token::Word(s) | Token::Text(s), Token::Punct(':')) if s == key => {
                value(tokens, &mut (i + 2), 0).ok()
            }
            _ => None,
        })
        .collect()
}
fn first(tokens: &[Token], key: &str) -> Value {
    properties(tokens, key)
        .into_iter()
        .next()
        .unwrap_or(Value::Null)
}
fn script(html: &str, d: &Definition) -> Result<String> {
    let doc = Html::parse_document(html);
    let selector =
        Selector::parse(&d.selectors.scripts).map_err(|_| anyhow::anyhow!("Fuente no válida"))?;
    let result = doc
        .select(&selector)
        .map(|s| s.text().collect::<String>())
        .filter(|s| s.contains(&d.script_marker))
        .collect::<Vec<_>>()
        .join("\n");
    ensure!(
        !result.is_empty(),
        "La fuente no ha enviado los datos esperados"
    );
    Ok(result)
}
fn text(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn asset(template: &str, id: &str, number: &str) -> String {
    template.replace("{id}", id).replace("{number}", number)
}
fn join(d: &Definition, path: &str) -> Result<String> {
    Ok(network::web_url(&d.base_url)?.join(path)?.to_string())
}
fn anime(v: &Value, d: &Definition) -> Option<Anime> {
    let id = text(v, "id");
    let slug = text(v, "slug");
    let title = text(v, "title");
    if slug.is_empty() || title.is_empty() {
        return None;
    }
    let url = join(d, &d.routes.media.replace("{slug}", &slug)).ok()?;
    let mut item = Anime {
        id: id.clone(),
        slug,
        title,
        url,
        cover: asset(&d.assets.cover, &id, ""),
        banner: asset(&d.assets.banner, &id, ""),
        synopsis: text(v, "synopsis"),
        ..Default::default()
    };
    let date = text(v, "startDate");
    item.year = date.get(..4).map(str::to_owned);
    item.kind = v["category"]["name"].as_str().map(str::to_owned);
    item.genres = v["genres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| g["name"].as_str().map(str::to_owned))
        .collect();
    item.episodes = episodes(&v[&d.fields.episodes], &item, d);
    Some(item)
}
fn episodes(v: &Value, a: &Anime, d: &Definition) -> Vec<Episode> {
    let mut result = Vec::new();
    for e in v.as_array().into_iter().flatten() {
        let Some(number) = e["number"]
            .as_f64()
            .or_else(|| e["number"].as_str().and_then(|s| s.parse().ok()))
        else {
            continue;
        };
        if !number.is_finite() || number < 0.0 {
            continue;
        }
        let n = number.to_string();
        if let Ok(url) = join(
            d,
            &d.routes
                .episode
                .replace("{slug}", &a.slug)
                .replace("{number}", &n),
        ) {
            result.push(Episode {
                id: text(e, "id"),
                number,
                title: text(e, "title"),
                url,
                thumbnail: asset(&d.assets.thumbnail, &a.id, &n),
            });
        }
    }
    result.sort_by(|a, b| a.number.total_cmp(&b.number));
    result.dedup_by(|a, b| a.number == b.number);
    result
}
fn unique(items: Vec<Anime>) -> Vec<Anime> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter(|a| seen.insert(a.slug.clone()))
        .collect()
}
fn dom_cards(html: &str, d: &Definition) -> Result<Vec<Anime>> {
    let doc = Html::parse_document(html);
    let links =
        Selector::parse(&d.selectors.cards).map_err(|_| anyhow::anyhow!("Fuente no válida"))?;
    let images =
        Selector::parse(&d.selectors.image).map_err(|_| anyhow::anyhow!("Fuente no válida"))?;
    let mut result = Vec::new();
    for a in doc.select(&links) {
        let Some(href) = a.value().attr("href") else {
            continue;
        };
        let Ok(url) = network::web_url(&d.base_url)?.join(href) else {
            continue;
        };
        if url.host_str() != network::web_url(&d.base_url)?.host_str() {
            continue;
        }
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if parts.len() != 2 || parts[0] != "media" {
            continue;
        }
        let image = a.select(&images).next();
        let mut title = a.value().attr("title").unwrap_or("").to_owned();
        if title.is_empty() {
            title = a
                .text()
                .collect::<String>()
                .trim()
                .trim_start_matches("Ver ")
                .to_string();
        }
        if title.is_empty() {
            title = image
                .and_then(|i| i.value().attr("alt"))
                .unwrap_or("")
                .trim_start_matches("Portada de ")
                .to_string();
        }
        if title.is_empty() {
            continue;
        }
        let cover = image
            .and_then(|i| i.value().attr("src"))
            .and_then(|src| url.join(src).ok())
            .map(|u| u.to_string())
            .unwrap_or_default();
        result.push(Anime {
            title,
            slug: parts[1].into(),
            cover,
            url: url.to_string(),
            ..Default::default()
        });
    }
    Ok(unique(result))
}
pub fn catalog(html: &str, d: &Definition, home: bool) -> Result<Catalog> {
    let tokens = script(html, d).and_then(|s| lex(&s)).unwrap_or_default();
    let mut result = Catalog {
        page: 1,
        total_pages: 1,
        ..Default::default()
    };
    let p = first(&tokens, &d.fields.pagination);
    result.page = p["currentPage"].as_u64().unwrap_or(1);
    result.total_pages = p["totalPages"].as_u64().unwrap_or(1);
    let genres = first(&tokens, &d.fields.genres);
    if let Some(map) = genres.as_object() {
        result.genres = map
            .iter()
            .filter_map(|(id, g)| {
                Some((
                    g["slug"].as_str().unwrap_or(id).to_owned(),
                    g["name"].as_str()?.into(),
                ))
            })
            .collect();
        result.genres.sort_by(|a, b| a.1.cmp(&b.1));
    }
    if home {
        let recent = first(&tokens, &d.fields.recent)
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                let mut a = anime(&e[&d.fields.media], d)?;
                if let Some(n) = e["number"].as_f64() {
                    a.badge = Some(format!("EP {n}"));
                }
                Some(a)
            })
            .collect();
        result
            .sections
            .push(("Últimos episodios".into(), unique(recent)));
        let latest = first(&tokens, &d.fields.latest)
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| anime(v, d))
            .collect();
        result
            .sections
            .push(("Recién agregados".into(), unique(latest)));
    } else {
        let items = first(&tokens, &d.fields.results)
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| anime(v, d))
            .collect();
        result
            .sections
            .push(("Explorar catálogo".into(), unique(items)));
    }
    result.sections.retain(|(_, items)| !items.is_empty());
    if result.sections.is_empty() {
        let cards = dom_cards(html, d)?;
        if !cards.is_empty() {
            result.sections.push(("Explorar".into(), cards));
        } else if tokens.is_empty() {
            bail!("No se pudo leer el catálogo. Intenta actualizar la fuente");
        }
    }
    Ok(result)
}
pub fn detail(html: &str, d: &Definition) -> Result<Anime> {
    let s = script(html, d)?;
    let tokens = lex(&s)?;
    let mut a = properties(&tokens, &d.fields.media)
        .iter()
        .find_map(|v| anime(v, d))
        .ok_or_else(|| anyhow::anyhow!("No se pudo leer esta ficha"))?;
    if a.episodes.is_empty() {
        a.episodes = episodes(&first(&tokens, &d.fields.episodes), &a, d);
    }
    if a.episodes.is_empty() {
        if let Some(pattern) = d.patterns.get("episodes") {
            let re = regex::Regex::new(pattern)?;
            let values: Vec<_> = re
                .captures_iter(&s)
                .filter_map(|c| {
                    Some(
                        serde_json::json!({"id": c.get(1)?.as_str(), "number": c.get(2)?.as_str()}),
                    )
                })
                .collect();
            a.episodes = episodes(&Value::Array(values), &a, d);
        }
    }
    Ok(a)
}
fn servers(v: Value) -> BTreeMap<String, Vec<Server>> {
    let mut result = BTreeMap::new();
    for (lang, list) in v.as_object().into_iter().flatten() {
        if lang != "SUB" && lang != "DUB" {
            continue;
        }
        let links: Vec<Server> = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| serde_json::from_value::<Server>(v.clone()).ok())
            .filter(|s| !s.server.trim().is_empty() && network::web_url(&s.url).is_ok())
            .collect();
        if !links.is_empty() {
            result.insert(lang.clone(), links);
        }
    }
    result
}
pub fn sources(html: &str, d: &Definition) -> Result<Sources> {
    let s = script(html, d)?;
    let tokens = lex(&s)?;
    let mut result = Sources {
        embeds: servers(first(&tokens, &d.fields.embeds)),
        downloads: servers(first(&tokens, &d.fields.downloads)),
    };
    // Compatibility fallback for remote v1 patterns; balanced parsing above is
    // preferred so DUB does not need to be the first object property.
    if let Some(pair) = d.patterns.get("pair") {
        let pair = regex::Regex::new(pair)?;
        for (prefix, target) in [
            ("embeds", &mut result.embeds),
            ("downloads", &mut result.downloads),
        ] {
            for lang in ["SUB", "DUB"] {
                if target.contains_key(lang) {
                    continue;
                }
                if let Some(pattern) = d.patterns.get(&format!("{prefix}_{}", lang.to_lowercase()))
                {
                    let re = regex::RegexBuilder::new(pattern)
                        .dot_matches_new_line(true)
                        .build()?;
                    if let Some(block) = re.captures(&s).and_then(|c| c.get(1)) {
                        let links: Vec<_> = pair
                            .captures_iter(block.as_str())
                            .filter_map(|c| {
                                let url: String =
                                    json5::from_str(&format!("\"{}\"", c.get(2)?.as_str())).ok()?;
                                network::web_url(&url).ok()?;
                                Some(Server {
                                    server: c.get(1)?.as_str().into(),
                                    url,
                                })
                            })
                            .collect();
                        if !links.is_empty() {
                            target.insert(lang.into(), links);
                        }
                    }
                }
            }
        }
    }
    ensure!(
        !result.languages().is_empty(),
        "Este episodio aún no tiene servidores disponibles"
    );
    Ok(result)
}
pub fn browse_url(d: &Definition, b: &Browse) -> Result<String> {
    let path = if b.home { "/" } else { &d.routes.catalog };
    let mut u = network::web_url(&d.base_url)?.join(path)?;
    if !b.home {
        let mut q = u.query_pairs_mut();
        q.append_pair(&d.routes.page_param, &b.page.max(1).to_string());
        if !b.search.is_empty() {
            q.append_pair(&d.routes.search_param, &b.search);
        }
        if !b.genre.is_empty() {
            q.append_pair(&d.routes.genre_param, &b.genre);
        }
        if b.airing {
            q.append_pair(&d.routes.status_param, &d.routes.airing_value);
        }
    }
    Ok(u.to_string())
}
