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
                padding: 2px 6px;
                border-radius: 6px;
                font-weight: bold;
                font-size: 0.9em;
            }
            .pm-dnf { background-color: @accent_bg_color; color: @accent_fg_color; }
            .pm-flatpak { background-color: @success_bg_color; color: @success_fg_color; }
            .pm-cargo { background-color: @error_bg_color; color: @error_fg_color; }
            .dep-badge { background-color: alpha(@window_fg_color, 0.1); color: @window_fg_color; border-radius: 6px; padding: 2px 6px; font-size: 0.9em; font-weight: bold; }
            .user-badge { background-color: alpha(@accent_bg_color, 0.2); color: @accent_bg_color; border-radius: 6px; padding: 2px 6px; font-size: 0.9em; font-weight: bold; }
            .source-label {
                padding: 2px 6px;
                border-radius: 6px;
                font-weight: bold;
                font-size: 0.9em;
            }
            .repo-page-label {
                font-size: 0.9em;
                padding: 4px 10px;
            }
            .compact-btn {
                min-height: 28px;
                padding-top: 4px;
                padding-bottom: 4px;
                padding-left: 6px;
                padding-right: 6px;
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
