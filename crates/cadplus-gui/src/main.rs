//! Cad+ — расчёт крыла, лонжеронов и нервюр с топооптимизацией.
//! Запуск: `cargo run --release -p cadplus-gui`.

// В Windows без лишнего чёрного окна консоли.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod project;
mod worker;

fn main() -> eframe::Result {
    let mut options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Cad+ — крыло, лонжероны и нервюры")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    // Запасной вариант для старых видеокарт: CADPLUS_RENDERER=glow.
    if std::env::var("CADPLUS_RENDERER").is_ok_and(|v| v == "glow") {
        options.renderer = eframe::Renderer::Glow;
    }
    eframe::run_native(
        "Cad+",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
