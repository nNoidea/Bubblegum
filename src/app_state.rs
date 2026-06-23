use crate::backends::get_all_backends;

use gtk4::gio;
use gtk4::gio::prelude::ListModelExt;
use gtk4::glib;
use tokio::task;

#[derive(Clone)]
pub struct AppState {
    pub packages: gio::ListStore,
    pub repositories: gio::ListStore,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            packages: gio::ListStore::new::<glib::BoxedAnyObject>(),
            repositories: gio::ListStore::new::<glib::BoxedAnyObject>(),
        }
    }

    pub async fn fetch_all(&self) {
        let backends = get_all_backends();
        let mut tasks = Vec::new();

        for backend in backends {
            tasks.push(task::spawn_blocking(move || {
                let pkgs = backend.get_packages().unwrap_or_default();
                let repos = backend.get_repositories().unwrap_or_default();
                (pkgs, repos)
            }));
        }

        let mut all_pkgs = Vec::new();
        let mut all_repos = Vec::new();

        for task in tasks {
            if let Ok((mut p, mut r)) = task.await {
                all_pkgs.append(&mut p);
                all_repos.append(&mut r);
            }
        }

        for p in &mut all_pkgs {
            p.icon = crate::ui::resolve_icon_name(p.icon.as_deref());
        }

        all_pkgs.sort_by(|a, b| {
            a.is_dependency.cmp(&b.is_dependency)
                .then(b.icon.is_some().cmp(&a.icon.is_some()))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        let glib_pkgs: Vec<glib::BoxedAnyObject> = all_pkgs.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.packages.splice(0, self.packages.n_items(), &glib_pkgs);

        let glib_repos: Vec<glib::BoxedAnyObject> = all_repos.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.repositories.splice(0, self.repositories.n_items(), &glib_repos);
    }
}
