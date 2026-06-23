use super::Backend;
use crate::models::{Package, PackageManager, Repository};
use anyhow::Result;
use std::process::Command;

pub struct CargoBackend;

impl Backend for CargoBackend {
    fn get_packages(&self) -> Result<Vec<Package>> {
        let output = Command::new("cargo")
            .args(["install", "--list"])
            .output()?;

        let mut packages = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if !line.starts_with(' ') && line.ends_with(':') {
                    let line = line.trim_end_matches(':');
                    if let Some((name, rest)) = line.split_once(' ') {
                        let rest = rest.trim();
                        let (version, source) = if let Some(idx) = rest.find('(') {
                            let v = rest[..idx].trim().to_string();
                            let s = rest[idx + 1..].trim_end_matches(')').to_string();
                            (v, s)
                        } else {
                            (rest.to_string(), "crates.io".to_string())
                        };

                        packages.push(Package {
                            id: name.to_string(),
                            name: name.to_string(),
                            manager: PackageManager::Cargo,
                            version,
                            source: Some(source),
                            icon: None,
                            description: None,
                            size: None,
                            install_date: None,
                        });
                    }
                }
            }
        }
        Ok(packages)
    }

    fn get_repositories(&self) -> Result<Vec<Repository>> {
        let mut repos = std::collections::HashSet::new();
        // Always include crates.io by default
        repos.insert("crates.io".to_string());

        let output = Command::new("cargo")
            .args(["install", "--list"])
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if !line.starts_with(' ') && line.ends_with(':') {
                    let line = line.trim_end_matches(':');
                    if let Some((_, rest)) = line.split_once(' ') {
                        let rest = rest.trim();
                        if let Some(idx) = rest.find('(') {
                            let s = rest[idx + 1..].trim_end_matches(')').to_string();
                            repos.insert(s);
                        }
                    }
                }
            }
        }

        let mut repositories = Vec::new();
        for repo_url in repos {
            let name = repo_url.clone();

            let url = if repo_url == "crates.io" {
                Some("https://crates.io".to_string())
            } else {
                Some(repo_url.clone())
            };

            repositories.push(Repository {
                id: repo_url.clone(),
                name,
                manager: PackageManager::Cargo,
                enabled: true,
                url,
                file_path: None,
                added_date: None,
            });
        }

        repositories.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(repositories)
    }
}
