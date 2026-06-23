use crate::models::{Package, PackageManager, Repository};
use anyhow::Result;
use std::fs;
use std::process::Command;

pub trait Backend {
    fn get_packages(&self) -> Result<Vec<Package>>;
    fn get_repositories(&self) -> Result<Vec<Repository>>;
}

pub struct FlatpakBackend;
pub struct CargoBackend;
pub struct DnfBackend;

impl Backend for FlatpakBackend {
    fn get_packages(&self) -> Result<Vec<Package>> {
        let output = Command::new("flatpak")
            .args(["list", "--app", "--columns=application,name,version,origin"])
            .output()?;

        let mut packages = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 4 {
                    packages.push(Package {
                        id: parts[0].trim().to_string(),
                        name: parts[1].trim().to_string(),
                        manager: PackageManager::Flatpak,
                        version: parts[2].trim().to_string(),
                        source: Some(parts[3].trim().to_string()),
                        icon: Some(parts[0].trim().to_string()),
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

fn build_desktop_icon_map() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let apps_dir = std::path::Path::new("/usr/share/applications");
    if let Ok(entries) = std::fs::read_dir(apps_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "desktop") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut icon = None;
                    for line in content.lines() {
                        if line.starts_with("Icon=") {
                            icon = Some(line["Icon=".len()..].trim().to_string());
                            break;
                        }
                    }
                    if let Some(ic) = icon {
                        if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                            map.insert(file_stem.to_lowercase(), ic);
                        }
                    }
                }
            }
        }
    }
    map
}

impl Backend for DnfBackend {
    fn get_packages(&self) -> Result<Vec<Package>> {
        // We use repoquery to get a stable, tab-separated format for installed packages
        let output = Command::new("dnf")
            .args([
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{from_repo}\n",
            ])
            .output()?;

        let icon_map = build_desktop_icon_map();
        let mut packages = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    let source_raw = parts[2].trim_start_matches('@').trim();
                    let source_str = source_raw.to_string();
                    
                    let pkg_name = parts[0].to_string();
                    let pkg_lower = pkg_name.to_lowercase();
                    let mut icon_name = pkg_name.clone();
                    
                    if let Some(exact_icon) = icon_map.get(&pkg_lower) {
                        icon_name = exact_icon.clone();
                    } else {
                        for (stem, icon) in &icon_map {
                            let stem_parts: Vec<&str> = stem.split('.').collect();
                            if stem_parts.contains(&pkg_lower.as_str()) {
                                icon_name = icon.clone();
                                break;
                            }
                        }
                    }

                    packages.push(Package {
                        id: pkg_name.clone(),
                        name: pkg_name,
                        manager: PackageManager::Dnf,
                        version: parts[1].to_string(),
                        source: Some(source_str),
                        icon: Some(icon_name),
                    });
                }
            }
        }
        Ok(packages)
    }

    fn get_repositories(&self) -> Result<Vec<Repository>> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let mut repos = Vec::new();
        if let Ok(entries) = fs::read_dir("/etc/yum.repos.d/") {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "repo") {
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let mut current_id = String::new();
                    let mut current_url = None;
                    let mut enabled = false;

                    let meta = fs::metadata(&path).ok();
                    let added_date = meta.and_then(|m| {
                        #[cfg(unix)]
                        {
                            chrono::DateTime::from_timestamp(m.ctime(), 0)
                                .map(|dt| dt.naive_local().date())
                        }
                        #[cfg(not(unix))]
                        {
                            None
                        }
                    });

                    for line in content.lines() {
                        let line = line.trim();
                        if line.starts_with('[') && line.ends_with(']') {
                            if !current_id.is_empty() {
                                repos.push(Repository {
                                    id: current_id.clone(),
                                    name: current_id.clone(),
                                    manager: PackageManager::Dnf,
                                    enabled,
                                    url: current_url.clone(),
                                    file_path: Some(path.to_string_lossy().to_string()),
                                    added_date,
                                });
                            }
                            current_id = line[1..line.len() - 1].to_string();
                            current_url = None;
                            enabled = true;
                        } else if let Some((k, v)) = line.split_once('=') {
                            let k = k.trim();
                            let v = v.trim();
                            match k {
                                "baseurl" | "metalink" | "mirrorlist" => {
                                    current_url = Some(v.to_string())
                                }
                                "enabled" => enabled = v == "1",
                                _ => {}
                            }
                        }
                    }
                    if !current_id.is_empty() {
                        repos.push(Repository {
                            id: current_id.clone(),
                            name: current_id.clone(),
                            manager: PackageManager::Dnf,
                            enabled,
                            url: current_url,
                            file_path: Some(path.to_string_lossy().to_string()),
                            added_date,
                        });
                    }
                }
            }
        }
        Ok(repos)
    }
}
