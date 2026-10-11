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
    cached_snapshot: Arc<Mutex<Option<crate::cache::AppCache>>>,
    custom_cache_path: Arc<Mutex<Option<std::path::PathBuf>>>,
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
            cached_snapshot: Arc::new(Mutex::new(None)),
            custom_cache_path: Arc::new(Mutex::new(None)),
        }
    }

    #[cfg(test)]
    pub fn set_custom_cache_path(&self, path: Option<std::path::PathBuf>) {
        *self.custom_cache_path.lock().unwrap() = path;
    }

    #[cfg(test)]
    pub fn load_from_cache(&self) -> bool {
        let path = self.custom_cache_path.lock().unwrap().clone();
        if let Some(ref p) = path {
            self.load_from_cache_path(p)
        } else {
            self.load_from_cache_path(&crate::cache::default_cache_path())
        }
    }

    #[cfg(test)]
    pub fn load_from_cache_path(&self, path: &std::path::Path) -> bool {
        if let Some(cache) = crate::cache::load_cache_from_path(path) {
            let glib_pkgs: Vec<glib::BoxedAnyObject> = cache
                .packages
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            self.packages.splice(0, self.packages.n_items(), &glib_pkgs);

            let glib_repos: Vec<glib::BoxedAnyObject> = cache
                .repositories
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            self.repositories
                .splice(0, self.repositories.n_items(), &glib_repos);

            let mut counts: HashMap<PackageManager, (usize, usize)> = HashMap::new();
            for pkg in &cache.packages {
                counts.entry(pkg.manager.clone()).or_insert((0, 0)).0 += 1;
            }
            for repo in &cache.repositories {
                counts.entry(repo.manager.clone()).or_insert((0, 0)).1 += 1;
            }

            {
                let mut statuses = self.backend_statuses.lock().unwrap();
                for b in &self.backends {
                    let mgr = b.manager();
                    let (p_cnt, r_cnt) = counts.get(&mgr).copied().unwrap_or((0, 0));
                    statuses.insert(
                        mgr,
                        BackendStatus::Loaded {
                            package_count: p_cnt,
                            repo_count: r_cnt,
                        },
                    );
                }
            }

            *self.cached_snapshot.lock().unwrap() = Some(cache);
            true
        } else {
            false
        }
    }

    pub async fn stream_cache_realtime(&self) {
        #[cfg(test)]
        let custom_path = self.custom_cache_path.lock().unwrap().clone();
        #[cfg(not(test))]
        let custom_path: Option<std::path::PathBuf> = None;

        let cache = task::spawn_blocking(move || {
            if let Some(ref path) = custom_path {
                crate::cache::load_cache_from_path(path)
            } else {
                crate::cache::load_cache()
            }
        })
        .await
        .ok()
        .flatten();

        if let Some(c) = cache {
            self.stream_cache_data(c).await;
        }
    }

    pub async fn stream_cache_data(&self, cache: crate::cache::AppCache) {
        let glib_repos: Vec<glib::BoxedAnyObject> = cache
            .repositories
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.repositories
            .splice(0, self.repositories.n_items(), &glib_repos);

        let mut counts: HashMap<PackageManager, (usize, usize)> = HashMap::new();
        for pkg in &cache.packages {
            counts.entry(pkg.manager.clone()).or_insert((0, 0)).0 += 1;
        }
        for repo in &cache.repositories {
            counts.entry(repo.manager.clone()).or_insert((0, 0)).1 += 1;
        }

        {
            let mut statuses = self.backend_statuses.lock().unwrap();
            for b in &self.backends {
                let mgr = b.manager();
                let (p_cnt, r_cnt) = counts.get(&mgr).copied().unwrap_or((0, 0));
                statuses.insert(
                    mgr,
                    BackendStatus::Loaded {
                        package_count: p_cnt,
                        repo_count: r_cnt,
                    },
                );
            }
        }

        let total = cache.packages.len();
        if total == 0 {
            *self.cached_snapshot.lock().unwrap() = Some(cache);
            return;
        }

        let first_batch_size = 25.min(total);
        let first_chunk: Vec<glib::BoxedAnyObject> = cache.packages[..first_batch_size]
            .iter()
            .cloned()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.packages.splice(0, self.packages.n_items(), &first_chunk);

        let mut current_idx = first_batch_size;
        while current_idx < total {
            tokio::time::sleep(std::time::Duration::from_millis(16)).await;

            let next_idx = (current_idx + 50).min(total);
            let chunk: Vec<glib::BoxedAnyObject> = cache.packages[current_idx..next_idx]
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            let current_len = self.packages.n_items();
            self.packages.splice(current_len, 0, &chunk);
            current_idx = next_idx;
        }

        *self.cached_snapshot.lock().unwrap() = Some(cache);
    }



    pub fn is_refreshing(&self) -> bool {
        self.is_refreshing.load(Ordering::SeqCst)
    }

    pub fn is_operating(&self) -> bool {
        self.is_operating.load(Ordering::SeqCst)
    }

    // Only exercised by tests since the header badges were removed.
    // Test-only helper: production code streams via `uninstall_with_logs`.
    #[cfg(test)]
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

        let is_identical = {
            let snap = self.cached_snapshot.lock().unwrap();
            if let Some(ref cached) = *snap {
                cached.packages == all_pkgs && cached.repositories == all_repos
            } else {
                false
            }
        };

        if !is_identical {
            let glib_pkgs: Vec<glib::BoxedAnyObject> = all_pkgs
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            self.packages.splice(0, self.packages.n_items(), &glib_pkgs);

            let glib_repos: Vec<glib::BoxedAnyObject> = all_repos
                .iter()
                .cloned()
                .map(glib::BoxedAnyObject::new)
                .collect();
            self.repositories
                .splice(0, self.repositories.n_items(), &glib_repos);

            *self.cached_snapshot.lock().unwrap() = Some(crate::cache::AppCache {
                packages: all_pkgs.clone(),
                repositories: all_repos.clone(),
            });
        }

        let summary = RefreshSummary {
            total_packages: total_pkgs,
            total_repositories: total_repos,
            statuses: new_statuses,
        };

        if summary.is_all_successful() {
            let cache_to_save = crate::cache::AppCache {
                packages: all_pkgs,
                repositories: all_repos,
            };
            let custom_path = self.custom_cache_path.lock().unwrap().clone();
            let _ = task::spawn_blocking(move || {
                if let Some(path) = custom_path {
                    let _ = crate::cache::save_cache_to_path(&path, &cache_to_save);
                } else {
                    let _ = crate::cache::save_cache(&cache_to_save);
                }
            })
            .await;
        }

        Some(summary)
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

    // Test-only helper: production code streams via `uninstall_with_logs`.
    #[cfg(test)]
    pub async fn uninstall(&self, package: &Package) -> Result<(), BackendError> {
        self.uninstall_with_logs(package, |_| {}).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{PackageManager, Repository};

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
                .clone()
                .expect("refresh_result must be set")
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

    #[test]
    fn test_app_state_load_from_cache_path_success() {
        let temp_dir = std::env::temp_dir().join(format!("bg_appstate_cache_{}", std::process::id()));
        let cache_path = temp_dir.join("cache.json");

        let pkg = make_test_package("cached-app", false, Some("cached-icon"));
        let repo = Repository {
            id: "cached-repo".to_string(),
            name: "Cached Repo".to_string(),
            manager: PackageManager::Dnf,
            enabled: true,
            url: Some("https://example.com/repo".to_string()),
            file_path: None,
            added_date: None,
        };
        let cache = crate::cache::AppCache {
            packages: vec![pkg],
            repositories: vec![repo],
        };
        crate::cache::save_cache_to_path(&cache_path, &cache).unwrap();

        let b_dnf = Arc::new(MockBackend::new(
            PackageManager::Dnf,
            Ok(BackendData::default()),
        ));
        let state = AppState::with_backends(vec![b_dnf]);

        assert!(state.load_from_cache_path(&cache_path));
        assert_eq!(state.packages.n_items(), 1);
        assert_eq!(state.repositories.n_items(), 1);

        let statuses = state.get_statuses();
        assert_eq!(
            statuses.get(&PackageManager::Dnf),
            Some(&BackendStatus::Loaded {
                package_count: 1,
                repo_count: 1,
            })
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_state_load_from_cache_path_non_existent() {
        let non_existent = std::path::Path::new("/non/existent/cache/bubblegum.json");
        let state = AppState::with_backends(vec![]);
        assert!(!state.load_from_cache_path(non_existent));
        assert_eq!(state.packages.n_items(), 0);
    }

    #[test]
    fn test_app_state_load_from_cache_default() {
        let temp_dir = std::env::temp_dir().join(format!("bg_default_cache_test_{}", std::process::id()));
        let cache_file = temp_dir.join("cache.json");
        let cache = crate::cache::AppCache {
            packages: vec![make_test_package("default-cached", false, None)],
            repositories: vec![],
        };
        crate::cache::save_cache_to_path(&cache_file, &cache).unwrap();

        let state = AppState::with_backends(vec![]);
        state.set_custom_cache_path(Some(cache_file.clone()));
        assert!(state.load_from_cache());
        assert_eq!(state.packages.n_items(), 1);

        state.set_custom_cache_path(None);
        let _ = state.load_from_cache();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_state_load_initial_cache_and_stream_remaining() {
        let temp_dir = std::env::temp_dir().join(format!("bg_stream_cache_test_{}", std::process::id()));
        let cache_file = temp_dir.join("cache.json");

        let mut pkgs = Vec::new();
        for i in 0..120 {
            pkgs.push(make_test_package(&format!("pkg-{}", i), false, None));
        }
        let cache = crate::cache::AppCache {
            packages: pkgs,
            repositories: vec![],
        };
        crate::cache::save_cache_to_path(&cache_file, &cache).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let state = AppState::with_backends(vec![]);
            state.set_custom_cache_path(Some(cache_file.clone()));

            state.stream_cache_realtime().await;
            assert_eq!(state.packages.n_items(), 120);

            // Test small cache (<= 25)
            let small_cache = crate::cache::AppCache {
                packages: vec![make_test_package("small-pkg", false, None)],
                repositories: vec![],
            };
            let state_small = AppState::with_backends(vec![]);
            state_small.stream_cache_data(small_cache).await;
            assert_eq!(state_small.packages.n_items(), 1);

            // Test empty cache
            let empty_cache = crate::cache::AppCache::default();
            let state_empty = AppState::with_backends(vec![]);
            state_empty.stream_cache_data(empty_cache).await;
            assert_eq!(state_empty.packages.n_items(), 0);

            // Test default path branch
            state.set_custom_cache_path(None);
            state.stream_cache_realtime().await;
        });

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_state_fetch_all_persists_cache_and_skips_identical() {
        let temp_dir = std::env::temp_dir().join(format!("bg_fetch_cache_test_{}", std::process::id()));
        let cache_file = temp_dir.join("cache.json");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pkg = make_test_package("persisted-pkg", false, None);
            let b_dnf = Arc::new(MockBackend::new(
                PackageManager::Dnf,
                Ok(BackendData {
                    packages: vec![pkg],
                    repositories: vec![],
                }),
            ));
            let state = AppState::with_backends(vec![b_dnf]);
            state.set_custom_cache_path(Some(cache_file.clone()));

            // Initial fetch saves cache
            let summary = state.fetch_all().await;
            assert!(summary.is_some());
            assert!(cache_file.exists());

            let loaded_cache = crate::cache::load_cache_from_path(&cache_file);
            assert!(loaded_cache.is_some());
            assert_eq!(loaded_cache.unwrap().packages.len(), 1);

            // Fetch again: data is identical, should skip re-splicing
            let summary2 = state.fetch_all().await;
            assert!(summary2.is_some());
            assert_eq!(state.packages.n_items(), 1);

            // Also test default cache path path-branch in fetch_all
            state.set_custom_cache_path(None);
            let summary3 = state.fetch_all().await;
            assert!(summary3.is_some());
        });

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_state_fetch_all_does_not_persist_cache_on_failure() {
        let temp_dir = std::env::temp_dir().join(format!("bg_fetch_fail_test_{}", std::process::id()));
        let cache_file = temp_dir.join("cache.json");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let b_dnf = Arc::new(MockBackend::new(
                PackageManager::Dnf,
                Err(BackendError::Unavailable {
                    program: "dnf".to_string(),
                }),
            ));
            let state = AppState::with_backends(vec![b_dnf]);
            state.set_custom_cache_path(Some(cache_file.clone()));

            let summary = state.fetch_all().await;
            assert!(summary.is_some());
            assert!(!summary.unwrap().is_all_successful());
            assert!(!cache_file.exists());
        });

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
