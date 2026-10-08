//! Filesystem layout derived from the application data directory.

use camino::Utf8PathBuf;

#[derive(Debug, Clone)]
pub struct Paths {
    pub database_path: Utf8PathBuf,
    pub logs_dir: Utf8PathBuf,
    pub search_index_path: Utf8PathBuf,
    pub covers_dir: Utf8PathBuf,
    pub books_dir: Utf8PathBuf,
    pub plugins_dir: Utf8PathBuf,
}

impl Paths {
    pub fn new(app_dir: &Utf8PathBuf) -> Self {
        let data_dir = app_dir.join("data");
        let search_index = app_dir.join("search");
        let logs = app_dir.join("logs");
        let plugins_dir = app_dir.join("plugins");

        Self {
            covers_dir: data_dir.join("covers"),
            books_dir: data_dir.join("books"),
            database_path: data_dir,
            logs_dir: logs,
            search_index_path: search_index,
            plugins_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn derives_every_path_from_the_app_dir() {
        let paths = Paths::new(&Utf8PathBuf::from("/app"));

        assert_eq!(paths.covers_dir, Utf8PathBuf::from("/app/data/covers"));
        assert_eq!(paths.books_dir, Utf8PathBuf::from("/app/data/books"));
        assert_eq!(paths.logs_dir, Utf8PathBuf::from("/app/logs"));
        assert_eq!(paths.search_index_path, Utf8PathBuf::from("/app/search"));
        assert_eq!(paths.plugins_dir, Utf8PathBuf::from("/app/plugins"));
    }

    /// Every other field joins a name onto its parent, but `database_path` is
    /// the `data` directory itself rather than a file inside it. Pinned because
    /// the name reads like a file path and a caller could reasonably treat it
    /// as one.
    #[test]
    fn database_path_is_the_data_directory_itself() {
        let paths = Paths::new(&Utf8PathBuf::from("/app"));

        assert_eq!(paths.database_path, Utf8PathBuf::from("/app/data"));
        assert_eq!(paths.database_path, paths.covers_dir.parent().unwrap());
    }
}
