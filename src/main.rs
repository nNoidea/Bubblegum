mod app_state;
mod backends;
mod models;
mod ui;

use app_state::AppState;
use gtk4 as gtk;
use libadwaita as adw;

use adw::prelude::*;
use gtk::glib;

fn main() -> glib::ExitCode {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _enter = rt.enter();

    let app = adw::Application::builder()
        .application_id("com.github.BubblegumGNOME2")
        .build();

    let state = AppState::new();

    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(
            "
            .detail-panel {
                background-color: @window_bg_color;
                border: 1px solid @borders;
                border-radius: 12px;
                box-shadow: 0 4px 12px rgba(0,0,0,0.5);
                padding: 16px 24px;
            }
            .packages-grid {
                padding-bottom: 240px;
            }
            .pm-label {
                background-color: alpha(currentColor, 0.2);
                color: currentColor;
                padding: 4px 8px;
                border-radius: 6px;
                font-weight: bold;
            }
            .pm-dnf { background-color: shade(@accent_bg_color, 0.6); color: @accent_fg_color; }
            .pm-flatpak { background-color: shade(@success_bg_color, 0.6); color: @success_fg_color; }
            .pm-cargo { background-color: shade(@error_bg_color, 0.6); color: @error_fg_color; }
            .dep-badge { background-color: alpha(currentColor, 0.1); color: currentColor; border-radius: 6px; padding: 4px 8px; font-weight: bold; }
            .user-badge { background-color: alpha(currentColor, 0.25); color: currentColor; border-radius: 6px; padding: 4px 8px; font-weight: bold; }
            .source-label {
                padding: 4px 8px;
                border-radius: 6px;
                font-weight: bold;
            }
            .repo-page-label {
                padding: 4px 10px;
            }
            .copy-btn {
                min-height: 24px;
                padding: 4px 8px;
                border-radius: 6px;
            }
            .copy-btn:hover {
                background-color: alpha(currentColor, 0.15);
            }
            ",
        );
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });

    app.connect_activate(move |app| {
        ui::build_window(app, state.clone());
    });
    app.run()
}
