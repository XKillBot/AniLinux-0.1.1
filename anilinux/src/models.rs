use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Anime {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub url: String,
    pub cover: String,
    pub banner: String,
    pub synopsis: String,
    pub year: Option<String>,
    pub kind: Option<String>,
    pub genres: Vec<String>,
    pub episodes: Vec<Episode>,
    pub badge: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub number: f64,
    pub title: String,
    pub url: String,
    pub thumbnail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Server {
    pub server: String,
    pub url: String,
}
#[derive(Clone, Debug, Default)]
pub struct Sources {
    pub embeds: BTreeMap<String, Vec<Server>>,
    pub downloads: BTreeMap<String, Vec<Server>>,
}
impl Sources {
    pub fn languages(&self) -> Vec<String> {
        let mut langs: Vec<_> = self
            .embeds
            .iter()
            .chain(self.downloads.iter())
            .filter(|(_, links)| !links.is_empty())
            .map(|(lang, _)| lang.clone())
            .collect();
        langs.sort_by_key(|l| (l != "SUB", l.clone()));
        langs.dedup();
        langs
    }
    pub fn preferred(&self, lang: &str) -> Option<Server> {
        self.embeds
            .get(lang)
            .and_then(|v| {
                v.iter()
                    .find(|s| s.server.eq_ignore_ascii_case("HLS"))
                    .or(v.first())
            })
            .or_else(|| self.downloads.get(lang).and_then(|v| v.first()))
            .cloned()
    }
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub sections: Vec<(String, Vec<Anime>)>,
    pub genres: Vec<(String, String)>,
    pub page: u64,
    pub total_pages: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Browse {
    pub home: bool,
    pub search: String,
    pub page: u64,
    pub genre: String,
    pub airing: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub player: String,
}
impl Default for Settings {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            download_dir: dirs::video_dir()
                .unwrap_or_else(|| home.join("Videos"))
                .join("AniLinux"),
            player: "auto".into(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalState {
    pub settings: Settings,
    pub searches: Vec<String>,
    pub history: Vec<History>,
    pub downloads: Vec<Download>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    pub anime: Anime,
    pub episode: Episode,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub key: String,
    pub title: String,
    pub episode: String,
    pub language: String,
    pub path: PathBuf,
    pub progress: Option<f32>,
    pub status: DownloadStatus,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}
