use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PackageManager {
    Dnf,
    Flatpak,
    Cargo,
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageManager::Dnf => write!(f, "DNF"),
            PackageManager::Flatpak => write!(f, "Flatpak"),
            PackageManager::Cargo => write!(f, "Cargo"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manager: PackageManager,
    pub version: String,
    pub source: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub size: Option<String>,
    pub install_date: Option<String>,
    pub is_dependency: bool,
    pub arch: Option<String>,
    pub branch: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub manager: PackageManager,
    pub enabled: bool,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub added_date: Option<NaiveDate>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_manager_display() {
        assert_eq!(format!("{}", PackageManager::Dnf), "DNF");
        assert_eq!(format!("{}", PackageManager::Flatpak), "Flatpak");
        assert_eq!(format!("{}", PackageManager::Cargo), "Cargo");
    }

    #[test]
    fn test_package_manager_traits() {
        let pm1 = PackageManager::Dnf;
        let pm2 = pm1.clone();
        assert_eq!(pm1, pm2);
        assert_ne!(PackageManager::Dnf, PackageManager::Cargo);
        assert_ne!(PackageManager::Flatpak, PackageManager::Cargo);
        assert_eq!(format!("{:?}", PackageManager::Dnf), "Dnf");
    }

    #[test]
    fn test_package_struct() {
        let pkg = Package {
            id: "test-id".to_string(),
            name: "test-pkg".to_string(),
            manager: PackageManager::Dnf,
            version: "1.0.0".to_string(),
            source: Some("fedora".to_string()),
            icon: Some("test-icon".to_string()),
            description: Some("Test description".to_string()),
            size: Some("10 MB".to_string()),
            install_date: Some("Jan 1, 2026".to_string()),
            is_dependency: false,
            arch: Some("x86_64".to_string()),
            branch: Some("stable".to_string()),
            scope: Some("system".to_string()),
        };

        assert_eq!(pkg.id, "test-id");
        assert_eq!(pkg.name, "test-pkg");
        assert_eq!(pkg.manager, PackageManager::Dnf);
        assert_eq!(pkg.version, "1.0.0");
        assert_eq!(pkg.source.as_deref(), Some("fedora"));
        assert_eq!(pkg.icon.as_deref(), Some("test-icon"));
        assert_eq!(pkg.description.as_deref(), Some("Test description"));
        assert_eq!(pkg.size.as_deref(), Some("10 MB"));
        assert_eq!(pkg.install_date.as_deref(), Some("Jan 1, 2026"));
        assert!(!pkg.is_dependency);
        assert_eq!(pkg.arch.as_deref(), Some("x86_64"));
        assert_eq!(pkg.branch.as_deref(), Some("stable"));
        assert_eq!(pkg.scope.as_deref(), Some("system"));

        let cloned = pkg.clone();
        assert_eq!(cloned.name, "test-pkg");
    }

    #[test]
    fn test_repository_struct() {
        let repo = Repository {
            id: "repo-id".to_string(),
            name: "repo-name".to_string(),
            manager: PackageManager::Flatpak,
            enabled: true,
            url: Some("https://example.com/repo".to_string()),
            file_path: Some("/etc/repo".to_string()),
            added_date: NaiveDate::from_ymd_opt(2026, 1, 1),
        };

        assert_eq!(repo.id, "repo-id");
        assert_eq!(repo.name, "repo-name");
        assert_eq!(repo.manager, PackageManager::Flatpak);
        assert!(repo.enabled);
        assert_eq!(repo.url.as_deref(), Some("https://example.com/repo"));
        assert_eq!(repo.file_path.as_deref(), Some("/etc/repo"));
        assert_eq!(repo.added_date, NaiveDate::from_ymd_opt(2026, 1, 1));

        let cloned = repo.clone();
        assert_eq!(cloned.id, "repo-id");
    }
}
