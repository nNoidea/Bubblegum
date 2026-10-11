use crate::models::{Package, Repository};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCache {
    pub packages: Vec<Package>,
    pub repositories: Vec<Repository>,
}

pub fn resolve_cache_dir(xdg_cache: Option<&str>, home: Option<&str>) -> PathBuf {
    if let Some(xdg_cache) = xdg_cache {
        PathBuf::from(xdg_cache).join("bubblegum")
    } else if let Some(home) = home {
        PathBuf::from(home).join(".cache").join("bubblegum")
    } else {
        PathBuf::from(".bubblegum")
    }
}

pub fn default_cache_dir() -> PathBuf {
    let xdg = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve_cache_dir(xdg.as_deref(), home.as_deref())
}

pub fn default_cache_path() -> PathBuf {
    default_cache_dir().join("cache.json")
}

pub fn load_cache_from_path(path: &Path) -> Option<AppCache> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<AppCache>(&content).ok()
}

pub fn save_cache_to_path(path: &Path, cache: &AppCache) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string(cache)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(path, data)
}

pub fn load_cache() -> Option<AppCache> {
    load_cache_from_path(&default_cache_path())
}

pub fn save_cache(cache: &AppCache) -> Result<(), std::io::Error> {
    save_cache_to_path(&default_cache_path(), cache)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PackageManager;

    fn sample_cache() -> AppCache {
        AppCache {
            packages: vec![Package {
                id: "test.app".to_string(),
                name: "Test App".to_string(),
                manager: PackageManager::Flatpak,
                version: "1.0.0".to_string(),
                source: Some("flathub".to_string()),
                icon: Some("test-icon".to_string()),
                description: Some("Description".to_string()),
                size: Some("10 MB".to_string()),
                install_date: None,
                is_dependency: false,
                arch: Some("x86_64".to_string()),
                branch: Some("stable".to_string()),
                scope: Some("user".to_string()),
            }],
            repositories: vec![Repository {
                id: "flathub".to_string(),
                name: "Flathub".to_string(),
                manager: PackageManager::Flatpak,
                enabled: true,
                url: Some("https://dl.flathub.org/repo/".to_string()),
                file_path: None,
                added_date: None,
            }],
        }
    }

    #[test]
    fn test_save_and_load_cache_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("bg_cache_test_{}", std::process::id()));
        let cache_path = temp_dir.join("sub").join("cache.json");

        let cache = sample_cache();
        let save_res = save_cache_to_path(&cache_path, &cache);
        assert!(save_res.is_ok());

        let loaded = load_cache_from_path(&cache_path);
        assert_eq!(loaded, Some(cache));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_cache_non_existent() {
        let dummy = Path::new("/non/existent/path/cache.json");
        assert_eq!(load_cache_from_path(dummy), None);
    }

    #[test]
    fn test_load_cache_malformed_json() {
        let temp_dir = std::env::temp_dir().join(format!("bg_cache_malformed_{}", std::process::id()));
        let cache_path = temp_dir.join("cache.json");
        fs::create_dir_all(&temp_dir).unwrap();
        fs::write(&cache_path, "{ not valid json").unwrap();

        assert_eq!(load_cache_from_path(&cache_path), None);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_default_cache_paths() {
        let p = default_cache_path();
        assert!(p.ends_with("cache.json"));
        assert!(p.to_string_lossy().contains("bubblegum"));

        let dir = default_cache_dir();
        assert!(dir.ends_with("bubblegum"));
    }

    #[test]
    fn test_resolve_cache_dir_variants() {
        assert_eq!(
            resolve_cache_dir(Some("/custom/cache"), Some("/home/user")),
            PathBuf::from("/custom/cache/bubblegum")
        );
        assert_eq!(
            resolve_cache_dir(None, Some("/home/user")),
            PathBuf::from("/home/user/.cache/bubblegum")
        );
        assert_eq!(
            resolve_cache_dir(None, None),
            PathBuf::from(".bubblegum")
        );
    }

    #[test]
    fn test_save_and_load_default_cache() {
        let cache = sample_cache();
        let path = default_cache_path();
        // Backup if it exists
        let backup = if path.exists() {
            fs::read_to_string(&path).ok()
        } else {
            None
        };

        let res = save_cache(&cache);
        assert!(res.is_ok());

        let loaded = load_cache();
        assert_eq!(loaded, Some(cache));

        // Restore backup or clean up
        if let Some(content) = backup {
            let _ = fs::write(&path, content);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
}
