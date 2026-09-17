use crate::backends::{Backend, BackendData, BackendError, get_all_backends};
use crate::models::{Package, PackageManager};

use gtk4::gio;
use gtk4::gio::prelude::ListModelExt;
use gtk4::glib;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::task;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendStatus {
    Idle,
    Loading,
    Loaded {
        package_count: usize,
        repo_count: usize,
    },
    Unavailable {
        reason: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone)]
pub struct RefreshSummary {
    pub total_packages: usize,
    pub total_repositories: usize,
    pub statuses: HashMap<PackageManager, BackendStatus>,
}

impl RefreshSummary {
    pub fn is_all_successful(&self) -> bool {
        !self.statuses.is_empty()
            && self
                .statuses
                .values()
                .all(|s| matches!(s, BackendStatus::Loaded { .. }))
    }

    pub fn failures_summary(&self) -> String {
        let mut failed = Vec::new();
        for (pm, status) in &self.statuses {
            match status {
                BackendStatus::Unavailable { reason } => {
                    failed.push(format!("{pm} unavailable ({reason})"));
                }
                BackendStatus::Failed { error } => {
                    failed.push(format!("{pm} failed ({error})"));
                }
                _ => {}
            }
        }
        failed.join("; ")
    }
}

pub fn sort_packages(packages: &mut [Package]) {
    packages.sort_by(|a, b| {
        a.is_dependency
            .cmp(&b.is_dependency)
            .then(b.icon.is_some().cmp(&a.icon.is_some()))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[derive(Clone)]
pub struct AppState {
    pub packages: gio::ListStore,
    pub repositories: gio::ListStore,
    pub backend_statuses: Arc<Mutex<HashMap<PackageManager, BackendStatus>>>,
    pub is_refreshing: Arc<AtomicBool>,
    pub is_operating: Arc<AtomicBool>,
    backends: Vec<Arc<dyn Backend>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::with_backends(get_all_backends())
    }

    pub fn with_backends(backends: Vec<Arc<dyn Backend>>) -> Self {
        let mut initial_statuses = HashMap::new();
        for b in &backends {
            initial_statuses.insert(b.manager(), BackendStatus::Idle);
        }

        Self {
            packages: gio::ListStore::new::<glib::BoxedAnyObject>(),
            repositories: gio::ListStore::new::<glib::BoxedAnyObject>(),
            backend_statuses: Arc::new(Mutex::new(initial_statuses)),
            is_refreshing: Arc::new(AtomicBool::new(false)),
            is_operating: Arc::new(AtomicBool::new(false)),
            backends,
        }
    }

    pub fn is_refreshing(&self) -> bool {
        self.is_refreshing.load(Ordering::SeqCst)
    }

    pub fn is_operating(&self) -> bool {
        self.is_operating.load(Ordering::SeqCst)
    }

    pub fn get_statuses(&self) -> HashMap<PackageManager, BackendStatus> {
        self.backend_statuses.lock().unwrap().clone()
    }

    pub async fn fetch_all(&self) -> Option<RefreshSummary> {
        if self.is_refreshing.swap(true, Ordering::SeqCst) {
            // Already refreshing, prevent overlapping refresh
            return None;
        }

        struct RefreshGuard<'a>(&'a AtomicBool);
        impl Drop for RefreshGuard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _guard = RefreshGuard(&self.is_refreshing);

        // Mark backends as loading
        {
            let mut statuses = self.backend_statuses.lock().unwrap();
            for b in &self.backends {
                statuses.insert(b.manager(), BackendStatus::Loading);
            }
        }

        let mut tasks = Vec::new();
        for backend in &self.backends {
            let b = backend.clone();
            tasks.push(task::spawn_blocking(move || {
                let mgr = b.manager();
                let res = b.refresh();
                (mgr, res)
            }));
        }

        let mut all_pkgs = Vec::new();
        let mut all_repos = Vec::new();
        let mut new_statuses = HashMap::new();

        for t in tasks {
            if let Ok((mgr, res)) = t.await {
                match res {
                    Ok(BackendData {
                        mut packages,
                        mut repositories,
                    }) => {
                        let p_count = packages.len();
                        let r_count = repositories.len();
                        all_pkgs.append(&mut packages);
                        all_repos.append(&mut repositories);
                        new_statuses.insert(
                            mgr,
                            BackendStatus::Loaded {
                                package_count: p_count,
                                repo_count: r_count,
                            },
                        );
                    }
                    Err(BackendError::Unavailable { program }) => {
                        new_statuses.insert(
                            mgr,
                            BackendStatus::Unavailable {
                                reason: format!("{program} not found"),
                            },
                        );
                    }
                    Err(BackendError::CommandFailed {
                        program,
                        status,
                        stderr,
                    }) => {
                        let err_msg = if stderr.is_empty() {
                            format!("{program} exited with {status}")
                        } else {
                            format!("{program} exited with {status}: {}", stderr.trim())
                        };
                        new_statuses.insert(mgr, BackendStatus::Failed { error: err_msg });
                    }
                    Err(err) => {
                        new_statuses.insert(
                            mgr,
                            BackendStatus::Failed {
                                error: err.to_string(),
                            },
                        );
                    }
                }
            }
        }

        for p in &mut all_pkgs {
            p.icon = crate::ui::resolve_icon_name(p.icon.as_deref());
        }

        sort_packages(&mut all_pkgs);

        let total_pkgs = all_pkgs.len();
        let total_repos = all_repos.len();

        {
            let mut statuses = self.backend_statuses.lock().unwrap();
            *statuses = new_statuses.clone();
        }

        let glib_pkgs: Vec<glib::BoxedAnyObject> = all_pkgs
            .into_iter()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.packages.splice(0, self.packages.n_items(), &glib_pkgs);

        let glib_repos: Vec<glib::BoxedAnyObject> = all_repos
            .into_iter()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.repositories
            .splice(0, self.repositories.n_items(), &glib_repos);

        Some(RefreshSummary {
            total_packages: total_pkgs,
            total_repositories: total_repos,
            statuses: new_statuses,
        })
    }

    pub async fn check_uninstall_impact(
        &self,
        package: &Package,
    ) -> Result<Vec<String>, BackendError> {
        let backend = self
            .backends
            .iter()
            .find(|b| b.manager() == package.manager)
            .cloned();

        if let Some(backend) = backend {
            let pkg_clone = package.clone();
            task::spawn_blocking(move || backend.check_uninstall_impact(&pkg_clone))
                .await
                .unwrap_or_else(|e| {
                    Err(BackendError::Io {
                        detail: format!("Task execution error: {e}"),
                    })
                })
        } else {
            Ok(Vec::new())
        }
    }

    pub async fn uninstall_with_logs<F>(
        &self,
        package: &Package,
        on_line: F,
    ) -> Result<(), BackendError>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        if self.is_operating.swap(true, Ordering::SeqCst) {
            return Err(BackendError::Io {
                detail: "Another package operation is currently in progress".to_string(),
            });
        }

        let backend = self
            .backends
            .iter()
            .find(|b| b.manager() == package.manager)
            .cloned();

        let is_operating = self.is_operating.clone();
        let pkg_clone = package.clone();

        if let Some(backend) = backend {
            let is_operating_task = is_operating.clone();
            task::spawn_blocking(move || {
                let res = backend.uninstall_with_logs(&pkg_clone, &|line| {
                    on_line(line.to_string());
                });
                is_operating_task.store(false, Ordering::SeqCst);
                res
            })
            .await
            .unwrap_or_else(|e| {
                is_operating.store(false, Ordering::SeqCst);
                Err(BackendError::Io {
                    detail: format!("Task execution error: {e}"),
                })
            })
        } else {
            self.is_operating.store(false, Ordering::SeqCst);
            Err(BackendError::Unavailable {
                program: format!("{}", package.manager),
            })
        }
    }

    #[allow(dead_code)]
    pub async fn uninstall(&self, package: &Package) -> Result<(), BackendError> {
        self.uninstall_with_logs(package, |_| {}).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PackageManager;

    struct MockBackend {
        manager: PackageManager,
        refresh_result: Mutex<Option<Result<BackendData, BackendError>>>,
        uninstall_result: Mutex<Option<Result<(), BackendError>>>,
        uninstalled_packages: Mutex<Vec<Package>>,
        impact_result: Mutex<Option<Result<Vec<String>, BackendError>>>,
        log_messages: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new(manager: PackageManager, refresh_result: Result<BackendData, BackendError>) -> Self {
            Self {
                manager,
                refresh_result: Mutex::new(Some(refresh_result)),
                uninstall_result: Mutex::new(Some(Ok(()))),
                uninstalled_packages: Mutex::new(Vec::new()),
                impact_result: Mutex::new(None),
                log_messages: Mutex::new(Vec::new()),
            }
        }

        fn set_uninstall_result(&self, res: Result<(), BackendError>) {
            *self.uninstall_result.lock().unwrap() = Some(res);
        }

        fn set_impact_result(&self, res: Result<Vec<String>, BackendError>) {
            *self.impact_result.lock().unwrap() = Some(res);
        }

        fn set_log_messages(&self, msgs: Vec<String>) {
            *self.log_messages.lock().unwrap() = msgs;
        }
    }

    impl Backend for MockBackend {
        fn manager(&self) -> PackageManager {
            self.manager.clone()
        }

        fn refresh(&self) -> Result<BackendData, BackendError> {
            self.refresh_result
                .lock()
                .unwrap()
                .take()
                .expect("refresh called multiple times")
        }

        fn check_uninstall_impact(&self, _package: &Package) -> Result<Vec<String>, BackendError> {
            if let Some(res) = self.impact_result.lock().unwrap().clone() {
                res
            } else {
                Ok(Vec::new())
            }
        }

        fn uninstall(&self, package: &Package) -> Result<(), BackendError> {
            self.uninstalled_packages
                .lock()
                .unwrap()
                .push(package.clone());
            self.uninstall_result
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .clone()
        }

        fn uninstall_with_logs(
            &self,
            package: &Package,
            on_line: &(dyn Fn(&str) + Send + Sync),
        ) -> Result<(), BackendError> {
            for line in self.log_messages.lock().unwrap().iter() {
                on_line(line);
            }
            self.uninstall(package)
        }
    }

    fn make_test_package(name: &str, is_dependency: bool, icon: Option<&str>) -> Package {
        Package {
            id: name.to_string(),
            name: name.to_string(),
            manager: PackageManager::Dnf,
            version: "1.0".to_string(),
            source: None,
            icon: icon.map(|s| s.to_string()),
            description: None,
            size: None,
            install_date: None,
            is_dependency,
            arch: None,
            branch: None,
            scope: None,
        }
    }

    #[test]
    fn test_sort_packages() {
        let mut pkgs = vec![
            make_test_package("zebra", false, None),
            make_test_package("dep_alpha", true, Some("icon")),
            make_test_package("Alpha", false, Some("icon")),
            make_test_package("beta", false, None),
            make_test_package("apple", false, Some("icon")),
        ];

        sort_packages(&mut pkgs);

        assert_eq!(pkgs[0].name, "Alpha");
        assert_eq!(pkgs[1].name, "apple");
        assert_eq!(pkgs[2].name, "beta");
        assert_eq!(pkgs[3].name, "zebra");
        assert_eq!(pkgs[4].name, "dep_alpha");
    }

    #[test]
    fn test_app_state_creation() {
        let state = AppState::new();
        assert_eq!(state.packages.n_items(), 0);
        assert_eq!(state.repositories.n_items(), 0);
        assert!(!state.is_refreshing());
        assert!(!state.is_operating());

        let cloned = state.clone();
        assert_eq!(cloned.packages.n_items(), 0);
    }

    #[test]
    fn test_app_state_fetch_all_with_partial_results() {
        let _ = gtk4::init();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pkg1 = make_test_package("pkg-flatpak", false, None);
            let b_flatpak = Arc::new(MockBackend::new(
                PackageManager::Flatpak,
                Ok(BackendData {
                    packages: vec![pkg1],
                    repositories: vec![],
                }),
            ));

            let b_dnf = Arc::new(MockBackend::new(
                PackageManager::Dnf,
                Err(BackendError::CommandFailed {
                    program: "dnf".to_string(),
                    status: 1,
                    stderr: "network timeout".to_string(),
                }),
            ));

            let b_cargo = Arc::new(MockBackend::new(
                PackageManager::Cargo,
                Err(BackendError::Unavailable {
                    program: "cargo".to_string(),
                }),
            ));

            let state = AppState::with_backends(vec![b_flatpak, b_dnf, b_cargo]);

            let summary = state.fetch_all().await.expect("Should return summary");
            assert!(!summary.is_all_successful());
            assert!(summary.failures_summary().contains("DNF failed"));
            assert!(summary.failures_summary().contains("Cargo unavailable"));

            // Partial results preserved: Flatpak package was loaded even though DNF & Cargo failed!
            assert_eq!(state.packages.n_items(), 1);
            assert_eq!(summary.total_packages, 1);

            let statuses = state.get_statuses();
            assert_eq!(
                statuses.get(&PackageManager::Flatpak),
                Some(&BackendStatus::Loaded {
                    package_count: 1,
                    repo_count: 0
                })
            );
            assert!(matches!(
                statuses.get(&PackageManager::Dnf),
                Some(BackendStatus::Failed { .. })
            ));
            assert!(matches!(
                statuses.get(&PackageManager::Cargo),
                Some(BackendStatus::Unavailable { .. })
            ));
        });
    }

    #[test]
    fn test_app_state_prevent_overlapping_refresh() {
        let _ = gtk4::init();
        let state = AppState::with_backends(vec![]);
        state.is_refreshing.store(true, Ordering::SeqCst);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let res = state.fetch_all().await;
            assert!(res.is_none());
        });
    }

    #[test]
    fn test_app_state_uninstall_routing_and_guards() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let b_dnf = Arc::new(MockBackend::new(
                PackageManager::Dnf,
                Ok(BackendData::default()),
            ));
            let state = AppState::with_backends(vec![b_dnf.clone()]);

            let pkg = make_test_package("my-app", false, None);

            // Success uninstall
            assert!(state.uninstall(&pkg).await.is_ok());
            assert_eq!(b_dnf.uninstalled_packages.lock().unwrap().len(), 1);

            // Failure uninstall
            b_dnf.set_uninstall_result(Err(BackendError::CommandFailed {
                program: "pkexec".to_string(),
                status: 1,
                stderr: "auth failed".to_string(),
            }));
            let err = state.uninstall(&pkg).await.unwrap_err();
            match err {
                BackendError::CommandFailed { stderr, .. } => {
                    assert_eq!(stderr, "auth failed");
                }
                _ => panic!("Expected CommandFailed error"),
            }

            // Concurrent operation guard
            state.is_operating.store(true, Ordering::SeqCst);
            let concurrent_err = state.uninstall(&pkg).await.unwrap_err();
            assert!(matches!(concurrent_err, BackendError::Io { .. }));
        });
    }

    #[test]
    fn test_app_state_check_uninstall_impact_and_uninstall_with_logs() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let b_dnf = Arc::new(MockBackend::new(
                PackageManager::Dnf,
                Ok(BackendData::default()),
            ));
            b_dnf.set_impact_result(Ok(vec!["dep1".to_string(), "dep2".to_string()]));
            b_dnf.set_log_messages(vec!["step 1".to_string(), "step 2".to_string()]);

            let state = AppState::with_backends(vec![b_dnf.clone()]);
            let pkg = make_test_package("my-app", false, None);

            // Impact check
            let impact = state.check_uninstall_impact(&pkg).await.unwrap();
            assert_eq!(impact, vec!["dep1".to_string(), "dep2".to_string()]);

            // Streaming uninstall
            let logs = Arc::new(Mutex::new(Vec::new()));
            let logs_clone = logs.clone();
            assert!(
                state
                    .uninstall_with_logs(&pkg, move |line| {
                        logs_clone.lock().unwrap().push(line);
                    })
                    .await
                    .is_ok()
            );

            assert_eq!(
                *logs.lock().unwrap(),
                vec!["step 1".to_string(), "step 2".to_string()]
            );

            // Missing backend error
            let flatpak_pkg = Package {
                manager: PackageManager::Flatpak,
                ..pkg.clone()
            };
            assert!(
                state
                    .uninstall_with_logs(&flatpak_pkg, |_| {})
                    .await
                    .is_err()
            );
            assert_eq!(
                state.check_uninstall_impact(&flatpak_pkg).await.unwrap(),
                Vec::<String>::new()
            );
        });
    }
}
