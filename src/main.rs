mod app;
mod capture;
mod config;
mod geometry;
mod hotkeys;
mod logging;
mod model;
mod overlay;
mod render;
mod x11;

use gtk::prelude::*;

fn main() -> anyhow::Result<()> {
    if let Ok(path) = logging::init() {
        eprintln!("zoomix log: {}", path.display());
    }

    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none() {
        anyhow::bail!("Zoomix v0.1 targets Linux Mint Cinnamon on X11. Log in with an X11 session and try again.");
    }

    let app = gtk::Application::builder()
        .application_id("io.github.zoomix")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_command_line(|app, _cmd| {
        if let Err(err) = app::ZoomixApp::launch(app) {
            eprintln!("zoomix: {err:#}");
            return 1;
        }
        0
    });

    app.run();
    Ok(())
}
