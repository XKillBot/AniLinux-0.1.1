use crate::{
    downloader,
    extension_manager::{self, Definition, Manifest},
    models::*,
    network, parser, player, resolver, storage,
};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
};
use tokio::{runtime::Handle, sync::Semaphore};
use tokio_util::sync::CancellationToken;

pub enum Event {
    Boot(Definition),
    Update(Option<Manifest>),
    UpdateFailed,
    Applied(anyhow::Result<Definition>),
    Catalog(u64, anyhow::Result<Catalog>),
    Detail(u64, anyhow::Result<Anime>),
    Sources(u64, anyhow::Result<Sources>),
    Quick(Anime, Episode, anyhow::Result<Sources>),
    Played(History, Result<player::Confirmation, player::Failure>),
    FilePlayed(Result<player::Confirmation, player::Failure>),
    Planned(String, PathBuf),
    Progress(String, Option<f32>),
    Finished(String, bool, bool),
    SaveFailed,
}
#[derive(Clone)]
pub struct Bus {
    pub tx: mpsc::Sender<Event>,
    pub ctx: egui::Context,
}
impl Bus {
    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}
#[derive(Clone)]
pub struct Tasks {
    pub handle: Handle,
    pub client: reqwest::Client,
    pub bus: Bus,
    pub downloads: Arc<Semaphore>,
    save_tx: tokio::sync::watch::Sender<Option<LocalState>>,
}
impl Tasks {
    pub fn new(handle: Handle, client: reqwest::Client, bus: Bus) -> Self {
        let (save_tx, mut rx) = tokio::sync::watch::channel::<Option<LocalState>>(None);
        let save_bus = bus.clone();
        handle.spawn(async move {
            while rx.changed().await.is_ok() {
                let snapshot = rx.borrow_and_update().clone();
                if let Some(state) = snapshot {
                    if storage::save(&state).await.is_err() {
                        save_bus.send(Event::SaveFailed);
                    }
                }
            }
        });
        Self {
            handle,
            client,
            bus,
            downloads: Arc::new(Semaphore::new(3)),
            save_tx,
        }
    }
    pub fn save(&self, state: &LocalState) {
        self.save_tx.send_replace(Some(state.clone()));
    }
    pub fn startup(&self) {
        let this = self.clone();
        self.handle.spawn(async move {
            let definition = extension_manager::load(&storage::data_dir()).await;
            if let Ok(d) = definition {
                this.bus.send(Event::Boot(d.clone()));
                match extension_manager::check(&this.client, &d).await {
                    Ok(update) => this.bus.send(Event::Update(update)),
                    Err(_) => this.bus.send(Event::UpdateFailed),
                }
            }
        });
        self.handle.spawn(async {
            let _ = crate::images::trim_cache().await;
        });
    }
    pub fn browse(&self, request: u64, d: Definition, browse: Browse) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result = async {
                let url = parser::browse_url(&d, &browse)?;
                let html = network::text(&this.client, &url, &d.base_url).await?;
                tokio::task::spawn_blocking(move || parser::catalog(&html, &d, browse.home)).await?
            }
            .await;
            this.bus.send(Event::Catalog(request, result));
        });
    }
    pub fn detail(&self, request: u64, d: Definition, url: String) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result = async {
                let html = network::text(&this.client, &url, &d.base_url).await?;
                tokio::task::spawn_blocking(move || parser::detail(&html, &d)).await?
            }
            .await;
            this.bus.send(Event::Detail(request, result));
        });
    }
    pub fn sources(&self, request: u64, d: Definition, episode: Episode, quick: Option<Anime>) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result = async {
                let html = network::text(&this.client, &episode.url, &d.base_url).await?;
                tokio::task::spawn_blocking(move || parser::sources(&html, &d)).await?
            }
            .await;
            if let Some(anime) = quick {
                this.bus.send(Event::Quick(anime, episode, result));
            } else {
                this.bus.send(Event::Sources(request, result));
            }
        });
    }
    pub fn play(
        &self,
        d: Definition,
        anime: Anime,
        episode: Episode,
        server: Server,
        preference: String,
    ) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result = async {
                let media = resolver::resolve(&this.client, &d, &server, &episode.url)
                    .await
                    .map_err(|e| {
                        eprintln!("AniLinux: no se pudo resolver el vídeo: {e:#}");
                        player::Failure::Resolve
                    })?;
                player::play(&media, &preference).await
            }
            .await;
            this.bus
                .send(Event::Played(History { anime, episode }, result));
        });
    }
    pub fn play_file(&self, path: PathBuf, preference: String) {
        let bus = self.bus.clone();
        self.handle.spawn(async move {
            bus.send(Event::FilePlayed(
                player::play_file(&path, &preference).await,
            ));
        });
    }
    #[allow(clippy::too_many_arguments)]
    pub fn download(
        &self,
        d: Definition,
        anime: Anime,
        episode: Episode,
        server: Server,
        language: String,
        root: PathBuf,
        key: String,
        cancel: CancellationToken,
    ) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result: anyhow::Result<()> = async {
                let _permit = tokio::select! {
                    permit = this.downloads.acquire() => permit?,
                    _ = cancel.cancelled() => anyhow::bail!("Cancelada"),
                };
                let media = tokio::select! {
                    media = resolver::resolve(&this.client, &d, &server, &episode.url) => media?,
                    _ = cancel.cancelled() => anyhow::bail!("Cancelada"),
                };
                let path = downloader::target(
                    &root,
                    &anime.title,
                    &format!("Episodio {}", episode.number),
                    &language,
                    &key,
                    &media,
                );
                this.bus.send(Event::Planned(key.clone(), path.clone()));
                let bus = this.bus.clone();
                let id = key.clone();
                downloader::run(media, &path, cancel.clone(), move |p| {
                    bus.send(Event::Progress(id.clone(), p))
                })
                .await
            }
            .await;
            this.bus
                .send(Event::Finished(key, result.is_ok(), cancel.is_cancelled()));
        });
    }
    pub fn apply(&self, manifest: Manifest) {
        let this = self.clone();
        self.handle.spawn(async move {
            let result =
                extension_manager::apply(&this.client, &storage::data_dir(), &manifest).await;
            this.bus.send(Event::Applied(result));
        });
    }
}
