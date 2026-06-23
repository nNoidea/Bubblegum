use crate::app_state::AppState;
use crate::models::{Package, Repository};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use gtk::glib;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

thread_local! {
    static INJECTED_COLORS: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
    static GLOBAL_PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
    static GLOBAL_CSS: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
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

fn resolve_icon_name(pkg_icon: Option<&str>) -> String {
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        
        if let Some(icon) = pkg_icon {
            if theme.has_icon(icon) {
                return icon.to_string();
            }
            let sym = format!("{}-symbolic", icon);
            if theme.has_icon(&sym) {
                return sym;
            }
        }
        
        for fallback in &[
            "package-x-generic",
            "package-x-generic-symbolic",
            "system-software-install",
            "system-software-install-symbolic",
            "application-x-executable",
            "application-x-executable-symbolic"
        ] {
            if theme.has_icon(fallback) {
                return fallback.to_string();
            }
        }
    }
    String::from("application-x-executable")
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

    let header_bar = adw::HeaderBar::new();

    let refresh_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh_btn.set_tooltip_text(Some("Refresh Packages"));
    let state_for_refresh = state.clone();
    refresh_btn.connect_clicked(move |_| {
        let state = state_for_refresh.clone();
        glib::spawn_future_local(async move {
            let _ = state.fetch_all().await;
        });
    });
    header_bar.pack_start(&refresh_btn);

    let view_stack = adw::ViewStack::new();
    let view_switcher = adw::ViewSwitcher::builder()
        .stack(&view_stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .build();
    header_bar.set_title_widget(Some(&view_switcher));

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search packages...")
        .build();
    search_entry.set_width_request(300);

    header_bar.pack_end(&search_entry);

    let toast_overlay = adw::ToastOverlay::new();
    let active_toast: Arc<Mutex<Option<adw::Toast>>> = Arc::new(Mutex::new(None));

    let packages_page = build_packages_page(state.clone(), &search_entry, &toast_overlay, active_toast.clone());
    let page = view_stack.add_titled(&packages_page, Some("packages"), "Packages");
    page.set_icon_name(Some("view-app-grid-symbolic"));

    let repositories_page = build_repositories_page(state.clone(), &toast_overlay, active_toast.clone());
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

    search_entry.connect_search_changed(glib::clone!(#[weak] view_stack, move |entry| {
        if !entry.text().is_empty() {
            if let Some(visible_child) = view_stack.visible_child_name() {
                if visible_child != "packages" {
                    view_stack.set_visible_child_name("packages");
                }
            }
        }
    }));

    search_entry.set_key_capture_widget(Some(&window.clone().upcast::<gtk::Widget>()));

    let key_capture = gtk::EventControllerKey::new();
    let search_entry_clone = search_entry.clone();
    key_capture.connect_key_pressed(move |_, keyval, _, state| {
        let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let is_alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
        if !is_ctrl && !is_alt {
            if keyval == gtk::gdk::Key::BackSpace && !search_entry_clone.has_focus() {
                search_entry_clone.grab_focus();
            } else if let Some(c) = keyval.to_unicode() {
                if !c.is_control() && !search_entry_clone.has_focus() {
                    search_entry_clone.grab_focus();
                }
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_capture);

    window.present();
    search_entry.grab_focus();

    let search_entry_clone2 = search_entry.clone();
    let state_clone = state.clone();
    glib::spawn_future_local(async move {
        state_clone.fetch_all().await;
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

fn build_packages_page(
    state: AppState,
    search_entry: &gtk::SearchEntry,
    toast_overlay: &adw::ToastOverlay,
    active_toast: Arc<Mutex<Option<adw::Toast>>>,
) -> gtk::Widget {
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(move |_, list_item| {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("card");
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

        let icon_img = gtk::Image::builder()
            .pixel_size(24)
            .build();

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

        let header_box = box_
            .first_child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
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

        let item = list_item.item().unwrap();
        let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
        let pkg = boxed.borrow::<Package>();

        name_label.set_text(&pkg.name);
        
        let icon_name = resolve_icon_name(pkg.icon.as_deref());
        icon_img.set_icon_name(Some(&icon_name));

        let (pm_text, pm_class) = match pkg.manager {
            crate::models::PackageManager::Dnf => ("DNF", "pm-dnf"),
            crate::models::PackageManager::Flatpak => ("Flatpak", "pm-flatpak"),
            crate::models::PackageManager::Cargo => ("Cargo", "pm-cargo"),
        };
        pm_label.set_text(pm_text);

        pm_label.remove_css_class("pm-dnf");
        pm_label.remove_css_class("pm-flatpak");
        pm_label.remove_css_class("pm-cargo");
        pm_label.add_css_class(pm_class);

        let source_text = pkg.source.as_deref().unwrap_or("Unknown");
        source_label.set_text(source_text);

        let class_name = inject_color_css_if_needed(source_text);
        source_label.set_css_classes(&["source-label", &class_name]);

        version_label.set_text(&pkg.version);
    });

    let search_query = Arc::new(Mutex::new(String::new()));
    let filter = gtk::CustomFilter::new({
        let search_query = search_query.clone();
        let matcher = SkimMatcherV2::default();
        move |item| {
            let query = search_query.lock().unwrap();
            if query.is_empty() {
                return true;
            }
            let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
            let pkg = boxed.borrow::<Package>();

            let search_text = format!(
                "{} {} {} {}",
                pkg.name,
                pkg.manager,
                pkg.version,
                pkg.source.as_deref().unwrap_or("")
            );
            matcher.fuzzy_match(&search_text, &query).is_some()
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
    let d_source_label = gtk::Label::builder()
        .css_classes(["source-label"].to_vec())
        .build();
    let d_source_provider = gtk::CssProvider::new();
    d_source_label.style_context().add_provider(&d_source_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let pm_btn = gtk::Button::builder()
        .child(&d_pm_label)
        .css_classes(["flat", "compact-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    pm_btn.connect_clicked(glib::clone!(#[weak] d_pm_label, #[weak] toast_overlay, #[strong] active_toast, move |b| {
        let clipboard = b.clipboard();
        clipboard.set_text(&d_pm_label.text());
        if let Some(prev) = active_toast.lock().unwrap().take() {
            prev.dismiss();
        }
        let t = adw::Toast::new("Package manager copied!");
        *active_toast.lock().unwrap() = Some(t.clone());
        toast_overlay.add_toast(t);
    }));

    let source_btn = gtk::Button::builder()
        .child(&d_source_label)
        .css_classes(["flat", "compact-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    source_btn.connect_clicked(glib::clone!(#[weak] d_source_label, #[weak] toast_overlay, #[strong] active_toast, move |b| {
        let clipboard = b.clipboard();
        clipboard.set_text(&d_source_label.text());
        if let Some(prev) = active_toast.lock().unwrap().take() {
            prev.dismiss();
        }
        let t = adw::Toast::new("Repository source copied!");
        *active_toast.lock().unwrap() = Some(t.clone());
        toast_overlay.add_toast(t);
    }));

    d_badges_box.append(&pm_btn);
    d_badges_box.append(&source_btn);
    let d_version = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .build();

    let d_name_btn = gtk::Button::builder()
        .child(&d_name)
        .css_classes(["flat", "compact-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    d_name_btn.connect_clicked(glib::clone!(#[weak] d_name, #[weak] toast_overlay, #[strong] active_toast, move |b| {
        let clipboard = b.clipboard();
        clipboard.set_text(&d_name.text());
        if let Some(prev) = active_toast.lock().unwrap().take() {
            prev.dismiss();
        }
        let t = adw::Toast::new("Package name copied!");
        *active_toast.lock().unwrap() = Some(t.clone());
        toast_overlay.add_toast(t);
    }));

    let version_btn = gtk::Button::builder()
        .child(&d_version)
        .css_classes(["flat", "compact-btn"].to_vec())
        .halign(gtk::Align::Start)
        .build();
    version_btn.connect_clicked(glib::clone!(#[weak] d_version, #[weak] toast_overlay, #[strong] active_toast, move |b| {
        let clipboard = b.clipboard();
        clipboard.set_text(&d_version.text());
        if let Some(prev) = active_toast.lock().unwrap().take() {
            prev.dismiss();
        }
        let t = adw::Toast::new("Version copied!");
        *active_toast.lock().unwrap() = Some(t.clone());
        toast_overlay.add_toast(t);
    }));

    detail_labels_box.append(&d_name_btn);
    detail_labels_box.append(&d_badges_box);
    detail_labels_box.append(&version_btn);

    let uninstall_btn = gtk::Button::builder()
        .label("Uninstall")
        .css_classes(["destructive-action"].to_vec())
        .valign(gtk::Align::Center)
        .build();

    let d_icon_img = gtk::Image::builder()
        .pixel_size(48)
        .build();

    detail_box.append(&d_icon_img);
    detail_box.append(&detail_labels_box);
    detail_box.append(&uninstall_btn);
    revealer.set_child(Some(&detail_box));

    selection_model.connect_selected_item_notify(glib::clone!(
        #[weak]
        revealer,
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
        move |model| {
            if let Some(item) = model.selected_item() {
                let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
                let pkg = boxed.borrow::<Package>();
                
                let icon_name = resolve_icon_name(pkg.icon.as_deref());
                d_icon_img.set_icon_name(Some(&icon_name));

                d_name.set_text(&pkg.name);
                
                let (pm_text, pm_class) = match pkg.manager {
                    crate::models::PackageManager::Dnf => ("DNF", "pm-dnf"),
                    crate::models::PackageManager::Flatpak => ("Flatpak", "pm-flatpak"),
                    crate::models::PackageManager::Cargo => ("Cargo", "pm-cargo"),
                };
                d_pm_label.set_text(pm_text);
                d_pm_label.remove_css_class("pm-dnf");
                d_pm_label.remove_css_class("pm-flatpak");
                d_pm_label.remove_css_class("pm-cargo");
                d_pm_label.add_css_class(pm_class);

                let source_text = pkg.source.as_deref().unwrap_or("Unknown");
                d_source_label.set_text(source_text);
                let (bg, fg) = crate::ui::generate_color(source_text);
                d_source_provider.load_from_data(&format!("label {{ background-color: {}; color: {}; }}", bg, fg));

                d_version.set_text(&pkg.version);
                revealer.set_reveal_child(true);
            } else {
                revealer.set_reveal_child(false);
            }
        }
    ));

    let state_for_uninstall = state.clone();
    let overlay_for_uninstall = toast_overlay.clone();

    uninstall_btn.connect_clicked(glib::clone!(
        #[weak]
        selection_model,
        #[strong]
        active_toast,
        move |btn| {
            if let Some(item) = selection_model.selected_item() {
                let boxed = item.downcast_ref::<glib::BoxedAnyObject>().unwrap();
                let pkg = boxed.borrow::<Package>().clone();

                let window = btn.root().unwrap().downcast::<gtk::Window>().unwrap();
                let dialog = adw::AlertDialog::builder()
                    .heading("Uninstall Application")
                    .body(format!("Are you sure you want to uninstall {}?", pkg.name))
                    .build();

                dialog.add_response("cancel", "Cancel");
                dialog.add_response("uninstall", "Uninstall");
                dialog.set_response_appearance("uninstall", adw::ResponseAppearance::Destructive);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");

                let state_clone = state_for_uninstall.clone();
                let overlay_clone = overlay_for_uninstall.clone();
                let active_toast_for_dialog = active_toast.clone();

                dialog.choose(
                    Some(&window),
                    None::<&gtk::gio::Cancellable>,
                    move |choice| {
                        if choice == "uninstall" {
                            if let Some(prev) = active_toast_for_dialog.lock().unwrap().take() {
                                prev.dismiss();
                            }
                            let loading_toast = adw::Toast::new(&format!("Uninstalling {}...", pkg.name));
                            loading_toast.set_timeout(0); // keep it until dismissed
                            overlay_clone.add_toast(loading_toast.clone());

                            let overlay_clone2 = overlay_clone.clone();
                            let state_clone2 = state_clone.clone();
                            let active_toast2 = active_toast_for_dialog.clone();

                            glib::spawn_future_local(async move {
                                let result = tokio::task::spawn_blocking(move || {
                                    use crate::models::PackageManager;
                                    use std::process::Command;
                                    match pkg.manager {
                                        PackageManager::Dnf => Command::new("pkexec")
                                            .args(["dnf", "remove", "-y", &pkg.id])
                                            .output(),
                                        PackageManager::Flatpak => Command::new("flatpak")
                                            .args(["uninstall", "-y", &pkg.id])
                                            .output(),
                                        PackageManager::Cargo => Command::new("cargo")
                                            .args(["uninstall", &pkg.id])
                                            .output(),
                                    }
                                })
                                .await;

                                loading_toast.dismiss();

                                if let Some(prev) = active_toast2.lock().unwrap().take() {
                                    prev.dismiss();
                                }

                                match result {
                                    Ok(Ok(output)) if output.status.success() => {
                                        let success_toast = adw::Toast::new(&format!(
                                            "{} deleted successfully!",
                                            pkg.name
                                        ));
                                        *active_toast2.lock().unwrap() = Some(success_toast.clone());
                                        overlay_clone2.add_toast(success_toast);

                                        glib::spawn_future_local(async move {
                                            state_clone2.fetch_all().await;
                                        });
                                    }
                                    _ => {
                                        let error_toast = adw::Toast::new(&format!(
                                            "Failed to uninstall {}",
                                            pkg.name
                                        ));
                                        *active_toast2.lock().unwrap() = Some(error_toast.clone());
                                        overlay_clone2.add_toast(error_toast);
                                    }
                                }
                            });
                        }
                    },
                );
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
    active_toast: Arc<Mutex<Option<adw::Toast>>>,
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
        title_label.set_css_classes(&["source-label", "repo-page-label", &class_name]);

        let title_btn = gtk::Button::builder()
            .child(&title_label)
            .css_classes(["flat"].to_vec())
            .halign(gtk::Align::Start)
            .margin_top(12)
            .build();
        
        let title_clone = repo.name.clone();
        let overlay_btn_clone_title = overlay_clone.clone();
        let active_toast_btn_clone_title = active_toast.clone();
        title_btn.connect_clicked(move |btn| {
            btn.clipboard().set_text(&title_clone);
            if let Some(old_toast) = active_toast_btn_clone_title.lock().unwrap().take() {
                old_toast.dismiss();
            }
            let t = adw::Toast::new("Copied to clipboard!");
            *active_toast_btn_clone_title.lock().unwrap() = Some(t.clone());
            overlay_btn_clone_title.add_toast(t);
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

        let pm_btn = gtk::Button::builder()
            .child(&pm_label)
            .css_classes(["flat"].to_vec())
            .halign(gtk::Align::Start)
            .margin_bottom(12)
            .build();

        let pm_clone = pm_text.to_string();
        let overlay_btn_clone_pm = overlay_clone.clone();
        let active_toast_btn_clone_pm = active_toast.clone();
        pm_btn.connect_clicked(move |btn| {
            btn.clipboard().set_text(&pm_clone);
            if let Some(old_toast) = active_toast_btn_clone_pm.lock().unwrap().take() {
                old_toast.dismiss();
            }
            let t = adw::Toast::new("Copied to clipboard!");
            *active_toast_btn_clone_pm.lock().unwrap() = Some(t.clone());
            overlay_btn_clone_pm.add_toast(t);
        });

        text_vbox.append(&pm_btn);

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

            let url_clone = url.clone();
            let overlay_btn_clone = overlay_clone.clone();
            let active_toast_btn_clone = active_toast.clone();
            url_row.connect_activated(move |row| {
                row.clipboard().set_text(&url_clone);
                if let Some(old_toast) = active_toast_btn_clone.lock().unwrap().take() {
                    old_toast.dismiss();
                }
                let t = adw::Toast::new("Copied to clipboard!");
                *active_toast_btn_clone.lock().unwrap() = Some(t.clone());
                overlay_btn_clone.add_toast(t);
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

            let path_clone = path.clone();
            let overlay_btn_clone = overlay_clone.clone();
            let active_toast_btn_clone = active_toast.clone();
            path_row.connect_activated(move |row| {
                row.clipboard().set_text(&path_clone);
                if let Some(old_toast) = active_toast_btn_clone.lock().unwrap().take() {
                    old_toast.dismiss();
                }
                let t = adw::Toast::new("Copied to clipboard!");
                *active_toast_btn_clone.lock().unwrap() = Some(t.clone());
                overlay_btn_clone.add_toast(t);
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
