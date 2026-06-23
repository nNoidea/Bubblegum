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

#[derive(Debug, Clone)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub manager: PackageManager,
    pub version: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
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
