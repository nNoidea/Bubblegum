use super::Backend;
use crate::models::{Package, PackageManager, Repository};
use anyhow::Result;
use std::process::Command;

pub struct FlatpakBackend;

impl Backend for FlatpakBackend {
    fn get_packages(&self) -> Result<Vec<Package>> {
        let output = Command::new("flatpak")
            .args(["list", "--app", "--columns=application,name,version,origin,description,size"])
            .output()?;

        let mut packages = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 4 {
                    let description = parts.get(4).filter(|s| !s.is_empty()).map(|s| s.trim().to_string());
                    let size = parts.get(5).filter(|s| !s.is_empty()).map(|s| s.trim().to_string());
                    
                    packages.push(Package {
                        id: parts[0].trim().to_string(),
                        name: parts[1].trim().to_string(),
                        manager: PackageManager::Flatpak,
                        version: parts[2].trim().to_string(),
                        source: Some(parts[3].trim().to_string()),
                        icon: Some(parts[0].trim().to_string()),
                        description,
                        size,
                        install_date: None,
                    });
                }
            }
        }
        Ok(packages)
    }

    fn get_repositories(&self) -> Result<Vec<Repository>> {
        let output = Command::new("flatpak")
            .args(["remotes", "--columns=name,url"])
            .output()?;

        let mut repos = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 2 {
                    repos.push(Repository {
                        id: parts[0].trim().to_string(),
                        name: parts[0].trim().to_string(),
                        manager: PackageManager::Flatpak,
                        enabled: true,
                        url: Some(parts[1].trim().to_string()),
                        file_path: None,
                        added_date: None,
                    });
                }
            }
        }
        Ok(repos)
    }
}
