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
impl Server {
    pub fn is_mp4upload(&self) -> bool {
        let name = self.server.trim().to_ascii_lowercase();
        let url = self.url.to_ascii_lowercase();
        name == "mp4upload" || name.contains("mp4upload") || url.contains("mp4upload.")
    }

    /// Indica si el servidor debe ocultarse en la interfaz por no ser funcional.
    /// Los servidores se conservan íntegros en `Sources` y en el parser; solo se ocultan visualmente.
    pub fn is_hidden_in_ui(&self) -> bool {
        if self.is_mp4upload() {
            return false;
        }
        let name = self.server.trim().to_ascii_lowercase();
        let url = self.url.to_ascii_lowercase();
        let clean_url = url.split(['?', '#']).next().unwrap_or("");
        if [".mp4", ".mkv", ".webm", ".ogv", ".mov", ".m4v"]
            .iter()
            .any(|ext| clean_url.ends_with(ext))
        {
            return false;
        }
        matches!(
            name.as_str(),
            "hls"
                | "voe"
                | "byse"
                | "bysezejataos"
                | "upnshare"
                | "transferit"
                | "mega"
                | "1fichier"
                | "pixeldrain"
                | "mediafire"
                | "streamtape"
                | "doodstream"
                | "filemoon"
                | "streamwish"
                | "yourupload"
                | "okru"
                | "fembed"
        ) || url.contains("zilla-networks.")
            || url.contains("transfer.it")
            || url.contains("transferit.")
            || url.contains("mega.nz")
            || url.contains("1fichier.")
            || url.contains("voe.")
            || url.contains("byse")
            || url.contains("upnshare")
    }
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

    /// Devuelve los servidores visibles para una pestaña e idioma, ocultando los que no funcionan
    /// y priorizando siempre `MP4Upload` (con respaldo cruzado entre `embeds` y `downloads`).
    pub fn visible_servers(&self, lang: &str, download_tab: bool) -> Vec<Server> {
        let (primary, secondary) = if download_tab {
            (self.downloads.get(lang), self.embeds.get(lang))
        } else {
            (self.embeds.get(lang), self.downloads.get(lang))
        };
        let mut list: Vec<Server> = primary
            .into_iter()
            .flatten()
            .filter(|s| !s.is_hidden_in_ui())
            .cloned()
            .collect();
        if !list.iter().any(Server::is_mp4upload) {
            if let Some(mp4) = secondary
                .into_iter()
                .flatten()
                .find(|s| s.is_mp4upload())
                .cloned()
            {
                list.insert(0, mp4);
            } else if list.is_empty() {
                list.extend(
                    secondary
                        .into_iter()
                        .flatten()
                        .filter(|s| !s.is_hidden_in_ui())
                        .cloned(),
                );
            }
        }
        list.sort_by_key(|s| !s.is_mp4upload());
        list
    }

    pub fn preferred(&self, lang: &str) -> Option<Server> {
        self.visible_servers(lang, false)
            .into_iter()
            .next()
            .or_else(|| self.visible_servers(lang, true).into_iter().next())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_broken_servers_and_selects_mp4upload_across_tabs() {
        let mut s = Sources::default();
        s.embeds.insert(
            "SUB".into(),
            vec![
                Server {
                    server: "HLS".into(),
                    url: "https://player.zilla-networks.com/play/0123456789abcdef0123456789abcdef"
                        .into(),
                },
                Server {
                    server: "Voe".into(),
                    url: "https://voe.sx/e/abc".into(),
                },
                Server {
                    server: "MP4Upload".into(),
                    url: "https://www.mp4upload.com/embed-abc.html".into(),
                },
            ],
        );
        s.downloads.insert(
            "SUB".into(),
            vec![
                Server {
                    server: "TransferIt".into(),
                    url: "https://transfer.it/t/abc".into(),
                },
                Server {
                    server: "MEGA".into(),
                    url: "https://mega.nz/file/abc".into(),
                },
            ],
        );
        // Los datos originales permanecen íntegros en memoria
        assert_eq!(s.embeds["SUB"].len(), 3);
        assert_eq!(s.downloads["SUB"].len(), 2);

        // Tanto en línea como en descargas se ocultan los rotos y se obtiene MP4Upload
        let online = s.visible_servers("SUB", false);
        assert_eq!(online.len(), 1);
        assert!(online[0].is_mp4upload());

        let dl = s.visible_servers("SUB", true);
        assert_eq!(dl.len(), 1);
        assert!(dl[0].is_mp4upload());

        assert!(s.preferred("SUB").is_some_and(|srv| srv.is_mp4upload()));
    }
}
