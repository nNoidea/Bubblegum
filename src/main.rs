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
        .application_id("com.github.Bubblegum")
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
            .status-badge {
                padding: 2px 8px;
                border-radius: 9999px;
                font-size: 11px;
                font-weight: bold;
            }
            .status-loaded { background-color: alpha(@success_color, 0.2); color: @success_color; }
            .status-unavailable { background-color: alpha(@warning_color, 0.2); color: @warning_color; }
            .status-failed { background-color: alpha(@error_color, 0.2); color: @error_color; }
            .status-loading { background-color: alpha(@accent_color, 0.2); color: @accent_color; }
            .status-idle { opacity: 0.6; }
            .log-terminal {
                background-color: #1a1a24;
                border: 1px solid @borders;
                border-radius: 12px;
                box-shadow: inset 0 2px 8px rgba(0,0,0,0.5);
                padding: 4px;
            }
            .log-terminal textview,
            .log-terminal text {
                background-color: transparent;
                color: #e2e4ed;
                font-family: 'Monospace', 'Source Code Pro', 'DejaVu Sans Mono', monospace;
                font-size: 12px;
                line-height: 1.45;
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
