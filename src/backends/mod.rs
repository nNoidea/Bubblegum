pub mod cargo;
pub mod dnf;
pub mod flatpak;

use crate::models::{Package, Repository};
use anyhow::Result;

pub trait Backend: Send + 'static {
    fn get_packages(&self) -> Result<Vec<Package>>;
    fn get_repositories(&self) -> Result<Vec<Repository>>;
}

pub fn get_all_backends() -> Vec<Box<dyn Backend>> {
    vec![
        Box::new(flatpak::FlatpakBackend),
        Box::new(cargo::CargoBackend),
        Box::new(dnf::DnfBackend),
    ]
}
