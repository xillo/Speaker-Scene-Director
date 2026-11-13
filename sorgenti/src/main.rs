// No Python runtime. A native GUI executable for Windows and other desktop systems.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
mod config;
mod core;
mod obs;
mod teamspeak;
mod ui;

fn main() -> eframe::Result<()> {
    let dir = config::base_dir();
    let cfg = match config::load_or_initialize(&dir) {
        Ok(cfg) => cfg,
        Err(error) => {
            // Shown in terminal when available; for a GUI-only Windows build, also write an error log.
            eprintln!("Configuration error: {error}");
            let _ = std::fs::write(dir.join("startup-error.txt"), &error);
            return Ok(());
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_icon(eframe::egui::IconData {
            rgba: include_bytes!("../assets/app.rgba").to_vec(),
            width: 256,
            height: 256,
        }).with_inner_size([960.0, 680.0]).with_min_inner_size([780.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Speaker Scene Director",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
            Ok(Box::new(ui::DirectorApp::new(cfg, dir)))
        }),
    )
}
