use super::Backend;
use crate::models::{Package, PackageManager, Repository};
use anyhow::Result;
use std::fs;
use std::process::Command;

pub struct DnfBackend;

fn build_desktop_icon_map() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let apps_dir = std::path::Path::new("/usr/share/applications");
    if let Ok(entries) = std::fs::read_dir(apps_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "desktop") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut icon = None;
                    let mut skip = false;
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed == "NoDisplay=true" || trimmed == "Hidden=true" {
                            skip = true;
                            break;
                        }
                        if trimmed.starts_with("Icon=") {
                            icon = Some(trimmed["Icon=".len()..].trim().to_string());
                        }
                    }
                    if !skip {
                        if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let ic = icon.unwrap_or_else(|| "".to_string());
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
        let output = Command::new("dnf")
            .args([
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{from_repo}\t%{summary}\t%{installsize}\t%{installtime}\t%{reason}\n",
            ])
            .output()?;

        let icon_map = build_desktop_icon_map();
        let mut packages = Vec::new();
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    let source_raw = parts.get(2).unwrap_or(&"").trim_start_matches('@').trim();
                    let source_str = source_raw.to_string();
                    
                    let pkg_name = parts[0].to_string();
                    let pkg_lower = pkg_name.to_lowercase();
                    let mut icon_name = pkg_name.clone();
                    let mut is_gui = false;
                    
                    if let Some(exact_icon) = icon_map.get(&pkg_lower) {
                        if !exact_icon.is_empty() {
                            icon_name = exact_icon.clone();
                        }
                        is_gui = true;
                    } else {
                        for (stem, icon) in &icon_map {
                            let stem_parts: Vec<&str> = stem.split('.').collect();
                            if stem_parts.contains(&pkg_lower.as_str()) {
                                if !icon.is_empty() {
                                    icon_name = icon.clone();
                                }
                                is_gui = true;
                                break;
                            }
                        }
                    }
                    
                    let summary = parts.get(3).filter(|s| !s.is_empty()).map(|s| s.trim().to_string());
                    
                    let size_bytes = parts.get(4).and_then(|s| s.parse::<u64>().ok());
                    let size_str = size_bytes.map(|b| {
                        let mb = b as f64 / 1_048_576.0;
                        format!("{:.1} MB", mb)
                    });
                    
                    let install_time = parts.get(5).and_then(|s| s.parse::<i64>().ok());
                    let date_str = install_time.and_then(|t| {
                        chrono::DateTime::from_timestamp(t, 0)
                            .map(|dt| dt.format("%b %e, %Y").to_string())
                    });

                    let reason = parts.get(6).unwrap_or(&"");
                    let mut is_dependency = reason.to_lowercase().contains("depend");
                    if is_gui {
                        is_dependency = false;
                    }

                    packages.push(Package {
                        id: pkg_name.clone(),
                        name: pkg_name,
                        manager: PackageManager::Dnf,
                        version: parts.get(1).unwrap_or(&"").to_string(),
                        source: Some(source_str),
                        icon: Some(icon_name),
                        description: summary,
                        size: size_str,
                        install_date: date_str,
                        is_dependency,
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
