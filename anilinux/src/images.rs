use crate::{network, storage};
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::Semaphore};

pub struct Images {
    textures: HashMap<String, TextureHandle>,
    order: VecDeque<String>,
    pending: HashSet<String>,
    failed: HashMap<String, Instant>,
    tx: mpsc::Sender<(String, Option<ColorImage>)>,
    rx: mpsc::Receiver<(String, Option<ColorImage>)>,
    limit: Arc<Semaphore>,
}
impl Default for Images {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            textures: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            tx,
            rx,
            limit: Arc::new(Semaphore::new(6)),
        }
    }
}
impl Images {
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((url, image)) = self.rx.try_recv() {
            self.pending.remove(&url);
            if let Some(image) = image {
                while self.textures.len() >= 96 {
                    if let Some(old) = self.order.pop_front() {
                        self.textures.remove(&old);
                    } else {
                        break;
                    }
                }
                self.textures.insert(
                    url.clone(),
                    ctx.load_texture(storage::hash(&url), image, TextureOptions::LINEAR),
                );
                self.order.push_back(url);
            } else {
                self.failed.insert(url, Instant::now());
            }
        }
    }
    pub fn get(
        &mut self,
        url: &str,
        ctx: &egui::Context,
        rt: &Handle,
        client: &reqwest::Client,
    ) -> Option<TextureHandle> {
        if let Some(texture) = self.textures.get(url) {
            self.order.retain(|s| s != url);
            self.order.push_back(url.into());
            return Some(texture.clone());
        }
        if url.is_empty()
            || self.pending.contains(url)
            || self
                .failed
                .get(url)
                .is_some_and(|t| t.elapsed() < Duration::from_secs(120))
        {
            return None;
        }
        self.pending.insert(url.into());
        let (url, ctx, tx, client, limit) = (
            url.to_owned(),
            ctx.clone(),
            self.tx.clone(),
            client.clone(),
            self.limit.clone(),
        );
        rt.spawn(async move {
            let result: anyhow::Result<ColorImage> = async {
                let _permit = limit.acquire().await?;
                let path = storage::cache_dir().join(format!("{}.img", storage::hash(&url)));
                let cached = tokio::fs::read(&path).await.ok();
                let bytes = match cached {
                    Some(ref b) => b.clone(),
                    None => {
                        network::bytes(&client, &url, "https://animeav1.com/", 8 * 1024 * 1024)
                            .await?
                    }
                };
                let decode_bytes = bytes.clone();
                let image = tokio::task::spawn_blocking(move || -> anyhow::Result<ColorImage> {
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(decode_bytes))
                        .with_guessed_format()?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(8192);
                    limits.max_image_height = Some(8192);
                    limits.max_alloc = Some(96 * 1024 * 1024);
                    reader.limits(limits);
                    let rgba = reader.decode()?.thumbnail(384, 384).to_rgba8();
                    Ok(ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    ))
                })
                .await??;
                if cached.is_none() {
                    let _ = storage::atomic_write(&path, &bytes).await;
                }
                Ok(image)
            }
            .await;
            let _ = tx.send((url, result.ok()));
            ctx.request_repaint();
        });
        None
    }
}
/// Disk LRU-ish eviction by modification time. Off the UI thread.
pub async fn trim_cache() -> anyhow::Result<()> {
    let root = storage::cache_dir();
    tokio::fs::create_dir_all(&root).await?;
    let mut entries = tokio::fs::read_dir(root).await?;
    let mut files = Vec::new();
    let mut total = 0;
    while let Some(entry) = entries.next_entry().await? {
        let meta = entry.metadata().await?;
        if meta.is_file() {
            total += meta.len();
            files.push((meta.modified().ok(), meta.len(), entry.path()));
        }
    }
    files.sort_by_key(|f| f.0);
    for (_, size, path) in files {
        if total <= 256 * 1024 * 1024 {
            break;
        }
        if tokio::fs::remove_file(path).await.is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}
