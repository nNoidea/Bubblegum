use super::{Backend, BackendData, BackendError, CommandRunner};
use crate::models::{Package, PackageManager, Repository};
use std::sync::Arc;

pub struct CargoBackend {
    runner: Arc<dyn CommandRunner>,
}

impl CargoBackend {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }
}

pub fn parse_cargo_packages(stdout: &str) -> Vec<Package> {
    let mut packages = Vec::new();
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
                    is_dependency: false,
                    arch: None,
                    branch: None,
                    scope: None,
                });
            }
        }
    }
    packages
}

pub fn parse_cargo_repositories(stdout: &str) -> Vec<Repository> {
    let mut repos = std::collections::HashSet::new();
    // Always include crates.io by default
    repos.insert("crates.io".to_string());

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
    repositories
}

impl Backend for CargoBackend {
    fn manager(&self) -> PackageManager {
        PackageManager::Cargo
    }

    fn refresh(&self) -> Result<BackendData, BackendError> {
        let output = self.runner.run("cargo", &["install", "--list"])?;
        if output.stdout.starts_with("error:") {
            return Err(BackendError::Parse {
                backend: PackageManager::Cargo,
                detail: output.stdout.trim().to_string(),
            });
        }
        let packages = parse_cargo_packages(&output.stdout);
        let repositories = parse_cargo_repositories(&output.stdout);

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
        self.runner
            .run_streaming("cargo", &["uninstall", &package.name], on_line)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::runner::{CommandOutput, MockCommandRunner};

    const SAMPLE_OUTPUT: &str = r#"cargo-tarpaulin v0.37.2:
    cargo-tarpaulin
ripgrep v14.1.0 (https://github.com/BurntSushi/ripgrep#12345):
    rg
tokio-console v0.1.12 (/path/to/local/crate):
    tokio-console
invalid-line
  just-a-binary
"#;

    #[test]
    fn test_parse_cargo_packages() {
        let packages = parse_cargo_packages(SAMPLE_OUTPUT);
        assert_eq!(packages.len(), 3);

        assert_eq!(packages[0].name, "cargo-tarpaulin");
        assert_eq!(packages[0].version, "v0.37.2");
        assert_eq!(packages[0].source.as_deref(), Some("crates.io"));
        assert_eq!(packages[0].manager, PackageManager::Cargo);
        assert!(!packages[0].is_dependency);

        assert_eq!(packages[1].name, "ripgrep");
        assert_eq!(packages[1].version, "v14.1.0");
        assert_eq!(
            packages[1].source.as_deref(),
            Some("https://github.com/BurntSushi/ripgrep#12345")
        );

        assert_eq!(packages[2].name, "tokio-console");
        assert_eq!(packages[2].version, "v0.1.12");
        assert_eq!(packages[2].source.as_deref(), Some("/path/to/local/crate"));
    }

    #[test]
    fn test_parse_cargo_packages_empty() {
        let packages = parse_cargo_packages("");
        assert!(packages.is_empty());
    }

    #[test]
    fn test_parse_cargo_repositories() {
        let repos = parse_cargo_repositories(SAMPLE_OUTPUT);
        assert_eq!(repos.len(), 3);

        let names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
        assert!(names.contains(&"crates.io".to_string()));
        assert!(names.contains(&"https://github.com/BurntSushi/ripgrep#12345".to_string()));
        assert!(names.contains(&"/path/to/local/crate".to_string()));

        let crates_io = repos.iter().find(|r| r.name == "crates.io").unwrap();
        assert_eq!(crates_io.url.as_deref(), Some("https://crates.io"));
        assert!(crates_io.enabled);
    }

    #[test]
    fn test_parse_cargo_repositories_default() {
        let repos = parse_cargo_repositories("");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "crates.io");
        assert_eq!(repos[0].url.as_deref(), Some("https://crates.io"));
    }

    #[test]
    fn test_cargo_backend_refresh_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["install", "--list"],
            Ok(CommandOutput {
                status: 0,
                stdout: SAMPLE_OUTPUT.to_string(),
                stderr: String::new(),
            }),
        );

        let backend = CargoBackend::new(mock_runner.clone());
        assert_eq!(backend.manager(), PackageManager::Cargo);

        let data = backend.refresh().unwrap();
        assert_eq!(data.packages.len(), 3);
        assert_eq!(data.repositories.len(), 3);

        let calls = mock_runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            (
                "cargo".to_string(),
                vec!["install".to_string(), "--list".to_string()]
            )
        );
    }

    #[test]
    fn test_cargo_backend_refresh_unavailable() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["install", "--list"],
            Err(BackendError::Unavailable {
                program: "cargo".to_string(),
            }),
        );

        let backend = CargoBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        assert_eq!(
            err,
            BackendError::Unavailable {
                program: "cargo".to_string()
            }
        );
    }

    #[test]
    fn test_cargo_backend_refresh_command_failed() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["install", "--list"],
            Err(BackendError::CommandFailed {
                program: "cargo".to_string(),
                status: 1,
                stderr: "cargo not configured".to_string(),
            }),
        );

        let backend = CargoBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        match err {
            BackendError::CommandFailed { status, stderr, .. } => {
                assert_eq!(status, 1);
                assert_eq!(stderr, "cargo not configured");
            }
            _ => panic!("Expected CommandFailed error"),
        }
    }

    #[test]
    fn test_cargo_backend_uninstall_success() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["uninstall", "ripgrep"],
            Ok(CommandOutput {
                status: 0,
                stdout: "uninstalled ripgrep".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = CargoBackend::new(mock_runner.clone());
        let pkg = Package {
            id: "ripgrep".to_string(),
            name: "ripgrep".to_string(),
            manager: PackageManager::Cargo,
            version: "14.1.0".to_string(),
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

        assert!(backend.uninstall(&pkg).is_ok());
        let calls = mock_runner.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "cargo");
        assert_eq!(calls[0].1, vec!["uninstall", "ripgrep"]);
    }

    #[test]
    fn test_cargo_backend_uninstall_failure() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["uninstall", "nonexistent"],
            Err(BackendError::CommandFailed {
                program: "cargo".to_string(),
                status: 101,
                stderr: "package not found".to_string(),
            }),
        );

        let backend = CargoBackend::new(mock_runner);
        let pkg = Package {
            id: "nonexistent".to_string(),
            name: "nonexistent".to_string(),
            manager: PackageManager::Cargo,
            version: "1.0.0".to_string(),
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
                assert_eq!(status, 101);
                assert_eq!(stderr, "package not found");
            }
            _ => panic!("Expected CommandFailed error"),
        }
    }

    #[test]
    fn test_cargo_backend_refresh_malformed() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["install", "--list"],
            Ok(CommandOutput {
                status: 0,
                stdout: "error: failed to read installed packages".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = CargoBackend::new(mock_runner);
        let err = backend.refresh().unwrap_err();
        match err {
            BackendError::Parse { backend, detail } => {
                assert_eq!(backend, PackageManager::Cargo);
                assert!(detail.contains("failed to read installed packages"));
            }
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn test_cargo_backend_uninstall_with_logs() {
        let mock_runner = Arc::new(MockCommandRunner::new());
        mock_runner.set_response(
            "cargo",
            &["uninstall", "ripgrep"],
            Ok(CommandOutput {
                status: 0,
                stdout: "Removing /home/user/.cargo/bin/rg\n".to_string(),
                stderr: String::new(),
            }),
        );

        let backend = CargoBackend::new(mock_runner);
        let pkg = Package {
            id: "ripgrep".to_string(),
            name: "ripgrep".to_string(),
            manager: PackageManager::Cargo,
            version: "14.1.0".to_string(),
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
        assert!(
            backend
                .uninstall_with_logs(&pkg, &|line| {
                    logs.lock().unwrap().push(line.to_string());
                })
                .is_ok()
        );

        assert_eq!(
            *logs.lock().unwrap(),
            vec!["Removing /home/user/.cargo/bin/rg".to_string()]
        );
    }
}
