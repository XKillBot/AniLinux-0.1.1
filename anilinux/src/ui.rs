use crate::{
    downloader,
    extension_manager::{self, Definition, Manifest},
    images::Images,
    models::*,
    network, storage,
    tasks::{Bus, Event, Tasks},
};
use egui::{Color32, CornerRadius, Margin, Pos2, Rect, RichText, Shadow, Stroke, StrokeKind, Vec2};
use std::{
    collections::{HashMap, HashSet},
    f32::consts::PI,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

const BG: Color32 = Color32::from_rgb(13, 15, 27);
const PANEL: Color32 = Color32::from_rgb(18, 22, 38);
const CARD: Color32 = Color32::from_rgb(25, 30, 50);
const CARD_HOVER: Color32 = Color32::from_rgb(33, 40, 66);
const INPUT_BG: Color32 = Color32::from_rgb(15, 18, 31);
const BORDER: Color32 = Color32::from_rgb(44, 53, 84);
const CYAN: Color32 = Color32::from_rgb(0, 226, 210);
const AZURE: Color32 = Color32::from_rgb(56, 139, 253);
const VIOLET: Color32 = Color32::from_rgb(124, 92, 255);
const EMERALD: Color32 = Color32::from_rgb(40, 210, 148);
const AMBER: Color32 = Color32::from_rgb(245, 180, 75);
const CORAL: Color32 = Color32::from_rgb(255, 112, 118);
const TEXT_MAIN: Color32 = Color32::from_rgb(238, 242, 252);
const MUTED: Color32 = Color32::from_rgb(148, 157, 188);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Catalog,
    Releases,
    Downloads,
    Settings,
    Detail,
}

struct Picker {
    anime: Anime,
    episode: Episode,
    sources: Option<Sources>,
    language: String,
    index: usize,
    download_tab: bool,
}

pub struct AniLinux {
    tasks: Tasks,
    rx: mpsc::Receiver<Event>,
    definition: Definition,
    state: LocalState,
    final_state: Arc<Mutex<LocalState>>,
    page: Page,
    browse: Browse,
    catalog: Catalog,
    detail: Option<Anime>,
    picker: Option<Picker>,
    search: String,
    folder: String,
    direct_open: bool,
    direct_url: String,
    direct_title: String,
    images: Images,
    catalog_seq: u64,
    detail_seq: u64,
    sources_seq: u64,
    loading: bool,
    detail_loading: bool,
    update: Option<Manifest>,
    applying: bool,
    update_notice: String,
    notice: Option<(String, Instant)>,
    error: Option<String>,
    jobs: HashMap<String, CancellationToken>,
    quick_pending: HashSet<String>,
    playing: bool,
    show_episodes: bool,
    descending: bool,
    dirty: bool,
    saved: Instant,
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

fn soft_shadow() -> Shadow {
    Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(4, 6, 15, 115),
    }
}

fn paint_gradient_rect(
    painter: &egui::Painter,
    rect: Rect,
    radius: f32,
    c_start: Color32,
    c_end: Color32,
) {
    if rect.width() <= 0.5 || rect.height() <= 0.5 {
        return;
    }
    let r = radius.clamp(0.0, rect.width().min(rect.height()) * 0.5);
    let mut perimeter = Vec::with_capacity(32);
    if r < 0.5 {
        perimeter.extend_from_slice(&[
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ]);
    } else {
        const STEPS: usize = 6;
        let corners = [
            (
                egui::pos2(rect.right() - r, rect.bottom() - r),
                0.0,
                0.5 * PI,
            ),
            (egui::pos2(rect.left() + r, rect.bottom() - r), 0.5 * PI, PI),
            (egui::pos2(rect.left() + r, rect.top() + r), PI, 1.5 * PI),
            (
                egui::pos2(rect.right() - r, rect.top() + r),
                1.5 * PI,
                2.0 * PI,
            ),
        ];
        for (center, a0, a1) in corners {
            for step in 0..=STEPS {
                let frac = step as f32 / STEPS as f32;
                let angle = a0 + (a1 - a0) * frac;
                perimeter.push(egui::pos2(
                    center.x + r * angle.cos(),
                    center.y + r * angle.sin(),
                ));
            }
        }
    }
    let color_at = |p: Pos2| {
        let tx = ((p.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        let ty = ((p.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        lerp_color(c_start, c_end, 0.74 * tx + 0.26 * ty)
    };
    let mut mesh = egui::Mesh::default();
    let center = rect.center();
    mesh.colored_vertex(center, color_at(center));
    for &p in &perimeter {
        mesh.colored_vertex(p, color_at(p));
    }
    let n = perimeter.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + ((i + 1) % n));
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn gradient_button(
    ui: &mut egui::Ui,
    label: &str,
    c0: Color32,
    c1: Color32,
    dark_text: bool,
    enabled: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(14.5);
    let text_color = if !enabled {
        MUTED
    } else if dark_text {
        Color32::from_rgb(8, 16, 26)
    } else {
        Color32::WHITE
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let size = egui::vec2((galley.size().x + 32.0).max(96.0), 38.0);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hover = ui
        .ctx()
        .animate_bool_responsive(response.id, enabled && response.hovered());
    if ui.is_rect_visible(rect) {
        if enabled {
            let glow_alpha = (36.0 + hover * 52.0) as u8;
            let glow_rect = rect.expand2(egui::vec2(1.0, 1.5));
            ui.painter().rect_filled(
                glow_rect.translate(egui::vec2(0.0, 2.0)),
                19.0,
                Color32::from_rgba_unmultiplied(c0.r(), c0.g(), c0.b(), glow_alpha / 2),
            );
            let start = lerp_color(c0, Color32::WHITE, hover * 0.14);
            let end = lerp_color(c1, Color32::WHITE, hover * 0.12);
            paint_gradient_rect(ui.painter(), rect, 19.0, start, end);
            ui.painter().rect_stroke(
                rect,
                19.0,
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, (42.0 + hover * 48.0) as u8),
                ),
                StrokeKind::Inside,
            );
        } else {
            ui.painter()
                .rect_filled(rect, 19.0, Color32::from_rgb(32, 37, 58));
            ui.painter()
                .rect_stroke(rect, 19.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        }
        let text_pos = egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(text_pos, galley, text_color);
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

fn pill_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let font = egui::FontId::proportional(13.5);
    let fg = if selected { Color32::WHITE } else { TEXT_MAIN };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let size = egui::vec2((galley.size().x + 24.0).max(58.0), 32.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_responsive(response.id, response.hovered());
    if ui.is_rect_visible(rect) {
        if selected {
            let s = lerp_color(AZURE, Color32::WHITE, hover * 0.1);
            let e = lerp_color(VIOLET, Color32::WHITE, hover * 0.1);
            paint_gradient_rect(ui.painter(), rect, 16.0, s, e);
            ui.painter().rect_stroke(
                rect,
                16.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(180, 220, 255, 95)),
                StrokeKind::Inside,
            );
        } else {
            let fill = lerp_color(CARD, CARD_HOVER, hover);
            let border = lerp_color(BORDER, CYAN, hover * 0.55);
            ui.painter().rect_filled(rect, 16.0, fill);
            ui.painter()
                .rect_stroke(rect, 16.0, Stroke::new(1.0, border), StrokeKind::Inside);
        }
        let pos = egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(pos, galley, fg);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn nav_button(ui: &mut egui::Ui, label: &str, active: bool, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 44.0), egui::Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_responsive(response.id, response.hovered());
    if ui.is_rect_visible(rect) {
        if active {
            paint_gradient_rect(
                ui.painter(),
                rect,
                12.0,
                Color32::from_rgb(19, 55, 76),
                Color32::from_rgb(29, 39, 74),
            );
            ui.painter().rect_stroke(
                rect,
                12.0,
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 226, 210, 95)),
                StrokeKind::Inside,
            );
            let bar = Rect::from_min_size(
                egui::pos2(rect.left() + 6.0, rect.center().y - 10.0),
                egui::vec2(4.0, 20.0),
            );
            paint_gradient_rect(ui.painter(), bar, 2.0, CYAN, AZURE);
        } else if hover > 0.01 {
            ui.painter().rect_filled(
                rect,
                12.0,
                lerp_color(PANEL, Color32::from_rgb(28, 34, 58), hover),
            );
        }
        let color = if active {
            CYAN
        } else {
            lerp_color(Color32::from_rgb(206, 213, 235), Color32::WHITE, hover)
        };
        ui.painter().text(
            egui::pos2(rect.left() + 20.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(15.0),
            color,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn badge_pill(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(99))
        .inner_margin(Margin::symmetric(10, 4))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 70),
        ))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).strong().color(fg));
        });
}

fn gradient_progress_bar(ui: &mut egui::Ui, progress: f32) {
    let p = progress.clamp(0.0, 1.0);
    let width = ui.available_width().max(80.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 12.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 6.0, INPUT_BG);
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        let fill_w = (rect.width() * p).max(if p > 0.0 { 10.0 } else { 0.0 });
        if fill_w > 0.5 {
            let fill_rect = Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
            paint_gradient_rect(ui.painter(), fill_rect, 6.0, CYAN, AZURE);
        }
    }
}

impl AniLinux {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        handle: Handle,
        client: reqwest::Client,
        state: LocalState,
        final_state: Arc<Mutex<LocalState>>,
    ) -> anyhow::Result<Self> {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT_MAIN);
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.window_corner_radius = CornerRadius::same(18);
        visuals.window_shadow = soft_shadow();
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.menu_corner_radius = CornerRadius::same(12);
        visuals.popup_shadow = soft_shadow();
        visuals.extreme_bg_color = INPUT_BG;
        visuals.selection.bg_fill = Color32::from_rgb(24, 78, 126);
        visuals.selection.stroke = Stroke::new(1.0, CYAN);

        let r12 = CornerRadius::same(12);
        visuals.widgets.noninteractive.bg_fill = CARD;
        visuals.widgets.noninteractive.weak_bg_fill = CARD;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.noninteractive.corner_radius = r12;

        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.inactive.weak_bg_fill = CARD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_MAIN);
        visuals.widgets.inactive.corner_radius = r12;

        visuals.widgets.hovered.bg_fill = CARD_HOVER;
        visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, CYAN);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.2, Color32::WHITE);
        visuals.widgets.hovered.corner_radius = r12;
        visuals.widgets.hovered.expansion = 1.0;

        visuals.widgets.active.bg_fill = Color32::from_rgb(20, 92, 120);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(20, 92, 120);
        visuals.widgets.active.bg_stroke = Stroke::new(1.2, CYAN);
        visuals.widgets.active.fg_stroke = Stroke::new(1.2, Color32::WHITE);
        visuals.widgets.active.corner_radius = r12;

        visuals.widgets.open.bg_fill = CARD_HOVER;
        visuals.widgets.open.weak_bg_fill = CARD_HOVER;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, AZURE);
        visuals.widgets.open.corner_radius = r12;

        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(12.0, 12.0);
        style.spacing.button_padding = egui::vec2(16.0, 9.0);
        style.spacing.window_margin = Margin::same(20);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.5));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
        cc.egui_ctx.set_style(style);
        let (tx, rx) = mpsc::channel();
        let tasks = Tasks::new(
            handle,
            client,
            Bus {
                tx,
                ctx: cc.egui_ctx.clone(),
            },
        );
        tasks.startup();
        Ok(Self {
            folder: state.settings.download_dir.to_string_lossy().into_owned(),
            tasks,
            rx,
            definition: extension_manager::bundled()?,
            state,
            final_state,
            page: Page::Home,
            browse: Browse {
                home: true,
                page: 1,
                ..Default::default()
            },
            catalog: Catalog::default(),
            detail: None,
            picker: None,
            search: String::new(),
            direct_open: false,
            direct_url: String::new(),
            direct_title: String::new(),
            images: Images::default(),
            catalog_seq: 0,
            detail_seq: 0,
            sources_seq: 0,
            loading: true,
            detail_loading: false,
            update: None,
            applying: false,
            update_notice: String::new(),
            notice: None,
            error: None,
            jobs: HashMap::new(),
            quick_pending: HashSet::new(),
            playing: false,
            show_episodes: true,
            descending: false,
            dirty: false,
            saved: Instant::now(),
        })
    }

    fn toast(&mut self, message: &str) {
        self.notice = Some((message.into(), Instant::now()));
    }

    fn player_selector_pills(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Reproductor:").size(13.0).color(MUTED));
            for (val, label) in [("auto", "Auto"), ("mpv", "mpv"), ("vlc", "VLC")] {
                let selected = self.state.settings.player == val;
                if pill_button(ui, label, selected).clicked() && !selected {
                    self.state.settings.player = val.into();
                    self.dirty = true;
                }
            }
        });
    }

    fn handle_direct_media(&mut self, download: bool) {
        let raw = self.direct_url.trim().to_owned();
        let Ok(parsed) = network::web_url(&raw) else {
            self.toast("Introduce un enlace http:// o https:// válido (.m3u8, .mp4, .mkv, .webm…).");
            return;
        };
        let title = if self.direct_title.trim().is_empty() {
            parsed
                .path_segments()
                .and_then(|mut s| s.rfind(|seg| !seg.is_empty()))
                .filter(|s| !s.is_empty())
                .unwrap_or("Flujo directo")
                .to_owned()
        } else {
            self.direct_title.trim().to_owned()
        };
        let origin = format!(
            "{}://{}/",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default()
        );
        let id_hash = storage::hash(&raw);
        let anime = Anime {
            id: id_hash.clone(),
            slug: format!("direct-{}", &id_hash[..10]),
            title: title.clone(),
            url: origin.clone(),
            ..Default::default()
        };
        let episode = Episode {
            id: "1".into(),
            number: 1.0,
            title,
            url: raw.clone(),
            thumbnail: String::new(),
        };
        let server = Server {
            server: "Directo".into(),
            url: raw,
        };
        if download {
            self.download(anime, episode, server, "DIRECTO".into());
        } else {
            self.playing = true;
            self.tasks.play(
                self.definition.clone(),
                anime,
                episode,
                server,
                self.state.settings.player.clone(),
            );
        }
    }

    fn refresh(&mut self) {
        self.catalog_seq += 1;
        self.loading = true;
        self.error = None;
        self.tasks.browse(
            self.catalog_seq,
            self.definition.clone(),
            self.browse.clone(),
        );
    }

    fn navigate(&mut self, page: Page) {
        self.page = page;
        self.picker = None;
        self.sources_seq += 1;
        if matches!(page, Page::Home | Page::Catalog | Page::Releases) {
            self.browse = Browse {
                home: page == Page::Home,
                airing: page == Page::Releases,
                page: 1,
                ..Default::default()
            };
            self.search.clear();
            self.refresh();
        }
    }

    fn search(&mut self) {
        self.page = Page::Catalog;
        self.browse = Browse {
            search: self.search.trim().into(),
            page: 1,
            ..Default::default()
        };
        if !self.browse.search.is_empty() {
            self.state.searches.retain(|s| s != &self.browse.search);
            self.state.searches.insert(0, self.browse.search.clone());
            self.state.searches.truncate(8);
            self.dirty = true;
        }
        self.refresh();
    }

    fn open(&mut self, a: Anime) {
        self.detail_seq += 1;
        self.detail_loading = true;
        self.error = None;
        self.tasks
            .detail(self.detail_seq, self.definition.clone(), a.url.clone());
        self.detail = Some(a);
        self.page = Page::Detail;
        self.show_episodes = true;
    }

    fn pick(&mut self, anime: Anime, episode: Episode) {
        self.sources_seq += 1;
        self.tasks.sources(
            self.sources_seq,
            self.definition.clone(),
            episode.clone(),
            None,
        );
        self.picker = Some(Picker {
            anime,
            episode,
            sources: None,
            language: String::new(),
            index: 0,
            download_tab: false,
        });
    }

    fn quick_download(&mut self, anime: Anime, episode: Episode) {
        if self.quick_pending.insert(episode.url.clone()) {
            self.tasks
                .sources(0, self.definition.clone(), episode, Some(anime));
            self.toast("Preparando descarga…");
        }
    }

    fn download(&mut self, a: Anime, ep: Episode, server: Server, lang: String) {
        let root = self.state.settings.download_dir.clone();
        if !root.is_absolute() {
            self.toast("Elige una carpeta de descarga válida en Opciones.");
            return;
        }
        let key = downloader::key(&ep.url, &lang);
        if self.jobs.contains_key(&key) {
            self.toast("Este episodio ya está en la cola.");
            return;
        }
        if self
            .state
            .downloads
            .iter()
            .any(|j| j.key == key && j.status == DownloadStatus::Complete)
        {
            self.toast("Este episodio ya está descargado.");
            return;
        }
        self.state.downloads.retain(|j| j.key != key);
        self.state.downloads.push(Download {
            key: key.clone(),
            title: a.title.clone(),
            episode: format!("Episodio {}", ep.number),
            language: lang.clone(),
            path: Default::default(),
            progress: None,
            status: DownloadStatus::Queued,
        });
        let cancel = CancellationToken::new();
        self.jobs.insert(key.clone(), cancel.clone());
        self.tasks.download(
            self.definition.clone(),
            a,
            ep,
            server,
            lang,
            root,
            key,
            cancel,
        );
        self.dirty = true;
        self.toast("Episodio añadido a descargas.");
    }

    fn cancel_episode(&mut self, ep: &Episode) {
        for lang in ["SUB", "DUB", "DIRECTO"] {
            if let Some(c) = self.jobs.get(&downloader::key(&ep.url, lang)) {
                c.cancel();
            }
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Boot(d) => {
                    self.definition = d;
                    self.refresh();
                }
                Event::Catalog(id, result) if id == self.catalog_seq => {
                    self.loading = false;
                    match result {
                        Ok(c) => self.catalog = c,
                        Err(_) => {
                            self.error = Some(
                                "No se pudo cargar el catálogo. Comprueba tu conexión o vuelve a intentarlo más tarde."
                                    .into(),
                            )
                        }
                    }
                }
                Event::Detail(id, result) if id == self.detail_seq => {
                    self.detail_loading = false;
                    match result {
                        Ok(a) => self.detail = Some(a),
                        Err(_) => {
                            self.error = Some(
                                "No se pudo abrir esta ficha. Vuelve a intentarlo o comprueba las actualizaciones de la fuente."
                                    .into(),
                            )
                        }
                    }
                }
                Event::Sources(id, result) if id == self.sources_seq => match result {
                    Ok(s) => {
                        if let Some(p) = &mut self.picker {
                            p.language = s.languages().first().cloned().unwrap_or_default();
                            p.sources = Some(s);
                        }
                    }
                    Err(_) => {
                        self.picker = None;
                        self.toast("Este episodio no tiene servidores disponibles o no se pudo consultar.");
                    }
                },
                Event::Quick(a, ep, result) => {
                    self.quick_pending.remove(&ep.url);
                    if let Ok(s) = result {
                        if let Some(lang) = s.languages().first().cloned() {
                            if let Some(server) = s.preferred(&lang) {
                                self.download(a, ep, server, lang);
                            }
                        }
                    } else {
                        self.toast("No se pudo preparar la descarga. Prueba otro episodio o servidor.");
                    }
                }
                Event::Update(m) => self.update = m,
                Event::UpdateFailed => {
                    self.update_notice =
                        "No se pudieron comprobar las actualizaciones. La fuente local sigue disponible."
                            .into()
                }
                Event::Applied(result) => {
                    self.applying = false;
                    match result {
                        Ok(d) => {
                            self.definition = d;
                            self.update = None;
                            self.update_notice.clear();
                            self.sources_seq += 1;
                            self.picker = None;
                            self.detail_seq += 1;
                            self.detail_loading = false;
                            if self.page == Page::Detail {
                                if let Some(a) = self.detail.clone() {
                                    self.open(a);
                                }
                            } else {
                                self.refresh();
                            }
                            self.toast("Fuente actualizada. Ya puedes seguir explorando.");
                        }
                        Err(_) => self.toast(
                            "No se pudo aplicar la actualización. Se conserva la fuente anterior.",
                        ),
                    }
                }
                Event::Played(history, result) => {
                    self.playing = false;
                    match result {
                        Ok(confirmation) => {
                            if !history.anime.slug.starts_with("direct-") {
                                self.state
                                    .history
                                    .retain(|h| h.anime.slug != history.anime.slug);
                                self.state.history.insert(0, history);
                                self.state.history.truncate(100);
                                self.dirty = true;
                            }
                            let is_vlc = self.state.settings.player == "vlc";
                            self.toast(match confirmation {
                                crate::player::Confirmation::Playing => {
                                    "Reproducción iniciada en mpv."
                                }
                                crate::player::Confirmation::Launched if is_vlc => {
                                    "Reproducción abierta en VLC."
                                }
                                crate::player::Confirmation::Launched => {
                                    "Reproductor externo iniciado."
                                }
                            });
                        }
                        Err(error) => self.toast(error.message()),
                    }
                }
                Event::FilePlayed(result) => {
                    let is_vlc = self.state.settings.player == "vlc";
                    match result {
                        Ok(crate::player::Confirmation::Playing) => {
                            self.toast("Reproducción iniciada en mpv.")
                        }
                        Ok(crate::player::Confirmation::Launched) if is_vlc => {
                            self.toast("Reproducción local abierta en VLC.")
                        }
                        Ok(crate::player::Confirmation::Launched) => {
                            self.toast("Reproducción enviada a la aplicación de vídeo.")
                        }
                        Err(error) => self.toast(error.message()),
                    }
                }
                Event::Planned(key, path) => {
                    if let Some(j) = self.state.downloads.iter_mut().find(|j| j.key == key) {
                        j.path = path;
                        j.status = DownloadStatus::Running;
                        self.dirty = true;
                    }
                }
                Event::Progress(key, p) => {
                    if let Some(j) = self.state.downloads.iter_mut().find(|j| j.key == key) {
                        j.progress = p;
                    }
                }
                Event::Finished(key, ok, cancelled) => {
                    self.jobs.remove(&key);
                    if let Some(j) = self.state.downloads.iter_mut().find(|j| j.key == key) {
                        j.status = if cancelled {
                            DownloadStatus::Cancelled
                        } else if ok {
                            DownloadStatus::Complete
                        } else {
                            DownloadStatus::Failed
                        };
                        if ok {
                            j.progress = Some(1.0);
                        }
                    }
                    self.dirty = true;
                    self.toast(if cancelled {
                        "Descarga cancelada."
                    } else if ok {
                        "Descarga terminada. Disponible sin conexión."
                    } else {
                        "No se pudo descargar. Prueba otro servidor y comprueba yt-dlp, FFmpeg y el espacio disponible."
                    });
                }
                Event::SaveFailed => self.toast(
                    "No se pudieron guardar los cambios locales. Comprueba el espacio y los permisos.",
                ),
                _ => {}
            }
        }
    }

    fn image(&mut self, ui: &mut egui::Ui, url: &str, size: Vec2) {
        if let Some(texture) =
            self.images
                .get(url, ui.ctx(), &self.tasks.handle, &self.tasks.client)
        {
            // Center crop rather than stretch the cover.
            let original = texture.size_vec2();
            let target_aspect = size.x / size.y;
            let image_aspect = original.x / original.y;
            let uv = if image_aspect > target_aspect {
                let w = target_aspect / image_aspect;
                Rect::from_min_max(
                    egui::pos2((1.0 - w) / 2.0, 0.0),
                    egui::pos2((1.0 + w) / 2.0, 1.0),
                )
            } else {
                let h = image_aspect / target_aspect;
                Rect::from_min_max(
                    egui::pos2(0.0, (1.0 - h) / 2.0),
                    egui::pos2(1.0, (1.0 + h) / 2.0),
                )
            };
            ui.add(
                egui::Image::new(&texture)
                    .uv(uv)
                    .fit_to_exact_size(size)
                    .maintain_aspect_ratio(false)
                    .corner_radius(10),
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                paint_gradient_rect(
                    ui.painter(),
                    rect,
                    10.0,
                    Color32::from_rgb(30, 36, 62),
                    Color32::from_rgb(20, 26, 46),
                );
                ui.painter()
                    .rect_stroke(rect, 10.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
                ui.painter().circle_stroke(
                    rect.center(),
                    18.0,
                    Stroke::new(1.8, Color32::from_rgba_unmultiplied(0, 226, 210, 90)),
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "A",
                    egui::FontId::proportional(17.0),
                    CYAN,
                );
            }
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, anime: &Anime, width: f32) -> bool {
        let mut clicked = false;
        ui.push_id(&anime.slug, |ui| {
            let mut frame = egui::Frame::new()
                .fill(CARD)
                .corner_radius(14)
                .inner_margin(9)
                .stroke(Stroke::new(1.0, BORDER))
                .shadow(soft_shadow())
                .begin(ui);
            {
                let ui = &mut frame.content_ui;
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    let inner_w = (width - 20.0).max(96.0);
                    ui.set_width(inner_w);
                    self.image(ui, &anime.cover, egui::vec2(inner_w, inner_w * 1.38));
                    ui.add_space(4.0);
                    if let Some(badge) = &anime.badge {
                        badge_pill(ui, badge, CYAN, Color32::from_rgb(16, 49, 62));
                    } else {
                        badge_pill(
                            ui,
                            anime.kind.as_deref().unwrap_or("ANIME"),
                            Color32::from_rgb(185, 196, 232),
                            Color32::from_rgb(33, 39, 66),
                        );
                    }
                    let mut text = egui::text::LayoutJob::simple(
                        anime.title.clone(),
                        egui::FontId::proportional(14.0),
                        TEXT_MAIN,
                        inner_w,
                    );
                    text.wrap.max_rows = 2;
                    ui.allocate_ui_with_layout(
                        egui::vec2(inner_w, 40.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_height(40.0);
                            ui.label(text);
                        },
                    );
                });
            }
            let response = frame.allocate_space(ui);
            let interact = response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&anime.title);
            let hover = ui
                .ctx()
                .animate_bool_responsive(interact.id, interact.hovered());
            frame.frame.fill = lerp_color(CARD, CARD_HOVER, hover);
            frame.frame.stroke = Stroke::new(1.0 + hover * 0.5, lerp_color(BORDER, CYAN, hover * 0.8));
            frame.paint(ui);
            clicked = interact.clicked();
        });
        clicked
    }

    fn cards(&mut self, ui: &mut egui::Ui, items: &[Anime], horizontal: bool, id: &str) {
        let mut selected = None;
        if horizontal {
            egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for item in items {
                        if self.card(ui, item, 168.0) {
                            selected = Some(item.clone());
                        }
                    }
                });
            });
        } else {
            let columns = ((ui.available_width() + 14.0) / 180.0).floor().max(1.0) as usize;
            let width =
                ((ui.available_width() - (columns - 1) as f32 * 14.0) / columns as f32).min(232.0);
            for row in items.chunks(columns) {
                ui.horizontal_top(|ui| {
                    for item in row {
                        if self.card(ui, item, width) {
                            selected = Some(item.clone());
                        }
                    }
                });
                ui.add_space(10.0);
            }
        }
        if let Some(a) = selected {
            self.open(a);
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let narrow = ctx.screen_rect().width() < 850.0;
        egui::SidePanel::left("navigation")
            .resizable(false)
            .exact_width(if narrow { 148.0 } else { 210.0 })
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(16, 18))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ctx, |ui| {
                ui.add_space(4.0);
                // Brand badge header with subtle gradient emblem
                ui.horizontal(|ui| {
                    let (logo_rect, _) =
                        ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                    paint_gradient_rect(ui.painter(), logo_rect, 10.0, CYAN, VIOLET);
                    ui.painter().text(
                        logo_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "A",
                        egui::FontId::proportional(19.0),
                        Color32::from_rgb(10, 14, 24),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(
                            RichText::new("AniLinux")
                                .size(if narrow { 20.0 } else { 23.0 })
                                .strong()
                                .color(TEXT_MAIN),
                        );
                        if !narrow {
                            ui.label(RichText::new("Escritorio Linux").size(11.0).color(MUTED));
                        }
                    });
                });
                ui.add_space(28.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                let nav_w = ui.available_width();
                for (page, label) in [
                    (Page::Home, "Inicio"),
                    (Page::Catalog, "Catálogo"),
                    (Page::Releases, "En emisión"),
                    (Page::Downloads, "Descargas"),
                    (Page::Settings, "Opciones"),
                ] {
                    if nav_button(ui, label, self.page == page, nav_w).clicked() {
                        self.navigate(page);
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let player_label = match self.state.settings.player.as_str() {
                        "mpv" => "Reproductor: mpv",
                        "vlc" => "Reproductor: VLC",
                        "xdg-open" => "Reproductor: Sistema",
                        _ => "Reproductor: Auto",
                    };
                    ui.label(RichText::new(player_label).size(11.5).color(MUTED));
                    ui.label(RichText::new("AnimeAV1 · HLS/HTTP").size(11.5).color(MUTED));
                    if !self.jobs.is_empty() {
                        badge_pill(
                            ui,
                            &format!("{} en cola", self.jobs.len()),
                            CYAN,
                            Color32::from_rgb(16, 49, 62),
                        );
                    }
                });
            });
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(22, 14))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let title = match self.page {
                        Page::Home => "Inicio",
                        Page::Catalog => "Catálogo",
                        Page::Releases => "En emisión",
                        Page::Downloads => "Descargas",
                        Page::Settings => "Opciones",
                        Page::Detail => "Explorar ficha",
                    };
                    ui.label(RichText::new(title).size(24.0).strong().color(TEXT_MAIN));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if gradient_button(ui, "Buscar", CYAN, AZURE, true, true).clicked() {
                            self.search();
                        }
                        let width = (ui.available_width() - 16.0).clamp(80.0, 320.0);
                        let pill_frame = egui::Frame::new()
                            .fill(INPUT_BG)
                            .corner_radius(CornerRadius::same(19))
                            .inner_margin(Margin::symmetric(14, 7))
                            .stroke(Stroke::new(1.0, BORDER));
                        pill_frame.show(ui, |ui| {
                            let response = ui.add_sized(
                                [(width - 28.0).max(60.0), 22.0],
                                egui::TextEdit::singleline(&mut self.search)
                                    .frame(false)
                                    .hint_text("Buscar anime…"),
                            );
                            if response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            {
                                self.search();
                            }
                        });
                    });
                });
                if self.update.is_some() {
                    ui.add_space(10.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(20, 48, 70))
                        .inner_margin(Margin::symmetric(14, 10))
                        .corner_radius(12)
                        .stroke(Stroke::new(1.0, CYAN))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new("Hay una actualización de la fuente disponible.")
                                        .color(TEXT_MAIN),
                                );
                                let label = if self.applying {
                                    "Actualizando…"
                                } else {
                                    "Actualizar ahora"
                                };
                                if gradient_button(ui, label, CYAN, AZURE, true, !self.applying)
                                    .clicked()
                                {
                                    if let Some(m) = self.update.clone() {
                                        self.applying = true;
                                        self.tasks.apply(m);
                                    }
                                }
                            });
                        });
                }
            });
    }

    fn browse_view(&mut self, ui: &mut egui::Ui) {
        let is_home = self.page == Page::Home;
        if is_home {
            // Subtle modern welcome banner with gradient surface
            let banner_w = ui.available_width();
            let (hero_rect, _) =
                ui.allocate_exact_size(egui::vec2(banner_w, 76.0), egui::Sense::hover());
            if ui.is_rect_visible(hero_rect) {
                paint_gradient_rect(
                    ui.painter(),
                    hero_rect,
                    16.0,
                    Color32::from_rgb(19, 44, 72),
                    Color32::from_rgb(38, 28, 76),
                );
                ui.painter().rect_stroke(
                    hero_rect,
                    16.0,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(88, 166, 255, 70)),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    egui::pos2(hero_rect.left() + 22.0, hero_rect.top() + 26.0),
                    egui::Align2::LEFT_CENTER,
                    "Explora tus series favoritas",
                    egui::FontId::proportional(19.0),
                    Color32::WHITE,
                );
                ui.painter().text(
                    egui::pos2(hero_rect.left() + 22.0, hero_rect.top() + 51.0),
                    egui::Align2::LEFT_CENTER,
                    "Reproducción fluida en mpv y VLC · Soporte directo HLS (.m3u8) y HTTP",
                    egui::FontId::proportional(12.5),
                    Color32::from_rgb(184, 204, 236),
                );
            }
            ui.add_space(14.0);
        }
        if !is_home {
            egui::Frame::new()
                .fill(PANEL)
                .corner_radius(14)
                .inner_margin(Margin::symmetric(14, 10))
                .stroke(Stroke::new(1.0, BORDER))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let mut genre = self.browse.genre.clone();
                        let current = self
                            .catalog
                            .genres
                            .iter()
                            .find(|(id, _)| id == &genre)
                            .map(|(_, n)| n.as_str())
                            .unwrap_or("Todos los géneros");
                        egui::ComboBox::from_id_salt("genres")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut genre, String::new(), "Todos los géneros");
                                for (id, name) in &self.catalog.genres {
                                    ui.selectable_value(&mut genre, id.clone(), name);
                                }
                            });
                        if genre != self.browse.genre {
                            self.browse.genre = genre;
                            self.browse.page = 1;
                            self.refresh();
                        }
                        if pill_button(ui, "Actualizar", false).clicked() {
                            self.refresh();
                        }
                        if !self.browse.search.is_empty() {
                            badge_pill(
                                ui,
                                &format!("Búsqueda: «{}»", self.browse.search),
                                CYAN,
                                Color32::from_rgb(16, 49, 62),
                            );
                        }
                    });
                });
            if self.browse.search.is_empty() && !self.state.searches.is_empty() {
                let searches = self.state.searches.clone();
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Recientes:").small().color(MUTED));
                    for s in searches.iter().take(5) {
                        if pill_button(ui, s, false).clicked() {
                            self.search = s.clone();
                            self.search();
                        }
                    }
                    if ui.small_button(RichText::new("Limpiar").color(MUTED)).clicked() {
                        self.state.searches.clear();
                        self.dirty = true;
                    }
                });
            }
            ui.add_space(12.0);
        }
        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Cargando catálogo…").color(MUTED));
            });
        }
        if !self.loading && is_home && !self.state.history.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Continuar viendo").size(21.0).strong());
                badge_pill(
                    ui,
                    "Recientes",
                    AZURE,
                    Color32::from_rgb(20, 38, 68),
                );
            });
            let items: Vec<_> = self
                .state
                .history
                .iter()
                .take(12)
                .map(|h| {
                    let mut a = h.anime.clone();
                    a.badge = Some(format!("EP {}", h.episode.number));
                    a
                })
                .collect();
            self.cards(ui, &items, true, "history");
            ui.add_space(20.0);
        }
        if !self.loading {
            let sections = self.catalog.sections.clone();
            if sections.is_empty() && self.error.is_none() {
                ui.label(RichText::new("No se encontraron animes con estos filtros.").color(MUTED));
            }
            for (index, (title, items)) in sections.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(21.5).strong());
                    if is_home && pill_button(ui, "Ver catálogo →", false).clicked() {
                        self.navigate(Page::Catalog);
                    }
                });
                ui.add_space(4.0);
                self.cards(ui, items, is_home, &format!("section-{index}"));
                ui.add_space(22.0);
            }
            if !is_home && self.catalog.total_pages > 1 {
                ui.horizontal(|ui| {
                    if gradient_button(
                        ui,
                        "‹ Anterior",
                        AZURE,
                        VIOLET,
                        false,
                        self.browse.page > 1,
                    )
                    .clicked()
                    {
                        self.browse.page -= 1;
                        self.refresh();
                    }
                    badge_pill(
                        ui,
                        &format!("Página {} de {}", self.catalog.page, self.catalog.total_pages),
                        TEXT_MAIN,
                        CARD,
                    );
                    if gradient_button(
                        ui,
                        "Siguiente ›",
                        AZURE,
                        VIOLET,
                        false,
                        self.browse.page < self.catalog.total_pages,
                    )
                    .clicked()
                    {
                        self.browse.page += 1;
                        self.refresh();
                    }
                });
            }
        }
    }

    fn detail_view(&mut self, ui: &mut egui::Ui) {
        if pill_button(ui, "‹ Volver al catálogo", false).clicked() {
            self.navigate(Page::Catalog);
            return;
        }
        ui.add_space(6.0);
        if self.detail_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Abriendo ficha…").color(MUTED));
            });
        }
        let Some(a) = self.detail.clone() else {
            return;
        };
        if !a.banner.is_empty() {
            self.image(ui, &a.banner, egui::vec2(ui.available_width(), 176.0));
            ui.add_space(8.0);
        }
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(16)
            .inner_margin(18)
            .stroke(Stroke::new(1.0, BORDER))
            .shadow(soft_shadow())
            .show(ui, |ui| {
                let compact = ui.available_width() < 540.0;
                ui.horizontal_top(|ui| {
                    if !compact {
                        self.image(ui, &a.cover, egui::vec2(158.0, 224.0));
                        ui.add_space(8.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&a.title).size(26.0).strong().color(Color32::WHITE));
                        ui.horizontal_wrapped(|ui| {
                            for s in [
                                a.year.clone(),
                                a.kind.clone(),
                                (!a.episodes.is_empty())
                                    .then(|| format!("{} episodios", a.episodes.len())),
                            ]
                            .into_iter()
                            .flatten()
                            {
                                badge_pill(ui, &s, CYAN, Color32::from_rgb(16, 49, 62));
                            }
                        });
                        if !a.synopsis.is_empty() {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(&a.synopsis)
                                    .color(Color32::from_rgb(198, 205, 226)),
                            );
                        }
                        if !a.genres.is_empty() {
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for genre in &a.genres {
                                    badge_pill(
                                        ui,
                                        genre,
                                        Color32::from_rgb(208, 214, 242),
                                        CARD,
                                    );
                                }
                            });
                        }
                        ui.add_space(8.0);
                        let last = self
                            .state
                            .history
                            .iter()
                            .find(|h| h.anime.slug == a.slug)
                            .map(|h| h.episode.clone());
                        ui.horizontal_wrapped(|ui| {
                            if let Some(ep) = last {
                                let text = format!("▶ Continuar · EP {}", ep.number);
                                if gradient_button(ui, &text, CYAN, AZURE, true, true).clicked() {
                                    self.pick(a.clone(), ep);
                                }
                            } else if let Some(ep) = a.episodes.first() {
                                if gradient_button(
                                    ui,
                                    "▶ Ver primer episodio",
                                    CYAN,
                                    AZURE,
                                    true,
                                    true,
                                )
                                .clicked()
                                {
                                    self.pick(a.clone(), ep.clone());
                                }
                            }
                            let toggle_label = if self.show_episodes {
                                "Ocultar episodios"
                            } else {
                                "Ver episodios"
                            };
                            if pill_button(ui, toggle_label, false).clicked() {
                                self.show_episodes = !self.show_episodes;
                            }
                        });
                    });
                });
            });
        ui.add_space(20.0);
        if self.show_episodes {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Episodios").size(22.0).strong());
                let order_label = if self.descending {
                    "↓ Más recientes primero"
                } else {
                    "↑ Orden ascendente"
                };
                if pill_button(ui, order_label, self.descending).clicked() {
                    self.descending = !self.descending;
                }
            });
            ui.label(
                RichText::new(
                    "Pulsa un episodio para elegir idioma, reproductor y servidor. El interruptor inicia o cancela su descarga.",
                )
                .size(12.5)
                .color(MUTED),
            );
            ui.add_space(4.0);
            if a.episodes.is_empty() && !self.detail_loading {
                ui.label(RichText::new("Aún no hay episodios publicados.").color(MUTED));
            }
            let mut episodes = a.episodes.clone();
            if self.descending {
                episodes.reverse();
            }
            for ep in episodes {
                ui.push_id(&ep.url, |ui| {
                    let jobs: Vec<_> = self
                        .state
                        .downloads
                        .iter()
                        .filter(|j| {
                            [
                                downloader::key(&ep.url, "SUB"),
                                downloader::key(&ep.url, "DUB"),
                            ]
                            .contains(&j.key)
                        })
                        .collect();
                    let complete = jobs.iter().any(|j| j.status == DownloadStatus::Complete);
                    let active = jobs.iter().any(|j| {
                        matches!(j.status, DownloadStatus::Queued | DownloadStatus::Running)
                    });
                    let pending = self.quick_pending.contains(&ep.url);
                    let (subtitle, sub_color) = if complete {
                        ("Disponible sin conexión", EMERALD)
                    } else if active {
                        ("Descargando…", AMBER)
                    } else if pending {
                        ("Preparando…", CYAN)
                    } else {
                        ("Elegir idioma y servidor", MUTED)
                    };
                    egui::Frame::new()
                        .fill(CARD)
                        .corner_radius(14)
                        .inner_margin(Margin::symmetric(12, 10))
                        .stroke(Stroke::new(1.0, BORDER))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.available_width() > 460.0 {
                                    self.image(ui, &ep.thumbnail, egui::vec2(112.0, 63.0));
                                }
                                let name = if ep.title.is_empty() {
                                    format!("Episodio {}", ep.number)
                                } else {
                                    format!("Episodio {} · {}", ep.number, ep.title)
                                };
                                let row_w = (ui.available_width() - 74.0).max(60.0);
                                let row_resp = ui
                                    .allocate_ui_with_layout(
                                        egui::vec2(row_w, 56.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(&name)
                                                    .size(15.0)
                                                    .strong()
                                                    .color(TEXT_MAIN),
                                            );
                                            ui.label(
                                                RichText::new(subtitle)
                                                    .size(12.5)
                                                    .color(sub_color),
                                            );
                                        },
                                    )
                                    .response
                                    .interact(egui::Sense::click())
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if row_resp.clicked() {
                                    self.pick(a.clone(), ep.clone());
                                }
                                let mut enabled = complete || active || pending;
                                let response = ui
                                    .add_enabled_ui(!complete && !pending, |ui| {
                                        toggle(ui, &mut enabled)
                                    })
                                    .inner;
                                if response.changed() {
                                    if active {
                                        self.cancel_episode(&ep);
                                    } else {
                                        self.quick_download(a.clone(), ep.clone());
                                    }
                                }
                            });
                        });
                });
            }
        }
    }

    fn downloads_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Tu biblioteca sin conexión")
                        .size(25.0)
                        .strong(),
                );
                ui.label(
                    RichText::new(
                        "Hasta 3 descargas simultáneas. Reproduce archivos locales o flujos abiertos en mpv o VLC.",
                    )
                    .color(MUTED),
                );
            });
        });
        ui.add_space(8.0);

        // Quick player selector + direct open-format (.m3u8 / HTTP) bar
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(14)
            .inner_margin(Margin::symmetric(16, 12))
            .stroke(Stroke::new(1.0, BORDER))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    self.player_selector_pills(ui);
                    ui.separator();
                    let label = if self.direct_open {
                        "Ocultar enlace directo (.m3u8 / HTTP)"
                    } else {
                        "+ Enlace directo (.m3u8 / HTTP)"
                    };
                    if pill_button(ui, label, self.direct_open).clicked() {
                        self.direct_open = !self.direct_open;
                    }
                });
                if self.direct_open {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Reproduce o descarga listas HLS (.m3u8) y vídeos multimedia por HTTP (.mp4, .mkv, .webm, .ogv) de fuentes propias o autorizadas.",
                        )
                        .size(12.5)
                        .color(MUTED),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.direct_url)
                            .desired_width(f32::INFINITY)
                            .hint_text("https://ejemplo.org/stream/lista.m3u8  o  https://ejemplo.org/video.mp4"),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.add_sized(
                            [260.0, 34.0],
                            egui::TextEdit::singleline(&mut self.direct_title)
                                .hint_text("Título opcional…"),
                        );
                        let play_label = if self.playing {
                            "Abriendo…"
                        } else {
                            "▶ Reproducir enlace"
                        };
                        if gradient_button(ui, play_label, CYAN, AZURE, true, !self.playing)
                            .clicked()
                        {
                            self.handle_direct_media(false);
                        }
                        if gradient_button(ui, "⬇ Descargar", AZURE, VIOLET, false, true).clicked()
                        {
                            self.handle_direct_media(true);
                        }
                    });
                }
            });

        ui.add_space(12.0);
        if self.state.downloads.is_empty() {
            egui::Frame::new()
                .fill(CARD)
                .corner_radius(16)
                .inner_margin(26)
                .stroke(Stroke::new(1.0, BORDER))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("Todavía no tienes episodios descargados.")
                            .size(16.0)
                            .color(TEXT_MAIN),
                    );
                    ui.label(
                        RichText::new(
                            "Activa el interruptor de cualquier episodio o usa «Enlace directo (.m3u8 / HTTP)» arriba.",
                        )
                        .size(13.0)
                        .color(MUTED),
                    );
                });
        }
        for job in self.state.downloads.clone().into_iter().rev() {
            egui::Frame::new()
                .fill(CARD)
                .inner_margin(18)
                .corner_radius(16)
                .stroke(Stroke::new(1.0, BORDER))
                .shadow(soft_shadow())
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&job.title).size(17.0).strong().color(Color32::WHITE));
                        badge_pill(
                            ui,
                            &format!("{} · {}", job.episode, job.language),
                            CYAN,
                            Color32::from_rgb(16, 49, 62),
                        );
                    });
                    let (status_text, status_fg, status_bg) = match job.status {
                        DownloadStatus::Queued => ("En cola", AMBER, Color32::from_rgb(54, 42, 20)),
                        DownloadStatus::Running => {
                            ("Descargando", CYAN, Color32::from_rgb(16, 49, 62))
                        }
                        DownloadStatus::Complete => (
                            "Disponible sin conexión",
                            EMERALD,
                            Color32::from_rgb(16, 52, 42),
                        ),
                        DownloadStatus::Failed => {
                            ("No se pudo descargar", CORAL, Color32::from_rgb(58, 24, 32))
                        }
                        DownloadStatus::Cancelled => {
                            ("Cancelada / interrumpida", MUTED, Color32::from_rgb(34, 38, 58))
                        }
                    };
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        badge_pill(ui, status_text, status_fg, status_bg);
                        if matches!(job.status, DownloadStatus::Queued | DownloadStatus::Running) {
                            if pill_button(ui, "Cancelar", false).clicked() {
                                if let Some(c) = self.jobs.get(&job.key) {
                                    c.cancel();
                                }
                            }
                        } else if job.status == DownloadStatus::Complete
                            && gradient_button(ui, "▶ Reproducir", CYAN, AZURE, true, true)
                                .clicked()
                        {
                            self.tasks
                                .play_file(job.path.clone(), self.state.settings.player.clone());
                        }
                        if !matches!(job.status, DownloadStatus::Queued | DownloadStatus::Running)
                            && pill_button(ui, "Quitar de la lista", false).clicked()
                        {
                            self.state.downloads.retain(|j| j.key != job.key);
                            self.dirty = true;
                        }
                    });
                    if job.status == DownloadStatus::Running {
                        ui.add_space(6.0);
                        if let Some(p) = job.progress {
                            gradient_progress_bar(ui, p);
                            ui.label(
                                RichText::new(format!("{:.0}% completado", p * 100.0))
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        } else {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(RichText::new("Recibiendo flujo…").size(12.5).color(MUTED));
                            });
                        }
                    }
                });
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("Quitar de la lista no borra el vídeo guardado en el disco.")
                .small()
                .color(MUTED),
        );
    }

    fn settings_view(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(16)
            .inner_margin(20)
            .stroke(Stroke::new(1.0, BORDER))
            .shadow(soft_shadow())
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("Reproducción y descargas")
                        .size(21.0)
                        .strong(),
                );
                ui.add_space(10.0);
                ui.label(RichText::new("Carpeta de descargas").strong());
                ui.add(
                    egui::TextEdit::singleline(&mut self.folder).desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                if gradient_button(ui, "Guardar carpeta", CYAN, AZURE, true, true).clicked() {
                    let mut path = std::path::PathBuf::from(self.folder.trim());
                    if let Some(suffix) = self.folder.trim().strip_prefix("~/") {
                        if let Some(home) = dirs::home_dir() {
                            path = home.join(suffix);
                        }
                    }
                    if path.is_absolute() {
                        self.state.settings.download_dir = path;
                        self.dirty = true;
                        self.toast("Carpeta de descarga guardada.");
                    } else {
                        self.toast(
                            "Indica una carpeta con ruta absoluta, por ejemplo dentro de tu carpeta personal.",
                        );
                    }
                }
                ui.add_space(18.0);
                ui.label(RichText::new("Reproductor preferido").strong());
                ui.label(
                    RichText::new(
                        "Funciona tanto para episodios descargados como para streaming (HLS / HTTP).",
                    )
                    .size(12.5)
                    .color(MUTED),
                );
                let old = self.state.settings.player.clone();
                let selected_label = match old.as_str() {
                    "auto" => "Automático (mpv / VLC)",
                    "mpv" => "mpv",
                    "vlc" => "VLC",
                    "xdg-open" => "Aplicación predeterminada (archivos locales)",
                    other => other,
                };
                egui::ComboBox::from_id_salt("player")
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("auto", "Automático (mpv / VLC)"),
                            ("mpv", "mpv"),
                            ("vlc", "VLC"),
                            ("xdg-open", "Aplicación predeterminada (archivos locales)"),
                        ] {
                            ui.selectable_value(&mut self.state.settings.player, value.into(), label);
                        }
                    });
                ui.add_space(6.0);
                self.player_selector_pills(ui);
                if old != self.state.settings.player {
                    self.dirty = true;
                }
            });

        ui.add_space(16.0);
        egui::Frame::new()
            .fill(PANEL)
            .corner_radius(16)
            .inner_margin(20)
            .stroke(Stroke::new(1.0, BORDER))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Fuente de contenido").size(20.0).strong());
                ui.horizontal(|ui| {
                    badge_pill(ui, "AnimeAV1", CYAN, Color32::from_rgb(16, 49, 62));
                    badge_pill(ui, "HLS .m3u8 / HTTP", AZURE, Color32::from_rgb(20, 38, 68));
                });
                if !self.update_notice.is_empty() {
                    ui.label(RichText::new(&self.update_notice).color(MUTED));
                } else {
                    ui.label(
                        RichText::new("Las actualizaciones de la fuente se comprueban al iniciar.")
                            .color(MUTED),
                    );
                }
                ui.add_space(8.0);
                ui.separator();
                ui.label(
                    RichText::new(
                        "Descarga y reproduce únicamente contenido para el que tengas autorización.",
                    )
                    .small()
                    .color(MUTED),
                );
            });
    }

    fn picker(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.picker.take() else {
            return;
        };
        let mut open = true;
        let mut action = None;
        egui::Window::new(format!("Episodio {}", p.episode.number))
            .id(egui::Id::new("episode-picker"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(&p.anime.title).size(20.0).strong().color(Color32::WHITE));
                ui.add_space(4.0);
                if let Some(s) = &p.sources {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Idioma:").color(MUTED));
                        for lang in s.languages() {
                            if pill_button(ui, &lang, p.language == lang).clicked() {
                                p.language = lang;
                                p.index = 0;
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if pill_button(ui, "Ver en línea", !p.download_tab).clicked() {
                            p.download_tab = false;
                            p.index = 0;
                        }
                        if pill_button(ui, "Enlaces de descarga", p.download_tab).clicked() {
                            p.download_tab = true;
                            p.index = 0;
                        }
                    });
                    if !p.download_tab {
                        self.player_selector_pills(ui);
                    }
                    let servers = if p.download_tab {
                        s.downloads.get(&p.language)
                    } else {
                        s.embeds.get(&p.language)
                    };
                    let servers = servers.cloned().unwrap_or_default();
                    if servers.is_empty() {
                        ui.label(
                            RichText::new("No hay servidores en esta categoría para este idioma.")
                                .color(MUTED),
                        );
                    } else {
                        p.index = p.index.min(servers.len() - 1);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Servidor:").color(MUTED));
                            egui::ComboBox::from_id_salt("server")
                                .selected_text(&servers[p.index].server)
                                .show_ui(ui, |ui| {
                                    for (i, server) in servers.iter().enumerate() {
                                        ui.selectable_value(&mut p.index, i, &server.server);
                                    }
                                });
                        });
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if !p.download_tab {
                                let play_label = if self.playing {
                                    "Abriendo…"
                                } else {
                                    "▶ Reproducir"
                                };
                                if gradient_button(ui, play_label, CYAN, AZURE, true, !self.playing)
                                    .clicked()
                                {
                                    action = Some((false, servers[p.index].clone()));
                                }
                            }
                            if gradient_button(ui, "⬇ Descargar", AZURE, VIOLET, false, true)
                                .clicked()
                            {
                                action = Some((true, servers[p.index].clone()));
                            }
                        });
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Consultando servidores…").color(MUTED));
                    });
                }
            });
        if let Some((download, server)) = action {
            if download {
                self.download(
                    p.anime.clone(),
                    p.episode.clone(),
                    server,
                    p.language.clone(),
                );
            } else {
                self.playing = true;
                self.tasks.play(
                    self.definition.clone(),
                    p.anime.clone(),
                    p.episode.clone(),
                    server,
                    self.state.settings.player.clone(),
                );
            }
        }
        if open {
            self.picker = Some(p);
        } else {
            self.sources_seq += 1;
        }
    }
}

fn toggle(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(48.0, 26.0), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *value);
    if ui.is_rect_visible(rect) {
        if t > 0.02 {
            let c0 = lerp_color(Color32::from_rgb(56, 62, 88), CYAN, t);
            let c1 = lerp_color(Color32::from_rgb(56, 62, 88), AZURE, t);
            paint_gradient_rect(ui.painter(), rect, 13.0, c0, c1);
        } else {
            ui.painter()
                .rect_filled(rect, 13.0, Color32::from_rgb(48, 54, 78));
            ui.painter()
                .rect_stroke(rect, 13.0, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        }
        let x = rect.left() + 13.0 + t * (rect.width() - 26.0);
        let thumb_color = lerp_color(Color32::from_rgb(198, 204, 226), Color32::WHITE, t);
        ui.painter()
            .circle_filled(egui::pos2(x, rect.center().y), 10.0, thumb_color);
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if *value {
            "Cancelar descarga"
        } else {
            "Descargar episodio"
        })
}

impl eframe::App for AniLinux {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input_mut(|i| {
            i.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::CTRL,
                egui::Key::Q,
            ))
        }) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.poll();
        self.images.poll(ctx);
        self.sidebar(ctx);
        self.header(ctx);
        egui::TopBottomPanel::bottom("notice")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(18, 9))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some((text, time)) = &self.notice {
                        if time.elapsed() < Duration::from_secs(12) {
                            badge_pill(ui, "Aviso", CYAN, Color32::from_rgb(16, 49, 62));
                            ui.label(RichText::new(text).color(TEXT_MAIN));
                        } else {
                            ui.label(RichText::new("AniLinux · Listo").small().color(MUTED));
                        }
                    } else {
                        ui.label(RichText::new("AniLinux · Listo").small().color(MUTED));
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(22))
            .show(ctx, |ui| {
                if let Some(error) = self.error.clone() {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(54, 24, 34))
                        .corner_radius(12)
                        .inner_margin(Margin::symmetric(14, 10))
                        .stroke(Stroke::new(1.0, CORAL))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(Color32::from_rgb(255, 190, 185), error);
                                if pill_button(ui, "Reintentar", false).clicked() {
                                    if self.page == Page::Detail {
                                        if let Some(a) = self.detail.clone() {
                                            self.open(a);
                                        }
                                    } else {
                                        self.refresh();
                                    }
                                }
                            });
                        });
                    ui.add_space(10.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt(format!("page-{}", self.page as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Home | Page::Catalog | Page::Releases => self.browse_view(ui),
                        Page::Detail => self.detail_view(ui),
                        Page::Downloads => self.downloads_view(ui),
                        Page::Settings => self.settings_view(ui),
                    });
            });
        self.picker(ctx);
        if self.dirty && self.saved.elapsed() > Duration::from_millis(500) {
            self.tasks.save(&self.state);
            self.dirty = false;
            self.saved = Instant::now();
        }
        if self.loading
            || self.detail_loading
            || self.playing
            || !self.jobs.is_empty()
            || self.dirty
            || self
                .notice
                .as_ref()
                .is_some_and(|(_, t)| t.elapsed() < Duration::from_secs(12))
        {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for cancel in self.jobs.values() {
            cancel.cancel();
        }
        if let Ok(mut snapshot) = self.final_state.lock() {
            *snapshot = self.state.clone();
        }
    }
}
