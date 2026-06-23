use crate::backend::{Backend, CargoBackend, DnfBackend, FlatpakBackend};

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
        let (flatpak_p, cargo_p, dnf_p) = tokio::join!(
            task::spawn_blocking(|| FlatpakBackend.get_packages()),
            task::spawn_blocking(|| CargoBackend.get_packages()),
            task::spawn_blocking(|| DnfBackend.get_packages()),
        );

        let mut pkgs = Vec::new();
        if let Ok(Ok(mut p)) = flatpak_p {
            pkgs.append(&mut p);
        }
        if let Ok(Ok(mut p)) = cargo_p {
            pkgs.append(&mut p);
        }
        if let Ok(Ok(mut p)) = dnf_p {
            pkgs.append(&mut p);
        }

        let glib_pkgs: Vec<glib::BoxedAnyObject> = pkgs.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.packages.splice(0, self.packages.n_items(), &glib_pkgs);

        let (flatpak_r, cargo_r, dnf_r) = tokio::join!(
            task::spawn_blocking(|| FlatpakBackend.get_repositories()),
            task::spawn_blocking(|| CargoBackend.get_repositories()),
            task::spawn_blocking(|| DnfBackend.get_repositories()),
        );

        let mut repos = Vec::new();
        if let Ok(Ok(mut r)) = flatpak_r {
            repos.append(&mut r);
        }
        if let Ok(Ok(mut r)) = cargo_r {
            repos.append(&mut r);
        }
        if let Ok(Ok(mut r)) = dnf_r {
            repos.append(&mut r);
        }

        let glib_repos: Vec<glib::BoxedAnyObject> = repos.into_iter().map(glib::BoxedAnyObject::new).collect();
        self.repositories.splice(0, self.repositories.n_items(), &glib_repos);
    }
}
