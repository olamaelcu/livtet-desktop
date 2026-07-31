use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use async_trait::async_trait;
use camino::Utf8PathBuf;
use livtet_core::DbId;
use livtet_core::covers::{CachedCover, CoverError, CoverResult, CoverStorage, encode_cover};
use tracing::warn;

fn hash_key(key: &str) -> String {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn parse_content_key(key: &str) -> Option<(&str, &str, &str, &str, &str)> {
    let mut parts = key.split("::");
    let provider = parts.next()?;
    let identifier_type = parts.next()?;
    let identifier_value = parts.next()?;
    let size = parts.next()?;
    let ext = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((provider, identifier_type, identifier_value, size, ext))
}

pub struct CacacheStorage {
    entries_dir: PathBuf,
    markers_dir: PathBuf,
    permanent_dir: Utf8PathBuf,
}

impl CacacheStorage {
    pub fn new(cache_dir: Utf8PathBuf, permanent_dir: Utf8PathBuf) -> Self {
        let entries_dir = cache_dir.as_std_path().join("entries");
        let markers_dir = cache_dir.as_std_path().join("markers");
        Self {
            entries_dir,
            markers_dir,
            permanent_dir,
        }
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.entries_dir.join(hash_key(key))
    }

    fn marker_dir(&self, edition_id: DbId) -> PathBuf {
        self.markers_dir.join(edition_id.to_string())
    }

    fn permanent_path_impl(
        permanent_dir: &Utf8PathBuf,
        edition_id: DbId,
        ext: &str,
    ) -> Utf8PathBuf {
        let safe_ext = ext.replace(['.', '/', '\\'], "");
        permanent_dir
            .join(edition_id.to_string())
            .join(format!("cover.{safe_ext}"))
    }
}

#[async_trait]
impl CoverStorage for CacacheStorage {
    async fn store(&mut self, key: &str, bytes: &[u8]) -> CoverResult<()> {
        let path = self.entry_path(key);
        if let Some(parent) = path.parent() {
            fs_err::tokio::create_dir_all(parent)
                .await
                .map_err(CoverError::Io)?;
        }
        fs_err::tokio::write(&path, bytes)
            .await
            .map_err(CoverError::Io)?;
        Ok(())
    }

    async fn copy_to_permanent(
        &mut self,
        cache_key: &str,
        edition_id: DbId,
    ) -> CoverResult<String> {
        let ext = cache_key.rsplit("::").next().unwrap_or("jpg").to_string();

        let perm_path = Self::permanent_path_impl(&self.permanent_dir, edition_id, &ext);

        if let Some(parent) = perm_path.parent() {
            fs_err::tokio::create_dir_all(parent)
                .await
                .map_err(CoverError::Io)?;
        }

        let entry_path = self.entry_path(cache_key);
        let bytes = fs_err::tokio::read(&entry_path)
            .await
            .map_err(CoverError::Io)?;

        fs_err::tokio::write(&perm_path, &bytes)
            .await
            .map_err(CoverError::Io)?;

        let marker_dir = self.marker_dir(edition_id);
        fs_err::tokio::create_dir_all(&marker_dir)
            .await
            .map_err(CoverError::Io)?;

        let marker_path = marker_dir.join(hash_key(cache_key));
        let marker_content = cache_key.as_bytes();
        fs_err::tokio::write(&marker_path, marker_content)
            .await
            .map_err(CoverError::Io)?;

        Ok(perm_path.to_string())
    }

    fn permanent_path(&self, edition_id: DbId, ext: &str) -> Utf8PathBuf {
        Self::permanent_path_impl(&self.permanent_dir, edition_id, ext)
    }

    async fn list_cached(&self, edition_id: DbId) -> CoverResult<Vec<CachedCover>> {
        let marker_dir = self.marker_dir(edition_id);

        let entries_result = fs_err::tokio::read_dir(&marker_dir).await;
        let mut dir = match entries_result {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CoverError::Io(e)),
        };

        let mut covers = Vec::new();
        while let Ok(Some(entry)) = dir.next_entry().await {
            if !entry
                .file_type()
                .await
                .map(|t| t.is_file())
                .unwrap_or(false)
            {
                continue;
            }

            let content_key = match fs_err::tokio::read_to_string(entry.path()).await {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, path = ?entry.path(), "failed to read marker");
                    continue;
                }
            };

            let Some((provider, _identifier_type, _identifier_value, size, ext)) =
                parse_content_key(&content_key)
            else {
                warn!(key = %content_key, "malformed content key");
                continue;
            };

            let entry_path = self.entry_path(&content_key);
            let bytes = match fs_err::tokio::read(&entry_path).await {
                Ok(b) => b,
                Err(e) => {
                    warn!(key = %content_key, error = %e, "cached entry missing");
                    continue;
                }
            };

            let display_path =
                Some(Self::permanent_path_impl(&self.permanent_dir, edition_id, ext).to_string());

            let (blurhash, dominant_color) = if let Some(ref dp) = display_path {
                match encode_cover(camino::Utf8Path::new(dp)) {
                    Ok(meta) => (Some(meta.blurhash), Some(meta.dominant_color)),
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };

            covers.push(CachedCover {
                key: content_key.clone(),
                provider: provider.to_string(),
                size: size.to_string(),
                ext: ext.to_string(),
                bytes,
                edition_id,
                display_path,
                blurhash,
                dominant_color,
            });
        }

        Ok(covers)
    }
}

impl CacacheStorage {
    pub async fn remove(&mut self, edition_id: DbId) -> CoverResult<()> {
        let marker_dir = self.marker_dir(edition_id);
        let entries_result = fs_err::tokio::read_dir(&marker_dir).await;
        let mut dir = match entries_result {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(CoverError::Io(e)),
        };
        while let Ok(Some(entry)) = dir.next_entry().await {
            if !entry
                .file_type()
                .await
                .map(|t| t.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let content_key = match fs_err::tokio::read_to_string(entry.path()).await {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, path = ?entry.path(), "failed to read marker during remove");
                    continue;
                }
            };
            let entry_path = self.entry_path(&content_key);
            if entry_path.exists() {
                let _ = fs_err::tokio::remove_file(&entry_path).await;
            }
            let _ = fs_err::tokio::remove_file(entry.path()).await;
        }
        let _ = fs_err::tokio::remove_dir_all(&marker_dir).await;
        let perm_dir = self.permanent_dir.join(edition_id.to_string());
        let perm_std = perm_dir.as_std_path();
        if perm_std.exists() {
            let _ = fs_err::tokio::remove_dir_all(perm_std).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_content_key(
        provider: &str,
        identifier_type: &str,
        identifier_value: &str,
        size: &str,
        ext: &str,
    ) -> String {
        format!("{provider}::{identifier_type}::{identifier_value}::{size}::{ext}")
    }

    #[test]
    fn encode_decode_roundtrip() {
        let key = encode_content_key("google_books", "google_books_id", "abc123", "1", "jpg");
        let parsed = parse_content_key(&key);
        assert!(parsed.is_some());
        let (provider, id_type, id_value, size, ext) = parsed.unwrap();
        assert_eq!(provider, "google_books");
        assert_eq!(id_type, "google_books_id");
        assert_eq!(id_value, "abc123");
        assert_eq!(size, "1");
        assert_eq!(ext, "jpg");
    }

    #[test]
    fn hash_key_is_deterministic() {
        let h1 = hash_key("openlibrary::isbn::9780141439518::M::jpg");
        let h2 = hash_key("openlibrary::isbn::9780141439518::M::jpg");
        assert_eq!(h1, h2);
    }

    #[test]
    fn permanent_path_rejects_traversal() {
        let storage = CacacheStorage::new(
            Utf8PathBuf::from("/tmp/cache"),
            Utf8PathBuf::from("/tmp/permanent"),
        );
        let path = storage.permanent_path(DbId::new(), "../../etc/passwd");
        assert!(!path.as_str().contains(".."));
    }

    #[tokio::test]
    async fn store_and_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();

        let mut storage = CacacheStorage::new(cache_dir.clone(), Utf8PathBuf::from("/tmp/covers"));

        let key = encode_content_key("test", "isbn", "123", "M", "png");
        storage.store(&key, &[0x89, 0x50, 0x4e]).await.unwrap();

        let entry_path = storage.entry_path(&key);
        let bytes = fs_err::tokio::read(&entry_path).await.unwrap();
        assert_eq!(bytes, &[0x89, 0x50, 0x4e]);
    }

    #[tokio::test]
    async fn copy_to_permanent_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(tmp.path().join("cache")).unwrap();
        let perm_dir = Utf8PathBuf::from_path_buf(tmp.path().join("covers")).unwrap();

        let mut storage = CacacheStorage::new(cache_dir.clone(), perm_dir.clone());

        let edition_id = DbId::new();
        let content_key = encode_content_key("openlibrary", "isbn", "9780141439518", "M", "jpg");
        storage
            .store(&content_key, &[0xff, 0xd8, 0xff])
            .await
            .unwrap();

        let perm_path = storage
            .copy_to_permanent(&content_key, edition_id)
            .await
            .unwrap();

        assert!(perm_path.contains(&edition_id.to_string()));
        assert!(perm_path.ends_with(".jpg"));

        let stats = fs_err::tokio::metadata(&perm_path).await.unwrap();
        assert_eq!(stats.len(), 3);

        let cached = storage.list_cached(edition_id).await.unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].key, content_key);
        assert_eq!(cached[0].provider, "openlibrary");
        assert_eq!(cached[0].size, "M");
        assert_eq!(cached[0].ext, "jpg");
        assert_eq!(cached[0].bytes, &[0xff, 0xd8, 0xff]);
        assert_eq!(cached[0].edition_id, edition_id);
        assert!(
            cached[0]
                .display_path
                .as_deref()
                .unwrap()
                .contains(&edition_id.to_string())
        );
    }

    #[tokio::test]
    async fn list_cached_empty_for_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(tmp.path().join("cache")).unwrap();
        let perm_dir = Utf8PathBuf::from_path_buf(tmp.path().join("covers")).unwrap();

        let storage = CacacheStorage::new(cache_dir, perm_dir);
        let cached = storage.list_cached(DbId::new()).await.unwrap();
        assert!(cached.is_empty());
    }
}
