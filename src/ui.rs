use crate::app_state::AppState;
use crate::models::{Package, Repository};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use gtk::glib;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn show_toast(
    overlay: &adw::ToastOverlay,
    active_toast: &Rc<RefCell<Option<adw::Toast>>>,
    toast: adw::Toast,
) {
    if let Some(prev) = active_toast.borrow_mut().take() {
        prev.dismiss();
    }
    *active_toast.borrow_mut() = Some(toast.clone());
    overlay.add_toast(toast);
}

fn create_status_badge(name: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(format!("{name}: -"))
        .css_classes(["caption", "status-badge", "status-idle"].to_vec())
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build()
}

fn update_status_badge(badge: &gtk::Label, name: &str, status: &crate::app_state::BackendStatus) {
    badge.remove_css_class("status-idle");
    badge.remove_css_class("status-loading");
    badge.remove_css_class("status-loaded");
    badge.remove_css_class("status-unavailable");
    badge.remove_css_class("status-failed");

    match status {
        crate::app_state::BackendStatus::Idle => {
            badge.set_text(&format!("{name}: -"));
            badge.add_css_class("status-idle");
            badge.set_tooltip_text(Some(&format!("{name} is idle")));
        }
        crate::app_state::BackendStatus::Loading => {
            badge.set_text(&format!("{name}: Loading..."));
            badge.add_css_class("status-loading");
            badge.set_tooltip_text(Some(&format!("{name} is loading...")));
        }
        crate::app_state::BackendStatus::Loaded {
            package_count,
            repo_count,
        } => {
            badge.set_text(&format!("{name}: {package_count}"));
            badge.add_css_class("status-loaded");
            badge.set_tooltip_text(Some(&format!(
                "{name}: {package_count} packages, {repo_count} repositories loaded"
            )));
        }
        crate::app_state::BackendStatus::Unavailable { reason } => {
            badge.set_text(&format!("{name}: Unavailable"));
            badge.add_css_class("status-unavailable");
            badge.set_tooltip_text(Some(&format!("{name} unavailable: {reason}")));
        }
        crate::app_state::BackendStatus::Failed { error } => {
            badge.set_text(&format!("{name}: Failed"));
            badge.add_css_class("status-failed");
            badge.set_tooltip_text(Some(&format!("{name} failed: {error}")));
        }
    }
}

thread_local! {
    static INJECTED_COLORS: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
    static GLOBAL_PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
    static GLOBAL_CSS: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn generate_color(seed: &str) -> (String, String) {
    if seed.to_lowercase() == "fedora" {
        return (
            "@accent_bg_color".to_string(),
            "@accent_fg_color".to_string(),
        );
    }

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let hash = hasher.finish();

    let h = (hash % 360) as f64;
    let s = 0.6 + ((hash / 360) % 20) as f64 / 100.0;
    let l = 0.4 + ((hash / 7200) % 20) as f64 / 100.0;

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r_, g_, b_) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r_ + m) * 255.0) as u8;
    let g = ((g_ + m) * 255.0) as u8;
    let b = ((b_ + m) * 255.0) as u8;

    let luminance = 0.299 * (r as f64) + 0.587 * (g as f64) + 0.114 * (b as f64);
    let fg_color = if luminance > 128.0 {
        "#000000"
    } else {
        "#ffffff"
    };

    (
        format!("#{:02x}{:02x}{:02x}", r, g, b),
        fg_color.to_string(),
    )
}

fn inject_color_css_if_needed(source_text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source_text.hash(&mut hasher);
    let class_name = format!("src-color-{:x}", hasher.finish());

    INJECTED_COLORS.with(|injected| {
        let mut injected = injected.borrow_mut();
        if !injected.contains(&class_name) {
            let (bg, fg) = generate_color(source_text);

            GLOBAL_CSS.with(|css| {
                let mut css_str = css.borrow_mut();
                css_str.push_str(&format!(
                    ".{} {{ background-color: {}; color: {}; }}\n",
                    class_name, bg, fg
                ));

                GLOBAL_PROVIDER.with(|provider| {
                    provider.load_from_data(&css_str);
                    gtk::style_context_add_provider_for_display(
                        &gtk::gdk::Display::default().unwrap(),
                        provider,
                        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                });
            });

            injected.insert(class_name.clone());
        }
    });

    class_name
}

pub fn resolve_icon_name(pkg_icon: Option<&str>) -> Option<String> {
    if gtk::is_initialized_main_thread()
        && let Some(display) = gtk::gdk::Display::default()
    {
        let theme = gtk::IconTheme::for_display(&display);

        if let Some(icon) = pkg_icon {
            if theme.has_icon(icon) {
                return Some(icon.to_string());
            }
            let sym = format!("{}-symbolic", icon);
            if theme.has_icon(&sym) {
                return Some(sym);
            }
        }
    }
    None
}

pub fn build_window(app: &adw::Application, state: AppState) {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        theme.add_search_path("/var/lib/flatpak/exports/share/icons");
        if let Some(home) = std::env::var_os("HOME") {
            let mut p = std::path::PathBuf::from(home);
            p.push(".local/share/flatpak/exports/share/icons");
            if let Some(s) = p.to_str() {
                theme.add_search_path(s);
            }
        }
    }

    let toast_overlay = adw::ToastOverlay::new();
    let active_toast: Rc<RefCell<Option<adw::Toast>>> = Rc::new(RefCell::new(None));

    let header_bar = adw::HeaderBar::new();

    let refresh_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.set_tooltip_text(Some("Refresh Packages"));
    refresh_btn.set_cursor_from_name(Some("pointer"));

    let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    status_box.set_valign(gtk::Align::Center);
    status_box.set_margin_start(8);

    let dnf_badge = create_status_badge("DNF");
    let flatpak_badge = create_status_badge("Flatpak");
    let cargo_badge = create_status_badge("Cargo");

    status_box.append(&dnf_badge);
    status_box.append(&flatpak_badge);
    status_box.append(&cargo_badge);

    let update_badges = {
        let dnf_b = dnf_badge.clone();
        let fp_b = flatpak_badge.clone();
        let cr_b = cargo_badge.clone();
        let state = state.clone();
        move || {
            let statuses = state.get_statuses();
            if let Some(s) = statuses.get(&crate::models::PackageManager::Dnf) {
                update_status_badge(&dnf_b, "DNF", s);
            }
            if let Some(s) = statuses.get(&crate::models::PackageManager::Flatpak) {
                update_status_badge(&fp_b, "Flatpak", s);
            }
            if let Some(s) = statuses.get(&crate::models::PackageManager::Cargo) {
                update_status_badge(&cr_b, "Cargo", s);
            }
        }
    };

    let update_badges_for_store = update_badges.clone();
    state.packages.connect_items_changed(move |_, _, _, _| {
        update_badges_for_store();
    });

    let state_for_refresh = state.clone();
    let toast_overlay_refresh = toast_overlay.clone();
    let active_toast_refresh = active_toast.clone();
    let update_badges_refresh = update_badges.clone();
    refresh_btn.connect_clicked(glib::clone!(
        #[weak]
        refresh_btn,
        move |_| {
            let state = state_for_refresh.clone();
            if state.is_refreshing() {
                return;
            }
            let overlay = toast_overlay_refresh.clone();
            let active_t = active_toast_refresh.clone();
            let update_b = update_badges_refresh.clone();
            refresh_btn.set_sensitive(false);
            let btn = refresh_btn.clone();
            glib::spawn_future_local(async move {
                let summary_opt = state.fetch_all().await;
                btn.set_sensitive(true);
                update_b();
                if let Some(summary) = summary_opt {
                    let toast_msg = if summary.is_all_successful() {
                        format!(
                            "Refreshed successfully ({} packages, {} repositories)",
                            summary.total_packages, summary.total_repositories
                        )
                    } else {
                        format!("Refresh issues: {}", summary.failures_summary())
                    };
                    show_toast(&overlay, &active_t, adw::Toast::new(&toast_msg));
                }
            });
        }
    ));
    header_bar.pack_start(&refresh_btn);
    header_bar.pack_start(&status_box);

    let view_stack = adw::ViewStack::new();
    let view_switcher = adw::ViewSwitcher::builder()
        .stack(&view_stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    view_switcher.set_cursor_from_name(Some("pointer"));
    header_bar.set_title_widget(Some(&view_switcher));

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search packages...")
        .build();
    search_entry.set_width_request(300);

    header_bar.pack_end(&search_entry);

    let packages_page = build_packages_page(
        state.clone(),
        &search_entry,
        &toast_overlay,
        active_toast.clone(),
        &refresh_btn,
    );
    let page = view_stack.add_titled(&packages_page, Some("packages"), "Packages");
    page.set_icon_name(Some("view-app-grid-symbolic"));

    let repositories_page =
        build_repositories_page(state.clone(), &toast_overlay, active_toast.clone());
    let page = view_stack.add_titled(&repositories_page, Some("repositories"), "Repositories");
    page.set_icon_name(Some("folder-symbolic"));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&header_bar);
    content.append(&view_stack);
    toast_overlay.set_child(Some(&content));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("BubblegumGNOME")
        .default_width(1000)
        .default_height(800)
        .content(&toast_overlay)
        .build();

    search_entry.connect_search_changed(glib::clone!(
        #[weak]
        view_stack,
        move |entry| {
            if !entry.text().is_empty()
                && let Some(visible_child) = view_stack.visible_child_name()
                && visible_child != "packages"
            {
                view_stack.set_visible_child_name("packages");
            }
        }
    ));

    search_entry.set_key_capture_widget(Some(&window.clone().upcast::<gtk::Widget>()));

    let key_capture = gtk::EventControllerKey::new();
    let search_entry_clone = search_entry.clone();
    key_capture.connect_key_pressed(move |_, keyval, _, state| {
        let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let is_alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
        if !is_ctrl && !is_alt {
            if keyval == gtk::gdk::Key::BackSpace && !search_entry_clone.has_focus() {
                search_entry_clone.grab_focus();
            } else if let Some(c) = keyval.to_unicode()
                && !c.is_control()
                && !search_entry_clone.has_focus()
            {
                search_entry_clone.grab_focus();
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_capture);

    window.present();
    search_entry.grab_focus();

    let search_entry_clone2 = search_entry.clone();
    let state_clone = state.clone();
    let update_badges_initial = update_badges.clone();
    glib::spawn_future_local(async move {
        state_clone.fetch_all().await;
        update_badges_initial();
        println!(
            "Loaded {} packages and {} repositories",
            state_clone.packages.n_items(),
            state_clone.repositories.n_items()
        );

        // Wait a single frame to allow SingleSelection to populate, then reliably clear it.
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            search_entry_clone2.grab_focus();
            glib::ControlFlow::Break
        });
    });
}

pub fn format_uninstall_confirmation_body(pkg_name: &str, affected_packages: &[String]) -> String {
    if affected_packages.is_empty() {
        format!("Are you sure you want to uninstall {}?", pkg_name)
    } else {
        format!(
            "Are you sure you want to uninstall {}?\n\nThe following {} package(s) will also be deleted from your system:",
            pkg_name,
            affected_packages.len()
        )
    }
}

pub fn build_affected_packages_widget(
    affected: &[String],
    toast_overlay: Option<&adw::ToastOverlay>,
    active_toast: Option<&Rc<RefCell<Option<adw::Toast>>>>,
) -> Option<gtk::Widget> {
    if affected.is_empty() {
        return None;
    }

    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .valign(gtk::Align::Start)
        .build();

    for pkg_name in affected {
        let copy_icon = gtk::Image::builder()
            .icon_name("edit-copy-symbolic")
            .css_classes(vec!["dim-label".to_string()])
            .valign(gtk::Align::Center)
            .build();

        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(pkg_name))
            .activatable(true)
            .tooltip_text("Click to copy package name")
            .build();
        row.set_cursor_from_name(Some("pointer"));
        row.add_suffix(&copy_icon);

        let pkg_clone = pkg_name.clone();
        let copy_icon_clone = copy_icon.clone();
        let overlay_clone = toast_overlay.cloned();
        let active_toast_clone = active_toast.cloned();

        row.connect_activated(move |r| {
            r.clipboard().set_text(&pkg_clone);
            copy_icon_clone.set_icon_name(Some("object-select-symbolic"));
            let icon_back = copy_icon_clone.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                icon_back.set_icon_name(Some("edit-copy-symbolic"));
            });
            if let (Some(overlay), Some(active_toast)) = (&overlay_clone, &active_toast_clone) {
                show_toast(
                    overlay,
                    active_toast,
                    adw::Toast::new("Copied to clipboard!"),
                );
            }
        });

        list_box.append(&row);
    }

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .max_content_height(200)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .vexpand(false)
        .valign(gtk::Align::Start)
        .child(&list_box)
        .build();

    Some(scrolled.upcast())
}

pub fn build_copy_output_button(copy_icon: &gtk::Image) -> gtk::Button {
    let copy_btn_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    let copy_btn_label = gtk::Label::new(Some("Copy Output"));
    copy_btn_box.append(&copy_btn_label);
    copy_btn_box.append(copy_icon);

    let copy_btn = gtk::Button::builder()
        .child(&copy_btn_box)
        .tooltip_text("Copy output to clipboard")
        .build();
    copy_btn.set_cursor_from_name(Some("pointer"));
    copy_btn
}

pub fn open_uninstall_log_dialog(
    parent: &gtk::Window,
    state: AppState,
    pkg: Package,
    refresh_btn: Option<&gtk::Button>,
) {
    let dialog = adw::Dialog::builder()
        .title("Uninstalling")
        .content_width(680)
        .content_height(500)
        .build();

    let dialog_toast_overlay = adw::ToastOverlay::new();
    let dialog_active_toast: Rc<RefCell<Option<adw::Toast>>> = Rc::new(RefCell::new(None));
    let toolbar_view = adw::ToolbarView::new();

    let header_bar = adw::HeaderBar::new();
    let window_title = adw::WindowTitle::builder().title("Uninstalling").build();
    header_bar.set_title_widget(Some(&window_title));
    toolbar_view.add_top_bar(&header_bar);

    let content_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(14)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let status_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .valign(gtk::Align::Center)
        .build();

    let pm_label = gtk::Label::builder()
        .label(pkg.manager.to_string())
        .css_classes(vec![
            "pm-label".to_string(),
            match pkg.manager {
                crate::models::PackageManager::Dnf => "pm-dnf".to_string(),
                crate::models::PackageManager::Flatpak => "pm-flatpak".to_string(),
                crate::models::PackageManager::Cargo => "pm-cargo".to_string(),
            },
        ])
        .build();

    let pkg_info_label = gtk::Label::builder()
        .label(format!("{} v{}", pkg.name, pkg.version))
        .css_classes(vec!["title-4".to_string()])
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();

    let status_badge = gtk::Label::builder()
        .label("RUNNING")
        .css_classes(vec![
            "status-badge".to_string(),
            "status-loading".to_string(),
        ])
        .build();

    let spinner = gtk::Spinner::builder().spinning(true).build();

    status_box.append(&pm_label);
    status_box.append(&pkg_info_label);
    status_box.append(&status_badge);
    status_box.append(&spinner);
    content_box.append(&status_box);

    let text_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .left_margin(12)
        .right_margin(12)
        .top_margin(12)
        .bottom_margin(12)
        .build();

    let text_buffer = text_view.buffer();

    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .min_content_height(280)
        .child(&text_view)
        .css_classes(vec!["log-terminal".to_string()])
        .build();

    content_box.append(&scrolled_window);

    let action_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .build();

    let copy_icon = gtk::Image::builder()
        .icon_name("edit-copy-symbolic")
        .build();
    let copy_btn = build_copy_output_button(&copy_icon);

    let ok_btn = gtk::Button::builder()
        .label("OK")
        .css_classes(vec!["suggested-action".to_string(), "pill".to_string()])
        .sensitive(false)
        .build();
    ok_btn.set_cursor_from_name(Some("pointer"));

    action_box.append(&copy_btn);
    action_box.append(&ok_btn);
    content_box.append(&action_box);

    toolbar_view.set_content(Some(&content_box));
    dialog_toast_overlay.set_child(Some(&toolbar_view));
    dialog.set_child(Some(&dialog_toast_overlay));

    let dialog_overlay_clone = dialog_toast_overlay.clone();
    let dialog_active_toast_clone = dialog_active_toast.clone();
    let copy_icon_clone = copy_icon.clone();
    copy_btn.connect_clicked(glib::clone!(
        #[weak]
        text_buffer,
        move |btn| {
            let (start, end) = text_buffer.bounds();
            let text = text_buffer.text(&start, &end, false);
            btn.clipboard().set_text(text.as_str());
            copy_icon_clone.set_icon_name(Some("object-select-symbolic"));
            let icon_back = copy_icon_clone.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                icon_back.set_icon_name(Some("edit-copy-symbolic"));
            });
            show_toast(
                &dialog_overlay_clone,
                &dialog_active_toast_clone,
                adw::Toast::new("Copied output to clipboard!"),
            );
        }
    ));

    let refresh_btn_clone = refresh_btn.cloned();
    let already_refreshed = Rc::new(RefCell::new(false));
    let trigger_refresh = move || {
        if *already_refreshed.borrow() {
            return;
        }
        *already_refreshed.borrow_mut() = true;
        if let Some(btn) = &refresh_btn_clone {
            btn.emit_clicked();
        }
    };

    let refresh_on_ok = trigger_refresh.clone();
    ok_btn.connect_clicked(glib::clone!(
        #[weak]
        dialog,
        move |_| {
            refresh_on_ok();
            dialog.close();
        }
    ));

    dialog.connect_closed(move |_| {
        trigger_refresh();
    });

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();

    glib::spawn_future_local(glib::clone!(
        #[weak]
        text_buffer,
        #[weak]
        text_view,
        async move {
            while let Some(line) = receiver.recv().await {
                let mut end_iter = text_buffer.end_iter();
                text_buffer.insert(&mut end_iter, &format!("{}\n", line));
                let mark = text_buffer.create_mark(None, &text_buffer.end_iter(), false);
                text_view.scroll_to_mark(&mark, 0.0, true, 0.0, 1.0);
            }
        }
    ));

    dialog.present(Some(parent));

    let state_exec = state.clone();

    glib::spawn_future_local(async move {
        let result = state_exec
            .uninstall_with_logs(&pkg, move |line| {
                sender.send(line).ok();
            })
            .await;

        spinner.set_spinning(false);
        spinner.set_visible(false);
        ok_btn.set_sensitive(true);

        match result {
            Ok(()) => {
                status_badge.set_label("FINISHED");
                status_badge.remove_css_class("status-loading");
                status_badge.add_css_class("status-loaded");
            }
            Err(err) => {
                status_badge.set_label("FAILED");
                status_badge.remove_css_class("status-loading");
                status_badge.add_css_class("status-failed");
                let mut end_iter = text_buffer.end_iter();
                text_buffer.insert(&mut end_iter, &format!("\n[Error]: {}\n", err));
            }
        }
    });
}

fn build_packages_page(
    state: AppState,
    search_entry: &gtk::SearchEntry,
    toast_overlay: &adw::ToastOverlay,
    active_toast: Rc<RefCell<Option<adw::Toast>>>,
    refresh_btn: &gtk::Button,
) -> gtk::Widget {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(move |_, list_item| {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("card");
        card.set_cursor_from_name(Some("pointer"));
        card.set_margin_start(3);
        card.set_margin_end(3);
        card.set_margin_top(3);
        card.set_margin_bottom(3);
        card.set_width_request(180);
        card.set_height_request(96);
        card.set_hexpand(true);

        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 4);
        box_.set_margin_start(8);
        box_.set_margin_end(8);
        box_.set_margin_top(8);
        box_.set_margin_bottom(8);

        let icon_img = gtk::Image::builder().pixel_size(24).build();

        let name_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .lines(1)
            .css_classes(["title-4"].to_vec())
            .build();

        let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header_box.set_halign(gtk::Align::Start);
        header_box.append(&icon_img);
        header_box.append(&name_label);

        let badges_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        badges_box.set_halign(gtk::Align::Start);

        let pm_label = gtk::Label::builder()
            .css_classes(["pm-label"].to_vec())
            .build();

        let source_label = gtk::Label::builder()
            .css_classes(["source-label"].to_vec())
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .lines(1)
            .max_width_chars(15)
            .build();

        badges_box.append(&pm_label);
        badges_box.append(&source_label);

        let dep_label = gtk::Label::builder()
            .css_classes(["dep-badge"].to_vec())
            .label("Dependency")
            .build();
        badges_box.append(&dep_label);

        let version_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .lines(1)
            .css_classes(["dim-label"].to_vec())
            .build();

        box_.append(&header_box);
        box_.append(&badges_box);
        box_.append(&version_label);

        card.append(&box_);

        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        list_item.set_child(Some(&card));
    });

    factory.connect_bind(move |_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let card = list_item.child().unwrap().downcast::<gtk::Box>().unwrap();
        let box_ = card.first_child().unwrap().downcast::<gtk::Box>().unwrap();

        let header_box = box_.first_child().unwrap().downcast::<gtk::Box>().unwrap();
        let badges_box = header_box
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();

        let icon_img = header_box
            .first_child()
            .unwrap()
            .downcast::<gtk::Image>()
            .unwrap();
        let name_label = icon_img
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let version_label = badges_box
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();

        let pm_label = badges_box
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let source_label = pm_label
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let dep_label = source_label
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();

        let item = list_item.item().unwrap();
        let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
        let pkg = boxed.borrow::<Package>();

        name_label.set_text(&pkg.name);

        let icon_name = pkg.icon.as_deref();
        if let Some(name) = &icon_name {
            icon_img.set_icon_name(Some(name));
            icon_img.set_visible(true);
        } else {
            icon_img.set_visible(false);
        }

        let (pm_text, pm_class) = match pkg.manager {
            crate::models::PackageManager::Dnf => ("DNF", "pm-dnf"),
            crate::models::PackageManager::Flatpak => ("Flatpak", "pm-flatpak"),
            crate::models::PackageManager::Cargo => ("Cargo", "pm-cargo"),
        };
        pm_label.set_text(pm_text);

        card.remove_css_class("pm-dnf");
        card.remove_css_class("pm-flatpak");
        card.remove_css_class("pm-cargo");
        card.add_css_class(pm_class);

        let source_text = pkg.source.as_deref().unwrap_or("Unknown");
        source_label.set_text(source_text);

        let class_name = inject_color_css_if_needed(source_text);
        source_label.set_css_classes(&["source-label", &class_name]);

        if pkg.is_dependency {
            dep_label.set_text("Dependency");
            dep_label.remove_css_class("user-badge");
            dep_label.add_css_class("dep-badge");
        } else {
            dep_label.set_text("User");
            dep_label.remove_css_class("dep-badge");
            dep_label.add_css_class("user-badge");
        }
        dep_label.set_visible(true);

        version_label.set_text(&pkg.version);
    });

    let search_query = Arc::new(Mutex::new(String::new()));
    let filter = gtk::CustomFilter::new({
        let search_query = search_query.clone();
        move |item| {
            let query = search_query.lock().unwrap();
            let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
            let pkg = boxed.borrow::<Package>();
            package_matches_query(&pkg, &query)
        }
    });

    let filter_model =
        gtk::FilterListModel::new(Some(state.packages.clone()), Some(filter.clone()));
    let selection_model = gtk::SingleSelection::builder()
        .model(&filter_model)
        .autoselect(false)
        .can_unselect(true)
        .build();

    let search_timeout = Arc::new(Mutex::new(None::<glib::SourceId>));
    search_entry.connect_search_changed({
        let search_query = search_query.clone();
        let filter = filter.clone();
        let search_timeout = search_timeout.clone();
        move |entry| {
            let text = entry.text().to_string();
            let mut timeout = search_timeout.lock().unwrap();
            if let Some(source_id) = timeout.take() {
                source_id.remove();
            }
            let search_query = search_query.clone();
            let filter = filter.clone();
            let search_timeout_inner = search_timeout.clone();
            *timeout = Some(glib::timeout_add_local(
                std::time::Duration::from_millis(150),
                move || {
                    *search_query.lock().unwrap() = text.clone();
                    filter.changed(gtk::FilterChange::Different);
                    *search_timeout_inner.lock().unwrap() = None;
                    glib::ControlFlow::Break
                },
            ));
        }
    });

    let grid_view = gtk::GridView::builder()
        .model(&selection_model)
        .factory(&factory)
        .max_columns(8)
        .min_columns(4)
        .vscroll_policy(gtk::ScrollablePolicy::Natural)
        .enable_rubberband(false)
        .css_classes(["packages-grid"].to_vec())
        .build();

    let key_ctrl = gtk::EventControllerKey::new();
    let grid_weak = grid_view.downgrade();
    let sel_model_for_key = selection_model.clone();
    key_ctrl.connect_key_pressed(move |_, keyval, _, _| {
        let grid_view = match grid_weak.upgrade() {
            Some(view) => view,
            None => return glib::Propagation::Proceed,
        };
        use gtk::gdk::Key;
        if keyval == Key::Down || keyval == Key::Right {
            if let Some(first_child) = grid_view.first_child() {
                first_child.grab_focus();
            } else {
                grid_view.grab_focus();
            }
            sel_model_for_key.set_selected(0);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    search_entry.add_controller(key_ctrl);

    let search_focus_ctrl = gtk::EventControllerFocus::new();
    search_focus_ctrl.connect_enter(glib::clone!(
        #[weak]
        selection_model,
        move |_| {
            selection_model.set_autoselect(false);
            selection_model.set_selected(gtk::INVALID_LIST_POSITION);
        }
    ));
    search_entry.add_controller(search_focus_ctrl);

    let grid_focus_ctrl = gtk::EventControllerFocus::new();
    grid_focus_ctrl.connect_enter(glib::clone!(
        #[weak]
        selection_model,
        move |_| {
            selection_model.set_autoselect(true);
        }
    ));
    grid_view.add_controller(grid_focus_ctrl);

    let scrolled_window = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&grid_view)
        .vexpand(true)
        .build();

    let vadj = scrolled_window.vadjustment();
    vadj.set_step_increment(104.0); // Exact height of one card + margin

    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .valign(gtk::Align::End)
        .halign(gtk::Align::Center)
        .build();

    let detail_box = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    detail_box.add_css_class("detail-panel");
    detail_box.set_margin_start(24);
    detail_box.set_margin_end(24);
    detail_box.set_margin_top(12);
    detail_box.set_margin_bottom(36);

    let detail_labels_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let d_name = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes(["title-4"].to_vec())
        .build();
    let d_badges_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    d_badges_box.set_halign(gtk::Align::Start);
    let d_pm_label = gtk::Label::builder()
        .css_classes(["pm-label"].to_vec())
        .build();
    let d_source_label = gtk::Label::builder().build();
    let d_source_provider = gtk::CssProvider::new();

    let source_btn = gtk::Button::builder()
        .child(&d_source_label)
        .css_classes(["flat", "copy-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    source_btn.set_cursor_from_name(Some("pointer"));
    source_btn
        .style_context()
        .add_provider(&d_source_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    source_btn.connect_clicked(glib::clone!(
        #[weak]
        d_source_label,
        #[weak]
        toast_overlay,
        #[strong]
        active_toast,
        move |b| {
            let clipboard = b.clipboard();
            clipboard.set_text(&d_source_label.text());
            show_toast(
                &toast_overlay,
                &active_toast,
                adw::Toast::new("Copied to clipboard!"),
            );
        }
    ));

    let d_dep_label = gtk::Label::builder()
        .css_classes(["dep-badge"].to_vec())
        .label("Dependency")
        .build();
    d_badges_box.append(&d_pm_label);
    d_badges_box.append(&source_btn);
    d_badges_box.append(&d_dep_label);

    let d_version = gtk::Label::builder().halign(gtk::Align::Start).build();

    let d_name_btn = gtk::Button::builder()
        .child(&d_name)
        .css_classes(["flat", "copy-btn"].to_vec())
        .halign(gtk::Align::Start)
        .margin_bottom(4)
        .build();
    d_name_btn.set_cursor_from_name(Some("pointer"));
    d_name_btn.connect_clicked(glib::clone!(
        #[weak]
        d_name,
        #[weak]
        toast_overlay,
        #[strong]
        active_toast,
        move |b| {
            let clipboard = b.clipboard();
            clipboard.set_text(&d_name.text());
            show_toast(
                &toast_overlay,
                &active_toast,
                adw::Toast::new("Copied to clipboard!"),
            );
        }
    ));

    let version_btn = gtk::Button::builder()
        .child(&d_version)
        .css_classes(["flat", "copy-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    version_btn.set_cursor_from_name(Some("pointer"));
    version_btn.connect_clicked(glib::clone!(
        #[weak]
        d_version,
        #[weak]
        toast_overlay,
        #[strong]
        active_toast,
        move |b| {
            let clipboard = b.clipboard();
            clipboard.set_text(&d_version.text());
            show_toast(
                &toast_overlay,
                &active_toast,
                adw::Toast::new("Copied to clipboard!"),
            );
        }
    ));

    detail_labels_box.append(&d_name_btn);
    detail_labels_box.append(&d_badges_box);
    detail_labels_box.append(&version_btn);

    let d_description = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .selectable(true)
        .margin_start(8)
        .build();

    let d_size_date_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    d_size_date_box.set_margin_start(8);
    let d_size = gtk::Label::builder().halign(gtk::Align::Start).build();
    let d_date = gtk::Label::builder().halign(gtk::Align::Start).build();
    d_size_date_box.append(&d_size);
    d_size_date_box.append(&d_date);

    detail_labels_box.append(&d_description);
    detail_labels_box.append(&d_size_date_box);

    let uninstall_btn = gtk::Button::builder()
        .label("Uninstall")
        .css_classes(["destructive-action"].to_vec())
        .valign(gtk::Align::Center)
        .build();
    uninstall_btn.set_cursor_from_name(Some("pointer"));

    let d_icon_img = gtk::Image::builder().pixel_size(48).build();

    detail_box.append(&d_icon_img);
    detail_box.append(&detail_labels_box);
    detail_box.append(&uninstall_btn);
    revealer.set_child(Some(&detail_box));

    selection_model.connect_selected_item_notify(glib::clone!(
        #[weak]
        revealer,
        #[weak]
        detail_box,
        #[weak]
        d_name,
        #[weak]
        d_pm_label,
        #[weak]
        d_source_label,
        #[strong]
        d_source_provider,
        #[weak]
        d_version,
        #[weak]
        d_icon_img,
        #[weak]
        d_description,
        #[weak]
        d_size,
        #[weak]
        d_date,
        #[weak]
        d_size_date_box,
        #[weak]
        d_dep_label,
        move |model| {
            if let Some(item) = model.selected_item() {
                let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
                let pkg = boxed.borrow::<Package>();

                let icon_name = pkg.icon.as_deref();
                if let Some(name) = &icon_name {
                    d_icon_img.set_icon_name(Some(name));
                    d_icon_img.set_visible(true);
                } else {
                    d_icon_img.set_visible(false);
                }

                d_name.set_text(&pkg.name);

                let (pm_text, pm_class) = match pkg.manager {
                    crate::models::PackageManager::Dnf => ("DNF", "pm-dnf"),
                    crate::models::PackageManager::Flatpak => ("Flatpak", "pm-flatpak"),
                    crate::models::PackageManager::Cargo => ("Cargo", "pm-cargo"),
                };
                d_pm_label.set_text(pm_text);
                detail_box.remove_css_class("pm-dnf");
                detail_box.remove_css_class("pm-flatpak");
                detail_box.remove_css_class("pm-cargo");
                d_pm_label.remove_css_class("pm-dnf");
                d_pm_label.remove_css_class("pm-flatpak");
                d_pm_label.remove_css_class("pm-cargo");
                d_pm_label.add_css_class(pm_class);

                let source_text = pkg.source.as_deref().unwrap_or("Unknown");
                d_source_label.set_text(source_text);
                let (bg, fg) = crate::ui::generate_color(source_text);
                d_source_provider.load_from_data(&format!("* {{ background-color: {}; color: {}; }} *:hover {{ background-color: mix({}, black, 0.2); }}", bg, fg, bg));

                d_version.set_text(&pkg.version);
                if pkg.is_dependency {
                    d_dep_label.set_text("Dependency");
                    d_dep_label.remove_css_class("user-badge");
                    d_dep_label.add_css_class("dep-badge");
                } else {
                    d_dep_label.set_text("User");
                    d_dep_label.remove_css_class("dep-badge");
                    d_dep_label.add_css_class("user-badge");
                }
                d_dep_label.set_visible(true);

                if let Some(desc) = &pkg.description {
                    d_description.set_text(desc);
                    d_description.set_visible(true);
                } else {
                    d_description.set_visible(false);
                }

                let mut show_size_date = false;
                if let Some(sz) = &pkg.size {
                    d_size.set_text(&format!("Size: {}", sz));
                    d_size.set_visible(true);
                    show_size_date = true;
                } else {
                    d_size.set_visible(false);
                }

                if let Some(dt) = &pkg.install_date {
                    d_date.set_text(&format!("Installed: {}", dt));
                    d_date.set_visible(true);
                    show_size_date = true;
                } else {
                    d_date.set_visible(false);
                }
                d_size_date_box.set_visible(show_size_date);

                revealer.set_reveal_child(true);
            } else {
                revealer.set_reveal_child(false);
            }
        }
    ));

    let state_for_uninstall = state.clone();
    let overlay_for_uninstall = toast_overlay.clone();
    let refresh_btn_for_uninstall = refresh_btn.clone();

    uninstall_btn.connect_clicked(glib::clone!(
        #[weak]
        selection_model,
        #[weak]
        uninstall_btn,
        #[weak]
        revealer,
        #[strong]
        active_toast,
        move |_| {
            if state_for_uninstall.is_operating() {
                return;
            }
            if let Some(item) = selection_model.selected_item() {
                let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
                let pkg = boxed.borrow::<Package>().clone();

                let window = uninstall_btn
                    .root()
                    .unwrap()
                    .downcast::<gtk::Window>()
                    .unwrap();

                let state_clone = state_for_uninstall.clone();
                let overlay_clone = overlay_for_uninstall.clone();
                let active_toast_clone = active_toast.clone();

                uninstall_btn.set_sensitive(false);

                let refresh_btn_for_dialog = refresh_btn_for_uninstall.clone();
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    uninstall_btn,
                    #[weak]
                    window,
                    #[weak]
                    revealer,
                    #[weak]
                    selection_model,
                    async move {
                        let affected = state_clone
                            .check_uninstall_impact(&pkg)
                            .await
                            .unwrap_or_default();

                        uninstall_btn.set_sensitive(true);

                        let body = format_uninstall_confirmation_body(&pkg.name, &affected);
                        let mut dialog_builder = adw::AlertDialog::builder()
                            .heading("Uninstall Application")
                            .body(&body);

                        let extra_widget = build_affected_packages_widget(
                            &affected,
                            Some(&overlay_clone),
                            Some(&active_toast_clone),
                        );
                        if let Some(extra) = &extra_widget {
                            dialog_builder = dialog_builder.extra_child(extra);
                        }

                        let dialog = dialog_builder.build();

                        dialog.add_response("cancel", "Cancel");
                        dialog.add_response("uninstall", "Uninstall");
                        dialog.set_response_appearance(
                            "uninstall",
                            adw::ResponseAppearance::Destructive,
                        );
                        dialog.set_default_response(Some("cancel"));
                        dialog.set_close_response("cancel");

                        let state_for_dialog = state_clone.clone();
                        let pkg_for_dialog = pkg.clone();

                        dialog.choose(
                            Some(&window),
                            None::<&gtk::gio::Cancellable>,
                            glib::clone!(
                                #[weak]
                                window,
                                #[weak]
                                revealer,
                                #[weak]
                                selection_model,
                                move |choice| {
                                    if choice == "uninstall" {
                                        revealer.set_reveal_child(false);
                                        selection_model.unselect_all();
                                        open_uninstall_log_dialog(
                                            &window,
                                            state_for_dialog,
                                            pkg_for_dialog,
                                            Some(&refresh_btn_for_dialog),
                                        );
                                    }
                                }
                            ),
                        );
                    }
                ));
            }
        }
    ));

    let overlay = gtk::Overlay::builder().child(&scrolled_window).build();
    overlay.add_overlay(&revealer);

    overlay.upcast()
}

fn build_repositories_page(
    state: AppState,
    toast_overlay: &adw::ToastOverlay,
    active_toast: Rc<RefCell<Option<adw::Toast>>>,
) -> gtk::Widget {
    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .build();

    let overlay_clone = toast_overlay.clone();

    list_box.bind_model(Some(&state.repositories), move |item| {
        let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
        let repo = boxed.borrow::<Repository>();

        let expander = adw::ExpanderRow::builder().title("").build();
        expander.set_cursor_from_name(Some("pointer"));

        let prefix_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);

        let text_vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
        text_vbox.set_valign(gtk::Align::Center);

        let title_label = gtk::Label::builder()
            .label(&repo.name)
            .halign(gtk::Align::Start)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build();

        let class_name = inject_color_css_if_needed(&repo.name);

        let title_btn = gtk::Button::builder()
            .child(&title_label)
            .css_classes(["flat", "source-label", "repo-page-label", &class_name].to_vec())
            .halign(gtk::Align::Start)
            .margin_top(12)
            .build();
        title_btn.set_cursor_from_name(Some("pointer"));

        let title_clone = repo.name.clone();
        let overlay_btn_clone_title = overlay_clone.clone();
        let active_toast_btn_clone_title = active_toast.clone();
        title_btn.connect_clicked(move |btn| {
            btn.clipboard().set_text(&title_clone);
            show_toast(
                &overlay_btn_clone_title,
                &active_toast_btn_clone_title,
                adw::Toast::new("Copied to clipboard!"),
            );
        });

        text_vbox.append(&title_btn);

        let pm_label = gtk::Label::builder()
            .css_classes(["pm-label", "repo-page-label"].to_vec())
            .halign(gtk::Align::Start)
            .build();
        let (pm_text, pm_class) = match repo.manager {
            crate::models::PackageManager::Dnf => ("DNF", "pm-dnf"),
            crate::models::PackageManager::Flatpak => ("Flatpak", "pm-flatpak"),
            crate::models::PackageManager::Cargo => ("Cargo", "pm-cargo"),
        };
        pm_label.set_text(pm_text);
        pm_label.add_css_class(pm_class);

        text_vbox.append(&pm_label);

        prefix_box.append(&text_vbox);
        expander.add_prefix(&prefix_box);

        let switch = gtk::Switch::builder()
            .valign(gtk::Align::Center)
            .active(repo.enabled)
            .sensitive(false)
            .build();
        expander.add_suffix(&switch);

        if let Some(url) = &repo.url {
            let safe_url = glib::markup_escape_text(url);
            let url_row = adw::ActionRow::builder()
                .title("URL")
                .subtitle(safe_url)
                .activatable(true)
                .build();
            url_row.set_cursor_from_name(Some("pointer"));

            let url_clone = url.clone();
            let overlay_btn_clone = overlay_clone.clone();
            let active_toast_btn_clone = active_toast.clone();
            url_row.connect_activated(move |row| {
                row.clipboard().set_text(&url_clone);
                show_toast(
                    &overlay_btn_clone,
                    &active_toast_btn_clone,
                    adw::Toast::new("Copied to clipboard!"),
                );
            });

            expander.add_row(&url_row);
        }

        if let Some(path) = &repo.file_path {
            let safe_path = glib::markup_escape_text(path);
            let path_row = adw::ActionRow::builder()
                .title("File Path")
                .subtitle(safe_path)
                .activatable(true)
                .build();
            path_row.set_cursor_from_name(Some("pointer"));

            let path_clone = path.clone();
            let overlay_btn_clone = overlay_clone.clone();
            let active_toast_btn_clone = active_toast.clone();
            path_row.connect_activated(move |row| {
                row.clipboard().set_text(&path_clone);
                show_toast(
                    &overlay_btn_clone,
                    &active_toast_btn_clone,
                    adw::Toast::new("Copied to clipboard!"),
                );
            });

            expander.add_row(&path_row);
        }

        if let Some(date) = &repo.added_date {
            let safe_date = glib::markup_escape_text(&date.to_string());
            let date_row = adw::ActionRow::builder()
                .title("Added Date")
                .subtitle(safe_date)
                .build();
            expander.add_row(&date_row);
        }

        expander.upcast::<gtk::Widget>()
    });

    let pref_group = adw::PreferencesGroup::builder()
        .title("Configured Repositories")
        .description("View your configured package sources")
        .build();
    pref_group.add(&list_box);

    let pref_page = adw::PreferencesPage::builder().build();
    pref_page.add(&pref_group);

    pref_page.upcast()
}

pub fn package_matches_query(pkg: &Package, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let matcher = SkimMatcherV2::default();
    let search_text = format!(
        "{} {} {} {}",
        pkg.name,
        pkg.manager,
        pkg.version,
        pkg.source.as_deref().unwrap_or("")
    );
    matcher.fuzzy_match(&search_text, q).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PackageManager;

    #[test]
    fn test_generate_color_fedora() {
        let (bg, fg) = generate_color("Fedora");
        assert_eq!(bg, "@accent_bg_color");
        assert_eq!(fg, "@accent_fg_color");

        let (bg2, fg2) = generate_color("fedora");
        assert_eq!(bg2, "@accent_bg_color");
        assert_eq!(fg2, "@accent_fg_color");
    }

    #[test]
    fn test_generate_color_arbitrary() {
        for i in 0..500 {
            let seed = format!("seed-test-{}", i);
            let (bg, fg) = generate_color(&seed);
            assert!(bg.starts_with('#'));
            assert_eq!(bg.len(), 7);
            assert!(fg == "#000000" || fg == "#ffffff");
        }
    }

    #[test]
    fn test_resolve_icon_name_none() {
        assert_eq!(resolve_icon_name(None), None);
    }

    #[test]
    fn test_package_matches_query() {
        let pkg = Package {
            id: "org.mozilla.firefox".to_string(),
            name: "Firefox".to_string(),
            manager: PackageManager::Flatpak,
            version: "134.0".to_string(),
            source: Some("flathub".to_string()),
            icon: Some("org.mozilla.firefox".to_string()),
            description: Some("Web Browser".to_string()),
            size: Some("120 MB".to_string()),
            install_date: None,
            is_dependency: false,
            arch: None,
            branch: None,
            scope: None,
        };

        // Empty query matches all
        assert!(package_matches_query(&pkg, ""));
        assert!(package_matches_query(&pkg, "   "));

        // Match by name
        assert!(package_matches_query(&pkg, "fire"));
        assert!(package_matches_query(&pkg, "Firefox"));

        // Match by manager
        assert!(package_matches_query(&pkg, "flat"));

        // Match by version
        assert!(package_matches_query(&pkg, "134"));

        // Match by source
        assert!(package_matches_query(&pkg, "flathub"));

        // Non-matching query
        assert!(!package_matches_query(&pkg, "nonexistentxyz12345"));
    }

    #[test]
    fn test_gtk_ui_components_if_available() {
        if gtk::is_initialized_main_thread() {
            let state = AppState::new();
            let toast_overlay = adw::ToastOverlay::new();
            let active_toast = Rc::new(RefCell::new(None));
            let search_entry = gtk::SearchEntry::new();

            let _ = inject_color_css_if_needed("flathub");
            let _ = inject_color_css_if_needed("flathub");
            let _ = inject_color_css_if_needed("fedora");

            let _ = resolve_icon_name(Some("system-search"));
            let _ = resolve_icon_name(Some("nonexistent-icon-xyz"));

            let refresh_btn = gtk::Button::new();
            let packages_page = build_packages_page(
                state.clone(),
                &search_entry,
                &toast_overlay,
                active_toast.clone(),
                &refresh_btn,
            );
            assert!(packages_page.is::<gtk::Widget>());

            let repos_page =
                build_repositories_page(state.clone(), &toast_overlay, active_toast.clone());
            assert!(repos_page.is::<gtk::Widget>());
        }
    }

    #[test]
    fn test_format_uninstall_confirmation_body() {
        let empty_deps = Vec::new();
        let body_empty = format_uninstall_confirmation_body("ripgrep", &empty_deps);
        assert_eq!(body_empty, "Are you sure you want to uninstall ripgrep?");

        let deps = vec!["dep1".to_string(), "dep2".to_string()];
        let body_deps = format_uninstall_confirmation_body("git", &deps);
        assert!(body_deps.contains("git"));
        assert!(body_deps.contains("2 package(s) will also be deleted"));
    }

    #[test]
    fn test_build_affected_packages_widget_empty() {
        assert!(build_affected_packages_widget(&[], None, None).is_none());
    }

    #[test]
    fn test_build_affected_packages_widget_with_items() {
        if gtk::is_initialized_main_thread() {
            let toast_overlay = adw::ToastOverlay::new();
            let active_toast = Rc::new(RefCell::new(None));
            let pkgs = vec!["pkg1".to_string(), "pkg2".to_string()];
            let widget =
                build_affected_packages_widget(&pkgs, Some(&toast_overlay), Some(&active_toast));
            assert!(widget.is_some());
            let widget = widget.unwrap();
            let scrolled = widget
                .downcast_ref::<gtk::ScrolledWindow>()
                .expect("widget should be ScrolledWindow");
            assert_ne!(scrolled.min_content_height(), 140);
            assert!(scrolled.propagates_natural_height());
            assert_eq!(scrolled.max_content_height(), 200);
            assert!(!scrolled.vexpands());

            let child = scrolled.child().expect("scrolled has child");
            let list_box = child
                .downcast_ref::<gtk::ListBox>()
                .expect("child is ListBox");
            let mut count = 0;
            let mut current = list_box.first_child();
            while let Some(row_widget) = current {
                count += 1;
                let action_row = row_widget
                    .downcast_ref::<adw::ActionRow>()
                    .expect("row should be an ActionRow");
                assert!(action_row.is_activatable());
                assert_eq!(
                    action_row.tooltip_text().as_deref(),
                    Some("Click to copy package name")
                );
                action_row.emit_activate();
                current = row_widget.next_sibling();
            }
            assert_eq!(count, 2);
        }
    }

    #[test]
    fn test_open_uninstall_log_dialog_components() {
        if gtk::is_initialized_main_thread() {
            let state = AppState::new();
            let parent = gtk::Window::new();
            let refresh_btn = gtk::Button::new();
            let pkg = Package {
                id: "test-pkg".to_string(),
                name: "test-pkg".to_string(),
                manager: PackageManager::Dnf,
                version: "1.0".to_string(),
                source: None,
                icon: None,
                description: None,
                size: None,
                install_date: None,
                is_dependency: false,
                arch: None,
                branch: None,
                scope: None,
            };
            open_uninstall_log_dialog(&parent, state, pkg, Some(&refresh_btn));
        }
    }

    #[test]
    fn test_build_copy_output_button() {
        if gtk::is_initialized_main_thread() {
            let icon = gtk::Image::builder()
                .icon_name("edit-copy-symbolic")
                .build();
            let button = build_copy_output_button(&icon);
            let child = button.child().expect("button should have a child");
            let box_widget = child
                .downcast_ref::<gtk::Box>()
                .expect("child should be a Box");
            assert_eq!(box_widget.orientation(), gtk::Orientation::Horizontal);
            assert_eq!(box_widget.spacing(), 6);

            let first_child = box_widget.first_child().expect("box should have children");
            let label = first_child
                .downcast_ref::<gtk::Label>()
                .expect("first child is Label");
            assert_eq!(label.text().as_str(), "Copy Output");

            let second_child = first_child
                .next_sibling()
                .expect("box should have second child");
            let img = second_child
                .downcast_ref::<gtk::Image>()
                .expect("second child is Image");
            assert_eq!(img.icon_name().as_deref(), Some("edit-copy-symbolic"));
        }
    }
}
