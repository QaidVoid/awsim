use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub trait BlobInventory: Send + Sync {
    fn known_blobs(&self) -> Vec<(String, String, String)>;
}

#[derive(Debug)]
pub struct BodyStore {
    root: PathBuf,
}

impl BodyStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn group_dir(&self, group: &str) -> PathBuf {
        self.root.join(group)
    }

    pub fn blob_path(&self, group: &str, bucket: &str, key: &str) -> io::Result<PathBuf> {
        let bucket_root = join_safe(&self.group_dir(group), bucket)?;
        join_safe(&bucket_root, key)
    }

    pub fn write_blob(
        &self,
        group: &str,
        bucket: &str,
        key: &str,
        bytes: &[u8],
    ) -> io::Result<PathBuf> {
        let path = self.blob_path(group, bucket, key)?;
        atomic_write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_blob(&self, group: &str, bucket: &str, key: &str) -> io::Result<Vec<u8>> {
        let path = self.blob_path(group, bucket, key)?;
        fs::read(path)
    }

    pub fn delete_blob(&self, group: &str, bucket: &str, key: &str) -> io::Result<()> {
        let path = self.blob_path(group, bucket, key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn delete_bucket(&self, group: &str, bucket: &str) -> io::Result<()> {
        let bucket_root = join_safe(&self.group_dir(group), bucket)?;
        match fs::remove_dir_all(&bucket_root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn total_size(&self) -> io::Result<u64> {
        let mut total: u64 = 0;
        walk_files(&self.root, &mut |_path, meta| {
            total = total.saturating_add(meta.len());
        })?;
        Ok(total)
    }

    pub fn evict_to_fit(&self, reserve: u64) -> io::Result<(usize, u64)> {
        let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        walk_files(&self.root, &mut |path, meta| {
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((path.to_path_buf(), meta.len(), mtime));
        })?;

        entries.sort_by_key(|e| e.2);

        let mut current: u64 = entries.iter().map(|(_, len, _)| *len).sum();
        let target = current.saturating_sub(reserve);

        let mut deleted: usize = 0;
        let mut freed: u64 = 0;

        for (path, len, _) in entries {
            if current <= target {
                break;
            }
            match fs::remove_file(&path) {
                Ok(()) => {
                    deleted += 1;
                    freed = freed.saturating_add(len);
                    current = current.saturating_sub(len);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "BodyStore eviction skipping file");
                }
            }
        }

        Ok((deleted, freed))
    }

    pub fn gc_orphaned(
        &self,
        groups: &[&str],
        known: &HashSet<(String, String, String)>,
    ) -> io::Result<(usize, u64)> {
        let mut deleted_files: usize = 0;
        let mut freed_bytes: u64 = 0;

        for group in groups {
            let group_root = self.group_dir(group);
            if !group_root.exists() {
                continue;
            }

            let bucket_dirs = match fs::read_dir(&group_root) {
                Ok(entries) => entries,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };

            for bucket_entry in bucket_dirs.flatten() {
                let bucket_path = bucket_entry.path();
                let file_type = match bucket_entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if !file_type.is_dir() {
                    continue;
                }
                let bucket_name = match bucket_entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                let mut files: Vec<(PathBuf, String)> = Vec::new();
                collect_files(&bucket_path, &bucket_path, &mut files)?;

                for (path, key) in files {
                    let triple = ((*group).to_string(), bucket_name.clone(), key);
                    if known.contains(&triple) {
                        continue;
                    }
                    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    match fs::remove_file(&path) {
                        Ok(()) => {
                            deleted_files += 1;
                            freed_bytes += size;
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }

                remove_empty_dirs(&bucket_path, &bucket_path)?;
                let _ = fs::remove_dir(&bucket_path);
            }

            let _ = fs::remove_dir(&group_root);
        }

        Ok((deleted_files, freed_bytes))
    }
}

fn walk_files(root: &Path, visit: &mut dyn FnMut(&Path, &fs::Metadata)) -> io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!(path = %dir.display(), error = %e, "BodyStore walk skipping dir");
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                match fs::metadata(&path) {
                    Ok(meta) => visit(&path, &meta),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "BodyStore walk skipping file");
                    }
                }
            }
        }
    }
    Ok(())
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            collect_files(base, &path, out)?;
        } else if file_type.is_file() {
            let rel = match path.strip_prefix(base) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let key = rel
                .components()
                .filter_map(|c| match c {
                    Component::Normal(s) => s.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/");
            if !key.is_empty() {
                out.push((path, key));
            }
        }
    }
    Ok(())
}

fn remove_empty_dirs(base: &Path, dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(ft) = entry.file_type()
            && ft.is_dir()
        {
            remove_empty_dirs(base, &path)?;
            let _ = fs::remove_dir(&path);
        }
    }
    if dir != base {
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

fn join_safe(base: &Path, rel: &str) -> io::Result<PathBuf> {
    let trimmed = rel.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty key"));
    }

    let candidate = Path::new(trimmed);
    if candidate.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path traversal",
        ));
    }

    for comp in candidate.components() {
        match comp {
            Component::Normal(_) => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "path traversal",
                ));
            }
        }
    }

    Ok(base.join(candidate))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}."))
            .unwrap_or_default()
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("awsim-bs-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_then_read_simple_key() {
        let root = tmp_root("simple");
        let store = BodyStore::new(root.clone());
        store
            .write_blob("objects", "buck", "hello.txt", b"hi there")
            .unwrap();
        let got = store.read_blob("objects", "buck", "hello.txt").unwrap();
        assert_eq!(got, b"hi there");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_then_read_nested_key() {
        let root = tmp_root("nested");
        let store = BodyStore::new(root.clone());
        store
            .write_blob("objects", "buck", "folder/sub/file.txt", b"deep")
            .unwrap();
        let got = store
            .read_blob("objects", "buck", "folder/sub/file.txt")
            .unwrap();
        assert_eq!(got, b"deep");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_blob_removes_file() {
        let root = tmp_root("delobj");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "buck", "k", b"x").unwrap();
        store.delete_blob("objects", "buck", "k").unwrap();
        assert!(store.read_blob("objects", "buck", "k").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_bucket_removes_dir() {
        let root = tmp_root("delbuck");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "buck", "a", b"x").unwrap();
        store.write_blob("objects", "buck", "b/c", b"y").unwrap();
        store.delete_bucket("objects", "buck").unwrap();
        assert!(!store.group_dir("objects").join("buck").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_path_traversal() {
        let root = tmp_root("trav");
        let store = BodyStore::new(root.clone());
        assert!(store.write_blob("objects", "buck", "../foo", b"x").is_err());
        assert!(
            store
                .write_blob("objects", "buck", "foo/../bar", b"x")
                .is_err()
        );
        assert!(store.write_blob("objects", "buck", "", b"x").is_err());
        assert!(store.write_blob("objects", "buck", "/", b"x").is_err());
        store
            .write_blob("objects", "buck", "/abs/path", b"x")
            .unwrap();
        let stored = store.blob_path("objects", "buck", "/abs/path").unwrap();
        assert!(stored.starts_with(store.group_dir("objects").join("buck")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multiple_groups_isolated() {
        let root = tmp_root("groups");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "buck", "k", b"object").unwrap();
        store.write_blob("code", "buck", "k", b"code").unwrap();
        assert_eq!(store.read_blob("objects", "buck", "k").unwrap(), b"object");
        assert_eq!(store.read_blob("code", "buck", "k").unwrap(), b"code");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multipart_style_layout() {
        let root = tmp_root("mp");
        let store = BodyStore::new(root.clone());
        store
            .write_blob("multipart", "buck", "uid/1", b"part1")
            .unwrap();
        store
            .write_blob("multipart", "buck", "uid/2", b"part2")
            .unwrap();
        assert_eq!(
            store.read_blob("multipart", "buck", "uid/1").unwrap(),
            b"part1"
        );
        assert_eq!(
            store.read_blob("multipart", "buck", "uid/2").unwrap(),
            b"part2"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_orphaned_all_known_no_deletions() {
        let root = tmp_root("gc-allknown");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "b1", "k1", b"x").unwrap();
        store.write_blob("objects", "b1", "k2", b"y").unwrap();
        store.write_blob("objects", "b2", "deep/k3", b"z").unwrap();

        let mut known = HashSet::new();
        known.insert(("objects".to_string(), "b1".to_string(), "k1".to_string()));
        known.insert(("objects".to_string(), "b1".to_string(), "k2".to_string()));
        known.insert((
            "objects".to_string(),
            "b2".to_string(),
            "deep/k3".to_string(),
        ));

        let (deleted, freed) = store.gc_orphaned(&["objects"], &known).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(freed, 0);
        assert_eq!(store.read_blob("objects", "b1", "k1").unwrap(), b"x");
        assert_eq!(store.read_blob("objects", "b2", "deep/k3").unwrap(), b"z");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_orphaned_none_known_deletes_everything() {
        let root = tmp_root("gc-noknown");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "b1", "k1", b"abcd").unwrap();
        store.write_blob("objects", "b1", "k2", b"ef").unwrap();
        store
            .write_blob("objects", "b2", "deep/k3", b"hijk")
            .unwrap();

        let known: HashSet<(String, String, String)> = HashSet::new();
        let (deleted, freed) = store.gc_orphaned(&["objects"], &known).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(freed, 4 + 2 + 4);
        assert!(!store.group_dir("objects").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_orphaned_mixed() {
        let root = tmp_root("gc-mixed");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "b1", "keep", b"K").unwrap();
        store.write_blob("objects", "b1", "drop", b"DROP").unwrap();
        store
            .write_blob("objects", "b2", "x/y/z", b"orphan")
            .unwrap();

        let mut known = HashSet::new();
        known.insert(("objects".to_string(), "b1".to_string(), "keep".to_string()));

        let (deleted, freed) = store.gc_orphaned(&["objects"], &known).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(freed, 4 + 6);
        assert_eq!(store.read_blob("objects", "b1", "keep").unwrap(), b"K");
        assert!(!store.group_dir("objects").join("b2").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn total_size_sums_all_files() {
        let root = tmp_root("totalsize");
        let store = BodyStore::new(root.clone());
        store.write_blob("g1", "b1", "a", b"abc").unwrap();
        store.write_blob("g1", "b1", "b/c", b"defgh").unwrap();
        store.write_blob("g2", "b2", "x", b"y").unwrap();
        let total = store.total_size().unwrap();
        assert_eq!(total, 3 + 5 + 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn evict_to_fit_deletes_oldest_first() {
        let root = tmp_root("evictold");
        let store = BodyStore::new(root.clone());
        store.write_blob("g", "b", "a", b"AAAA").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        store.write_blob("g", "b", "b", b"BBBB").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        store.write_blob("g", "b", "c", b"CCCC").unwrap();

        let path_a = store.blob_path("g", "b", "a").unwrap();
        let path_b = store.blob_path("g", "b", "b").unwrap();
        let path_c = store.blob_path("g", "b", "c").unwrap();

        let (deleted, freed) = store.evict_to_fit(8).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(freed, 8);
        assert!(!path_a.exists());
        assert!(!path_b.exists());
        assert!(path_c.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn evict_to_fit_returns_zero_when_already_empty() {
        let root = tmp_root("evictempty");
        let store = BodyStore::new(root.clone());
        let (deleted, freed) = store.evict_to_fit(100).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(freed, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn evict_to_fit_cannot_free_more_than_present() {
        let root = tmp_root("evictover");
        let store = BodyStore::new(root.clone());
        store.write_blob("g", "b", "a", b"AAAA").unwrap();
        let (deleted, freed) = store.evict_to_fit(1_000_000).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(freed, 4);
        assert_eq!(store.total_size().unwrap(), 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gc_orphaned_groups_filter_protects_others() {
        let root = tmp_root("gc-filter");
        let store = BodyStore::new(root.clone());
        store.write_blob("objects", "b", "a", b"A").unwrap();
        store.write_blob("multipart", "b", "u/1", b"P").unwrap();
        store.write_blob("ecr", "repo", "sha256:abc", b"L").unwrap();

        let known: HashSet<(String, String, String)> = HashSet::new();
        let (deleted, _freed) = store.gc_orphaned(&["objects"], &known).unwrap();
        assert_eq!(deleted, 1);
        assert!(!store.group_dir("objects").exists());
        assert_eq!(store.read_blob("multipart", "b", "u/1").unwrap(), b"P");
        assert_eq!(store.read_blob("ecr", "repo", "sha256:abc").unwrap(), b"L");
        let _ = fs::remove_dir_all(&root);
    }
}
