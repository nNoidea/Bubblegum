use super::{Backend, BackendData, BackendError, CommandRunner};
use crate::models::{Package, PackageManager, Repository};
use std::fs;
use std::sync::Arc;

pub struct DnfBackend {
    runner: Arc<dyn CommandRunner>,
}

impl DnfBackend {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    pub fn get_repositories(&self) -> Result<Vec<Repository>, BackendError> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        let mut repos = Vec::new();
        if let Ok(entries) = fs::read_dir("/etc/yum.repos.d/") {
            for entry in entries.filter_map(std::io::Result::ok) {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "repo") {
                    let content = fs::read_to_string(&path).unwrap_or_default();
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

                    let file_path = path.to_str();
                    let mut file_repos = parse_repo_file_content(&content, file_path, added_date);
                    repos.append(&mut file_repos);
                }
            }
        }
        Ok(repos)
    }
}

pub fn parse_desktop_file_content(content: &str) -> Option<String> {
    let mut icon = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "NoDisplay=true" || trimmed == "Hidden=true" {
            return None;
        }
        if let Some(rest) = trimmed.strip_prefix("Icon=") {
            icon = Some(rest.trim().to_string());
        }
    }
    icon
}

fn build_desktop_icon_map() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let apps_dir = std::path::Path::new("/usr/share/applications");
    if let Ok(entries) = std::fs::read_dir(apps_dir) {
        for entry in entries.filter_map(std::io::Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "desktop")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Some(icon) = parse_desktop_file_content(&content)
                && let Some(file_stem) = path.file_stem().and_then(|s| s.to_str())
            {
                map.insert(file_stem.to_lowercase(), icon);
            }
        }
    }
    map
}

pub fn parse_dnf_repoquery_output(
    stdout: &str,
    icon_map: &std::collections::HashMap<String, String>,
) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 9 {
            // New format: name, version, release, arch, from_repo, summary, installsize, installtime, reason
            let pkg_name = parts[0].to_string();
            let version = parts[1].to_string();
            let release = parts[2].to_string();
            let arch_str = parts[3].to_string();
            let source_raw = parts[4].trim_start_matches('@').trim();
            let summary_raw = parts[5].trim();
            let size_bytes = parts[6].parse::<u64>().ok();
            let install_time = parts[7].parse::<i64>().ok();
            let reason = parts[8];

            let nevra = if !release.is_empty() && !arch_str.is_empty() {
                format!("{}-{}-{}.{}", pkg_name, version, release, arch_str)
            } else {
                pkg_name.clone()
            };

            let pkg_lower = pkg_name.to_lowercase();
            let mut icon_name = pkg_name.clone();
            let mut is_gui = false;

            if let Some(exact_icon) = icon_map.get(&pkg_lower) {
                if !exact_icon.is_empty() {
                    icon_name = exact_icon.clone();
                }
                is_gui = true;
            } else {
                for (stem, icon) in icon_map {
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

            let summary = if summary_raw.is_empty() {
                None
            } else {
                Some(summary_raw.to_string())
            };

            let size_str = size_bytes.map(|b| {
                let mb = b as f64 / 1_048_576.0;
                format!("{:.1} MB", mb)
            });

            let date_str = install_time.and_then(|t| {
                chrono::DateTime::from_timestamp(t, 0).map(|dt| dt.format("%b %e, %Y").to_string())
            });

            let mut is_dependency = reason.to_lowercase().contains("depend");
            if is_gui {
                is_dependency = false;
            }

            packages.push(Package {
                id: nevra,
                name: pkg_name,
                manager: PackageManager::Dnf,
                version,
                source: if source_raw.is_empty() {
                    None
                } else {
                    Some(source_raw.to_string())
                },
                icon: Some(icon_name),
                description: summary,
                size: size_str,
                install_date: date_str,
                is_dependency,
                arch: if arch_str.is_empty() {
                    None
                } else {
                    Some(arch_str)
                },
                branch: None,
                scope: None,
            });
        } else if parts.len() >= 7 {
            // Legacy format fallback: name, version, source, summary, size, time, reason
            let pkg_name = parts[0].to_string();
            let version = parts[1].to_string();
            let source_raw = parts[2].trim_start_matches('@').trim();
            let summary_raw = parts[3].trim();
            let size_bytes = parts[4].parse::<u64>().ok();
            let install_time = parts[5].parse::<i64>().ok();
            let reason = parts[6];

            let pkg_lower = pkg_name.to_lowercase();
            let mut icon_name = pkg_name.clone();
            let mut is_gui = false;

            if let Some(exact_icon) = icon_map.get(&pkg_lower) {
                if !exact_icon.is_empty() {
                    icon_name = exact_icon.clone();
                }
                is_gui = true;
            } else {
                for (stem, icon) in icon_map {
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

            let summary = if summary_raw.is_empty() {
                None
            } else {
                Some(summary_raw.to_string())
            };

            let size_str = size_bytes.map(|b| {
                let mb = b as f64 / 1_048_576.0;
                format!("{:.1} MB", mb)
            });

            let date_str = install_time.and_then(|t| {
                chrono::DateTime::from_timestamp(t, 0).map(|dt| dt.format("%b %e, %Y").to_string())
            });

            let mut is_dependency = reason.to_lowercase().contains("depend");
            if is_gui {
                is_dependency = false;
            }

            packages.push(Package {
                id: pkg_name.clone(),
                name: pkg_name,
                manager: PackageManager::Dnf,
                version,
                source: if source_raw.is_empty() {
                    None
                } else {
                    Some(source_raw.to_string())
                },
                icon: Some(icon_name),
                description: summary,
                size: size_str,
                install_date: date_str,
                is_dependency,
                arch: None,
                branch: None,
                scope: None,
            });
        }
    }
    packages
}

pub fn parse_repo_file_content(
    content: &str,
    file_path: Option<&str>,
    added_date: Option<chrono::NaiveDate>,
) -> Vec<Repository> {
    let mut repos = Vec::new();
    let mut current_id = String::new();
    let mut current_url = None;
    let mut enabled = false;

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
                    file_path: file_path.map(|s| s.to_string()),
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
                "baseurl" | "metalink" | "mirrorlist" => current_url = Some(v.to_string()),
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
            file_path: file_path.map(|s| s.to_string()),
            added_date,
        });
    }
    repos
}

pub fn parse_dnf_remove_simulation(stdout: &str, target_pkg: &str) -> Vec<String> {
    let mut in_removing_section = false;
    let mut packages = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("Removing:")
            || trimmed.starts_with("Removing dependent packages:")
            || trimmed.starts_with("Removing unused dependencies:")
        {
            in_removing_section = true;
            continue;
        }

        if trimmed.starts_with("Transaction Summary:")
            || trimmed.starts_with("Installing:")
            || trimmed.starts_with("Upgrading:")
            || trimmed.starts_with("Downgrading:")
            || trimmed.starts_with("Reinstalling:")
            || trimmed.starts_with("Operation aborted")
            || trimmed.starts_with("After this operation")
        {
            in_removing_section = false;
            continue;
        }

        if in_removing_section {
            if trimmed.starts_with("Package") {
                continue;
            }
            if let Some(pkg_name) = trimmed.split_whitespace().next() {
                let is_target =
                    pkg_name == target_pkg || target_pkg.starts_with(&format!("{}-", pkg_name));
                if !is_target {
                    packages.push(pkg_name.to_string());
                }
            }
        }
    }

    packages.sort();
    packages.dedup();
    packages
}

impl Backend for DnfBackend {
    fn manager(&self) -> PackageManager {
        PackageManager::Dnf
    }

    fn refresh(&self) -> Result<BackendData, BackendError> {
        let output = self.runner.run(
            "dnf",
            &[
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{release}\t%{arch}\t%{from_repo}\t%{summary}\t%{installsize}\t%{installtime}\t%{reason}\n",
            ],
        )?;

        if output.stdout.starts_with("error:") || output.stdout.starts_with("Error:") {
            return Err(BackendError::Parse {
                backend: PackageManager::Dnf,
                detail: output.stdout.trim().to_string(),
            });
        }

        let icon_map = build_desktop_icon_map();
        let packages = parse_dnf_repoquery_output(&output.stdout, &icon_map);
        let repositories = self.get_repositories()?;

        Ok(BackendData {
            packages,
            repositories,
        })
    }

    fn check_uninstall_impact(&self, package: &Package) -> Result<Vec<String>, BackendError> {
        let pkg_identifier = if !package.id.is_empty() {
            &package.id
        } else {
            &package.name
        };

        let output = self
            .runner
            .run_command("dnf", &["remove", "--assumeno", pkg_identifier])?;

        Ok(parse_dnf_remove_simulation(
            &output.stdout,
            &package.name,
        ))
    }

    fn uninstall(&self, package: &Package) -> Result<(), BackendError> {
        self.runner
            .run("pkexec", &["dnf", "remove", "-y", &package.id])?;
        Ok(())
    }

    fn uninstall_with_logs(
        &self,
        package: &Package,
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<(), BackendError> {
        self.runner
            .run_streaming("pkexec", &["dnf", "remove", "-y", &package.id], on_line)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::runner::{CommandOutput, MockCommandRunner};
    use std::collections::HashMap;

    #[test]
    fn test_parse_desktop_file_content() {
        let desktop_valid =
            "[Desktop Entry]\nName=Firefox\nIcon=org.mozilla.firefox\nType=Application\n";
        assert_eq!(
            parse_desktop_file_content(desktop_valid),
            Some("org.mozilla.firefox".to_string())
        );

        let desktop_hidden = "[Desktop Entry]\nName=Internal\nHidden=true\nIcon=something\n";
        assert_eq!(parse_desktop_file_content(desktop_hidden), None);

        let desktop_no_display = "[Desktop Entry]\nName=Helper\nNoDisplay=true\nIcon=something\n";
        assert_eq!(parse_desktop_file_content(desktop_no_display), None);

        let desktop_no_icon = "[Desktop Entry]\nName=NoIcon\nType=Application\n";
        assert_eq!(parse_desktop_file_content(desktop_no_icon), None);
    }

    #[test]
    fn test_parse_dnf_repoquery_output_9_columns() {
        let mut icon_map = HashMap::new();
        icon_map.insert("firefox".to_string(), "org.mozilla.firefox".to_string());

        let sample = "firefox\t134.0.2\t1.fc41\tx86_64\t@fedora\tWeb Browser\t104857600\t1704067200\tUser\n\
                      glibc\t2.40\t2.fc41\tx86_64\t@fedora\tCore C library\t15728640\t1704067200\tdependency\n";

        let pkgs = parse_dnf_repoquery_output(sample, &icon_map);
        assert_eq!(pkgs.len(), 2);

        // Firefox: NEVRA ID
        assert_eq!(pkgs[0].id, "firefox-134.0.2-1.fc41.x86_64");
        assert_eq!(pkgs[0].name, "firefox");
        assert_eq!(pkgs[0].version, "134.0.2");
        assert_eq!(pkgs[0].arch.as_deref(), Some("x86_64"));
        assert_eq!(pkgs[0].source.as_deref(), Some("fedora"));
        assert_eq!(pkgs[0].icon.as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(pkgs[0].size.as_deref(), Some("100.0 MB"));
        assert!(!pkgs[0].is_dependency);

        // glibc: dependency
        assert_eq!(pkgs[1].id, "glibc-2.40-2.fc41.x86_64");
        assert_eq!(pkgs[1].name, "glibc");
        assert!(pkgs[1].is_dependency);
    }

    #[test]
    fn test_parse_dnf_repoquery_output_legacy_7_columns() {
        let mut icon_map = HashMap::new();
        icon_map.insert("firefox".to_string(), "org.mozilla.firefox".to_string());
        icon_map.insert(
            "org.gnome.calculator".to_string(),
            "gnome-calculator".to_string(),
        );

        let sample = "firefox\t134.0.2\t@fedora\tWeb Browser\t104857600\t1704067200\tUser\n\
                      calculator\t47.0\t@updates\tCalculator app\t2097152\t1704067200\tDependency\n\
                      glibc\t2.40\t@fedora\tCore C library\t15728640\t1704067200\tdependency\n\
                      short-line\n";

        let pkgs = parse_dnf_repoquery_output(sample, &icon_map);
        assert_eq!(pkgs.len(), 3);

        assert_eq!(pkgs[0].name, "firefox");
        assert_eq!(pkgs[0].version, "134.0.2");
        assert_eq!(pkgs[0].source.as_deref(), Some("fedora"));
        assert_eq!(pkgs[0].icon.as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(pkgs[0].size.as_deref(), Some("100.0 MB"));
        assert_eq!(pkgs[0].description.as_deref(), Some("Web Browser"));
        assert!(!pkgs[0].is_dependency);

        assert_eq!(pkgs[1].name, "calculator");
        assert_eq!(pkgs[1].icon.as_deref(), Some("gnome-calculator"));
        assert_eq!(pkgs[1].size.as_deref(), Some("2.0 MB"));
        assert!(!pkgs[1].is_dependency);

        assert_eq!(pkgs[2].name, "glibc");
        assert_eq!(pkgs[2].icon.as_deref(), Some("glibc"));
        assert!(pkgs[2].is_dependency);
    }

    #[test]
    fn test_parse_dnf_repoquery_empty() {
        let pkgs = parse_dnf_repoquery_output("", &HashMap::new());
        assert!(pkgs.is_empty());
    }

    #[test]
    fn test_parse_repo_file_content() {
        let repo_content = r#"
[fedora]
name=Fedora $releasever - $basearch
metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch
enabled=1
gpgcheck=1

[fedora-debuginfo]
name=Fedora $releasever - $basearch - Debug
baseurl=http://download.example/pub/fedora/linux/releases/$releasever/Everything/$basearch/debug/tree/
enabled=0
"#;

        let repos =
            parse_repo_file_content(repo_content, Some("/etc/yum.repos.d/fedora.repo"), None);
        assert_eq!(repos.len(), 2);

        assert_eq!(repos[0].id, "fedora");
        assert_eq!(repos[0].name, "fedora");
        assert!(repos[0].enabled);
        assert_eq!(
            repos[0].url.as_deref(),
            Some(
                "https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch"
            )
        );
        assert_eq!(
            repos[0].file_path.as_deref(),
            Some("/etc/yum.repos.d/fedora.repo")
        );

        assert_eq!(repos[1].id, "fedora-debuginfo");
        assert_eq!(repos[1].name, "fedora-debuginfo");
        assert!(!repos[1].enabled);
        assert_eq!(
            repos[1].url.as_deref(),
            Some(
                "http://download.example/pub/fedora/linux/releases/$releasever/Everything/$basearch/debug/tree/"
            )
        );
    }

    #[test]
    fn test_dnf_backend_refresh_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "dnf",
            &[
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{release}\t%{arch}\t%{from_repo}\t%{summary}\t%{installsize}\t%{installtime}\t%{reason}\n",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: "firefox\t134.0.2\t1.fc41\tx86_64\t@fedora\tWeb Browser\t104857600\t1704067200\tUser\n".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        assert_eq!(backend.manager(), PackageManager::Dnf);
        let data = backend.refresh().unwrap();
        assert_eq!(data.packages.len(), 1);
        assert_eq!(data.packages[0].id, "firefox-134.0.2-1.fc41.x86_64");
    }

    #[test]
    fn test_dnf_backend_refresh_unavailable() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "dnf",
            &[
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{release}\t%{arch}\t%{from_repo}\t%{summary}\t%{installsize}\t%{installtime}\t%{reason}\n",
            ],
            Err(BackendError::Unavailable {
                program: "dnf".to_string(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        assert_eq!(
            err,
            BackendError::Unavailable {
                program: "dnf".to_string()
            }
        );
    }

    #[test]
    fn test_dnf_backend_uninstall_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "pkexec",
            &["dnf", "remove", "-y", "firefox-134.0.2-1.fc41.x86_64"],
            Ok(CommandOutput {
                status: 0,
                stdout: "Removed".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = DnfBackend::new(mock_runner.clone());
        let pkg = Package {
            id: "firefox-134.0.2-1.fc41.x86_64".to_string(),
            name: "firefox".to_string(),
            manager: PackageManager::Dnf,
            version: "134.0.2".to_string(),
            source: Some("fedora".to_string()),
            icon: None,
            description: None,
            size: None,
            install_date: None,
            is_dependency: false,
            arch: Some("x86_64".to_string()),
            branch: None,
            scope: None,
        };

        assert!(backend.uninstall(&pkg).is_ok());
        let calls = mock_runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "pkexec");
        assert_eq!(
            calls[0].1,
            vec!["dnf", "remove", "-y", "firefox-134.0.2-1.fc41.x86_64"]
        );
    }

    #[test]
    fn test_dnf_backend_uninstall_failure() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "pkexec",
            &["dnf", "remove", "-y", "some-pkg"],
            Err(BackendError::CommandFailed {
                program: "pkexec".to_string(),
                status: 1,
                stderr: "authorization failed".to_string(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let pkg = Package {
            id: "some-pkg".to_string(),
            name: "some-pkg".to_string(),
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

        let err = backend.uninstall(&pkg).unwrap_err();
        match err {
            BackendError::CommandFailed { status, stderr, .. } => {
                assert_eq!(status, 1);
                assert_eq!(stderr, "authorization failed");
            }
            _ => panic!("Expected CommandFailed"),
        }
    }

    #[test]
    fn test_dnf_backend_refresh_malformed() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "dnf",
            &[
                "repoquery",
                "--installed",
                "--qf",
                "%{name}\t%{version}\t%{release}\t%{arch}\t%{from_repo}\t%{summary}\t%{installsize}\t%{installtime}\t%{reason}\n",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: "Error: Failed to synchronize cache for repo 'updates'\n".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        match err {
            BackendError::Parse { backend, detail } => {
                assert_eq!(backend, PackageManager::Dnf);
                assert!(detail.contains("Failed to synchronize cache"));
            }
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn test_parse_dnf_remove_simulation_with_dependencies_and_unused() {
        let sample = r#"
Package                      Arch   Version                             Repository      Size
Removing:
 f3d                         x86_64 0:3.5.0-1.fc44                      updates      6.1 MiB
Removing dependent packages:
 app-dependent               x86_64 1.0.0-1.fc44                         updates      2.0 MiB
Removing unused dependencies:
 vtk                         x86_64 0:9.5.2-12.fc44                     updates    211.5 MiB
 libarrow                    x86_64 0:23.0.1-1.fc44                     fedora      13.9 MiB
 opencascade-modeling        x86_64 0:7.9.3-2.fc44                      fedora      46.8 MiB

Transaction Summary:
 Removing:          4 packages

After this operation, 278 MiB will be freed (install 0 B, remove 278 MiB).
Operation aborted by the user.
"#;
        let pkgs = parse_dnf_remove_simulation(sample, "f3d");
        assert_eq!(
            pkgs,
            vec![
                "app-dependent".to_string(),
                "libarrow".to_string(),
                "opencascade-modeling".to_string(),
                "vtk".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_dnf_remove_simulation_empty() {
        let sample = r#"
Package                      Arch   Version                             Repository      Size
Removing:
 alacritty                   x86_64 0.17.0-1.fc44                       updates      5.0 MiB

Transaction Summary:
 Removing:          1 package
"#;
        let pkgs = parse_dnf_remove_simulation(sample, "alacritty");
        assert!(pkgs.is_empty());
    }

    #[test]
    fn test_dnf_backend_check_uninstall_impact_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "dnf",
            &["remove", "--assumeno", "f3d-3.5.0.x86_64"],
            Ok(CommandOutput {
                status: 1,
                stdout: r#"
Removing:
 f3d                         x86_64 0:3.5.0-1.fc44                      updates      6.1 MiB
Removing unused dependencies:
 vtk                         x86_64 0:9.5.2-12.fc44                     updates    211.5 MiB
 alembic-libs                x86_64 0:1.8.12-1.fc44                     updates      1.7 MiB

Transaction Summary:
 Removing:          3 packages
"#
                .to_string(),
                stderr: "Operation aborted by the user.\n".to_string(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let pkg = Package {
            id: "f3d-3.5.0.x86_64".to_string(),
            name: "f3d".to_string(),
            manager: PackageManager::Dnf,
            version: "3.5.0".to_string(),
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

        let deps = backend.check_uninstall_impact(&pkg).unwrap();
        assert_eq!(deps, vec!["alembic-libs".to_string(), "vtk".to_string()]);
    }

    #[test]
    fn test_dnf_backend_check_uninstall_impact_failure() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "dnf",
            &["remove", "--assumeno", "badpkg"],
            Err(BackendError::Unavailable {
                program: "dnf".to_string(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let pkg = Package {
            id: "badpkg".to_string(),
            name: "badpkg".to_string(),
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

        assert!(backend.check_uninstall_impact(&pkg).is_err());
    }

    #[test]
    fn test_dnf_backend_uninstall_with_logs() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "pkexec",
            &["dnf", "remove", "-y", "pkg1"],
            Ok(CommandOutput {
                status: 0,
                stdout: "Removing pkg1\nComplete!\n".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = DnfBackend::new(mock_runner);
        let pkg = Package {
            id: "pkg1".to_string(),
            name: "pkg1".to_string(),
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

        let logs = std::sync::Mutex::new(Vec::new());
        assert!(backend
            .uninstall_with_logs(&pkg, &|line| {
                logs.lock().unwrap().push(line.to_string());
            })
            .is_ok());

        assert_eq!(
            *logs.lock().unwrap(),
            vec!["Removing pkg1".to_string(), "Complete!".to_string()]
        );
    }
}

