use super::{Backend, BackendData, BackendError, CommandRunner};
use crate::models::{Package, PackageManager, Repository};
use std::sync::Arc;

pub struct FlatpakBackend {
    runner: Arc<dyn CommandRunner>,
}

impl FlatpakBackend {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }
}

pub fn parse_flatpak_packages(stdout: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 4 {
            let app_id = parts[0].trim().to_string();
            let name = parts[1].trim().to_string();
            let version = parts[2].trim().to_string();
            let source = parts[3].trim();
            let description = parts
                .get(4)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());
            let size = parts
                .get(5)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());

            let full_ref = parts
                .get(6)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());
            let arch = parts
                .get(7)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());
            let branch = parts
                .get(8)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());
            let scope = parts
                .get(9)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().to_string());

            let id = full_ref.clone().unwrap_or_else(|| app_id.clone());

            packages.push(Package {
                id,
                name,
                manager: PackageManager::Flatpak,
                version,
                source: if source.is_empty() {
                    None
                } else {
                    Some(source.to_string())
                },
                icon: Some(app_id),
                description,
                size,
                install_date: None,
                is_dependency: false,
                arch,
                branch,
                scope,
            });
        }
    }
    packages
}

pub fn parse_flatpak_remotes(stdout: &str) -> Vec<Repository> {
    let mut repos = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let id = parts[0].trim().to_string();
            let url = parts[1].trim();
            if !id.is_empty() {
                repos.push(Repository {
                    id: id.clone(),
                    name: id,
                    manager: PackageManager::Flatpak,
                    enabled: true,
                    url: if url.is_empty() {
                        None
                    } else {
                        Some(url.to_string())
                    },
                    file_path: None,
                    added_date: None,
                });
            }
        }
    }
    repos
}

impl Backend for FlatpakBackend {
    fn manager(&self) -> PackageManager {
        PackageManager::Flatpak
    }

    fn refresh(&self) -> Result<BackendData, BackendError> {
        let pkg_out = self.runner.run(
            "flatpak",
            &[
                "list",
                "--app",
                "--columns=application,name,version,origin,description,size,ref,arch,branch,installation",
            ],
        )?;
        let remote_out = self
            .runner
            .run("flatpak", &["remotes", "--columns=name,url"])?;

        if pkg_out.stdout.starts_with("error:") {
            return Err(BackendError::Parse {
                backend: PackageManager::Flatpak,
                detail: pkg_out.stdout.trim().to_string(),
            });
        }

        let packages = parse_flatpak_packages(&pkg_out.stdout);
        let repositories = parse_flatpak_remotes(&remote_out.stdout);

        Ok(BackendData {
            packages,
            repositories,
        })
    }

    fn uninstall(&self, package: &Package) -> Result<(), BackendError> {
        self.uninstall_with_logs(package, &|_| {})
    }

    fn uninstall_with_logs(
        &self,
        package: &Package,
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<(), BackendError> {
        let mut args = vec!["uninstall", "-y"];
        if let Some(scope) = &package.scope {
            match scope.to_lowercase().as_str() {
                "system" => args.push("--system"),
                "user" => args.push("--user"),
                _ => {}
            }
        }
        args.push(&package.id);

        self.runner.run_streaming("flatpak", &args, on_line)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::runner::{CommandOutput, MockCommandRunner};

    const SAMPLE_FLATPAK_PACKAGES: &str = "org.mozilla.firefox\tFirefox\t134.0\tflathub\tFast, Private & Safe Web Browser\t120 MB\torg.mozilla.firefox/x86_64/stable\tx86_64\tstable\tsystem\norg.gnome.Calculator\tCalculator\t47.0\tflathub\t\t\torg.gnome.Calculator/x86_64/stable\tx86_64\tstable\tuser\ninvalid-line\norg.kde.kwrite\tKWrite\t24.08.1\tfedora\n";

    const SAMPLE_FLATPAK_REMOTES: &str = "flathub\thttps://dl.flathub.org/repo/\nfedora\toci+https://registry.fedoraproject.org\ninvalid\n";

    #[test]
    fn test_parse_flatpak_packages() {
        let pkgs = parse_flatpak_packages(SAMPLE_FLATPAK_PACKAGES);
        assert_eq!(pkgs.len(), 3);

        assert_eq!(pkgs[0].id, "org.mozilla.firefox/x86_64/stable");
        assert_eq!(pkgs[0].name, "Firefox");
        assert_eq!(pkgs[0].version, "134.0");
        assert_eq!(pkgs[0].source.as_deref(), Some("flathub"));
        assert_eq!(pkgs[0].icon.as_deref(), Some("org.mozilla.firefox"));
        assert_eq!(
            pkgs[0].description.as_deref(),
            Some("Fast, Private & Safe Web Browser")
        );
        assert_eq!(pkgs[0].size.as_deref(), Some("120 MB"));
        assert_eq!(pkgs[0].manager, PackageManager::Flatpak);
        assert!(!pkgs[0].is_dependency);
        assert_eq!(pkgs[0].arch.as_deref(), Some("x86_64"));
        assert_eq!(pkgs[0].branch.as_deref(), Some("stable"));
        assert_eq!(pkgs[0].scope.as_deref(), Some("system"));

        // Package with empty description and size, user scope
        assert_eq!(pkgs[1].id, "org.gnome.Calculator/x86_64/stable");
        assert_eq!(pkgs[1].description, None);
        assert_eq!(pkgs[1].size, None);
        assert_eq!(pkgs[1].scope.as_deref(), Some("user"));

        // Legacy 4-column line
        assert_eq!(pkgs[2].id, "org.kde.kwrite");
        assert_eq!(pkgs[2].source.as_deref(), Some("fedora"));
        assert_eq!(pkgs[2].scope, None);
    }

    #[test]
    fn test_parse_flatpak_packages_empty() {
        let pkgs = parse_flatpak_packages("");
        assert!(pkgs.is_empty());
    }

    #[test]
    fn test_parse_flatpak_remotes() {
        let repos = parse_flatpak_remotes(SAMPLE_FLATPAK_REMOTES);
        assert_eq!(repos.len(), 2);

        assert_eq!(repos[0].id, "flathub");
        assert_eq!(repos[0].name, "flathub");
        assert_eq!(
            repos[0].url.as_deref(),
            Some("https://dl.flathub.org/repo/")
        );
        assert_eq!(repos[0].manager, PackageManager::Flatpak);
        assert!(repos[0].enabled);

        assert_eq!(repos[1].id, "fedora");
        assert_eq!(
            repos[1].url.as_deref(),
            Some("oci+https://registry.fedoraproject.org")
        );
    }

    #[test]
    fn test_parse_flatpak_remotes_empty() {
        let repos = parse_flatpak_remotes("");
        assert!(repos.is_empty());
    }

    #[test]
    fn test_flatpak_backend_refresh_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &[
                "list",
                "--app",
                "--columns=application,name,version,origin,description,size,ref,arch,branch,installation",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: SAMPLE_FLATPAK_PACKAGES.to_string(),
                stderr: String::new(),
            }),
        );
        mock_runner.set_response(
            "flatpak",
            &["remotes", "--columns=name,url"],
            Ok(CommandOutput {
                status: 0,
                stdout: SAMPLE_FLATPAK_REMOTES.to_string(),
                stderr: String::new(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner);
        assert_eq!(backend.manager(), PackageManager::Flatpak);
        let data = backend.refresh().unwrap();
        assert_eq!(data.packages.len(), 3);
        assert_eq!(data.repositories.len(), 2);
    }

    #[test]
    fn test_flatpak_backend_refresh_unavailable() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &[
                "list",
                "--app",
                "--columns=application,name,version,origin,description,size,ref,arch,branch,installation",
            ],
            Err(BackendError::Unavailable {
                program: "flatpak".to_string(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        assert_eq!(
            err,
            BackendError::Unavailable {
                program: "flatpak".to_string()
            }
        );
    }

    #[test]
    fn test_flatpak_backend_uninstall_system() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &[
                "uninstall",
                "-y",
                "--system",
                "org.mozilla.firefox/x86_64/stable",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: "Uninstalled".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner.clone());
        let pkg = Package {
            id: "org.mozilla.firefox/x86_64/stable".to_string(),
            name: "Firefox".to_string(),
            manager: PackageManager::Flatpak,
            version: "134.0".to_string(),
            source: Some("flathub".to_string()),
            icon: None,
            description: None,
            size: None,
            install_date: None,
            is_dependency: false,
            arch: Some("x86_64".to_string()),
            branch: Some("stable".to_string()),
            scope: Some("system".to_string()),
        };

        assert!(backend.uninstall(&pkg).is_ok());
        let calls = mock_runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "flatpak");
        assert_eq!(
            calls[0].1,
            vec![
                "uninstall",
                "-y",
                "--system",
                "org.mozilla.firefox/x86_64/stable"
            ]
        );
    }

    #[test]
    fn test_flatpak_backend_uninstall_user() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &[
                "uninstall",
                "-y",
                "--user",
                "org.gnome.Calculator/x86_64/stable",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: "Uninstalled".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner.clone());
        let pkg = Package {
            id: "org.gnome.Calculator/x86_64/stable".to_string(),
            name: "Calculator".to_string(),
            manager: PackageManager::Flatpak,
            version: "47.0".to_string(),
            source: Some("flathub".to_string()),
            icon: None,
            description: None,
            size: None,
            install_date: None,
            is_dependency: false,
            arch: Some("x86_64".to_string()),
            branch: Some("stable".to_string()),
            scope: Some("user".to_string()),
        };

        assert!(backend.uninstall(&pkg).is_ok());
        let calls = mock_runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "flatpak");
        assert_eq!(
            calls[0].1,
            vec![
                "uninstall",
                "-y",
                "--user",
                "org.gnome.Calculator/x86_64/stable"
            ]
        );
    }

    #[test]
    fn test_flatpak_backend_uninstall_failure() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &["uninstall", "-y", "org.kde.kwrite"],
            Err(BackendError::CommandFailed {
                program: "flatpak".to_string(),
                status: 1,
                stderr: "app not installed".to_string(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner);
        let pkg = Package {
            id: "org.kde.kwrite".to_string(),
            name: "KWrite".to_string(),
            manager: PackageManager::Flatpak,
            version: "24.08.1".to_string(),
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
                assert_eq!(stderr, "app not installed");
            }
            _ => panic!("Expected CommandFailed"),
        }
    }

    #[test]
    fn test_flatpak_backend_refresh_malformed() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &[
                "list",
                "--app",
                "--columns=application,name,version,origin,description,size,ref,arch,branch,installation",
            ],
            Ok(CommandOutput {
                status: 0,
                stdout: "error: unexpected corrupted listing".to_string(),
                stderr: String::new(),
            }),
        );
        mock_runner.set_response(
            "flatpak",
            &["remotes", "--columns=name,url"],
            Ok(CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        match err {
            BackendError::Parse { backend, detail } => {
                assert_eq!(backend, PackageManager::Flatpak);
                assert!(detail.contains("unexpected corrupted listing"));
            }
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn test_flatpak_backend_uninstall_with_logs() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "flatpak",
            &["uninstall", "-y", "--user", "org.gnome.Calculator"],
            Ok(CommandOutput {
                status: 0,
                stdout: "Uninstalling org.gnome.Calculator\nDone.\n".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = FlatpakBackend::new(mock_runner);
        let pkg = Package {
            id: "org.gnome.Calculator".to_string(),
            name: "Calculator".to_string(),
            manager: PackageManager::Flatpak,
            version: "47.0".to_string(),
            source: None,
            icon: None,
            description: None,
            size: None,
            install_date: None,
            is_dependency: false,
            arch: None,
            branch: None,
            scope: Some("user".to_string()),
        };

        let logs = std::sync::Mutex::new(Vec::new());
        assert!(backend
            .uninstall_with_logs(&pkg, &|line| {
                logs.lock().unwrap().push(line.to_string());
            })
            .is_ok());

        assert_eq!(
            *logs.lock().unwrap(),
            vec![
                "Uninstalling org.gnome.Calculator".to_string(),
                "Done.".to_string()
            ]
        );
    }
}
