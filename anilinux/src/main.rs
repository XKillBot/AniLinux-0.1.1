use anilinux::{network, storage, ui::AniLinux};
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let handle = runtime.handle().clone();
    let client = network::client()?;
    let state = storage::load();
    let final_state = Arc::new(Mutex::new(state.clone()));
    let last = final_state.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AniLinux")
            .with_app_id("anilinux")
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([620.0, 480.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let result = eframe::run_native(
        "AniLinux",
        options,
        Box::new(move |cc| {
            let app = AniLinux::new(cc, handle, client, state, last)?;
            Ok(Box::new(app))
        }),
    );
    // Stop workers before writing the final snapshot: no writer races on exit.
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    if let Ok(snapshot) = final_state.lock() {
        let dir = storage::data_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("library.exit.tmp");
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(&*snapshot)?)?;
        file.sync_all()?;
        std::fs::rename(tmp, dir.join("library.json"))?;
    }
    result.map_err(|e| anyhow::anyhow!("No se pudo iniciar AniLinux: {e}"))
}
