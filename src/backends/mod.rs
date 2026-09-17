pub mod cargo;
pub mod dnf;
pub mod flatpak;
pub mod runner;

pub use runner::{CommandRunner, SystemCommandRunner};

use crate::models::{Package, PackageManager, Repository};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackendData {
    pub packages: Vec<Package>,
    pub repositories: Vec<Repository>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    Unavailable {
        program: String,
    },
    CommandFailed {
        program: String,
        status: i32,
        stderr: String,
    },
    Parse {
        backend: PackageManager,
        detail: String,
    },
    Io {
        detail: String,
    },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Unavailable { program } => {
                write!(f, "Program '{}' is not available", program)
            }
            BackendError::CommandFailed {
                program,
                status,
                stderr,
            } => {
                write!(
                    f,
                    "Command '{}' failed with status {}: {}",
                    program,
                    status,
                    stderr.trim()
                )
            }
            BackendError::Parse { backend, detail } => {
                write!(f, "Failed to parse {} output: {}", backend, detail)
            }
            BackendError::Io { detail } => write!(f, "I/O error: {}", detail),
        }
    }
}

impl std::error::Error for BackendError {}

pub trait Backend: Send + Sync {
    fn manager(&self) -> PackageManager;
    fn refresh(&self) -> Result<BackendData, BackendError>;
    fn check_uninstall_impact(&self, package: &Package) -> Result<Vec<String>, BackendError> {
        let _ = package;
        Ok(Vec::new())
    }
    fn uninstall(&self, package: &Package) -> Result<(), BackendError>;
    fn uninstall_with_logs(
        &self,
        package: &Package,
        on_line: &(dyn Fn(&str) + Send + Sync),
    ) -> Result<(), BackendError> {
        let _ = on_line;
        self.uninstall(package)
    }
}

pub fn get_all_backends() -> Vec<Arc<dyn Backend>> {
    let runner = Arc::new(SystemCommandRunner);
    vec![
        Arc::new(flatpak::FlatpakBackend::new(runner.clone())),
        Arc::new(cargo::CargoBackend::new(runner.clone())),
        Arc::new(dnf::DnfBackend::new(runner)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_all_backends() {
        let backends = get_all_backends();
        assert_eq!(backends.len(), 3);
        assert_eq!(backends[0].manager(), PackageManager::Flatpak);
        assert_eq!(backends[1].manager(), PackageManager::Cargo);
        assert_eq!(backends[2].manager(), PackageManager::Dnf);
    }

    #[test]
    fn test_backend_error_display() {
        let e1 = BackendError::Unavailable {
            program: "dnf".to_string(),
        };
        assert_eq!(e1.to_string(), "Program 'dnf' is not available");

        let e2 = BackendError::CommandFailed {
            program: "cargo".to_string(),
            status: 101,
            stderr: "fatal error\n".to_string(),
        };
        assert_eq!(
            e2.to_string(),
            "Command 'cargo' failed with status 101: fatal error"
        );

        let e3 = BackendError::Parse {
            backend: PackageManager::Flatpak,
            detail: "invalid columns".to_string(),
        };
        assert_eq!(
            e3.to_string(),
            "Failed to parse Flatpak output: invalid columns"
        );

        let e4 = BackendError::Io {
            detail: "permission denied".to_string(),
        };
        assert_eq!(e4.to_string(), "I/O error: permission denied");
    }

    #[test]
    fn test_backend_data_default() {
        let data = BackendData::default();
        assert!(data.packages.is_empty());
        assert!(data.repositories.is_empty());
    }

    struct DummyBackend;
    impl Backend for DummyBackend {
        fn manager(&self) -> PackageManager {
            PackageManager::Cargo
        }
        fn refresh(&self) -> Result<BackendData, BackendError> {
            Ok(BackendData::default())
        }
        fn uninstall(&self, _package: &Package) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[test]
    fn test_backend_default_trait_methods() {
        let dummy = DummyBackend;
        let pkg = Package {
            id: "test".to_string(),
            name: "test".to_string(),
            manager: PackageManager::Cargo,
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

        let impact = dummy.check_uninstall_impact(&pkg).unwrap();
        assert!(impact.is_empty());

        let res = dummy.uninstall_with_logs(&pkg, &|_| {});
        assert!(res.is_ok());
    }
}
