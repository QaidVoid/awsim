use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

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
}
