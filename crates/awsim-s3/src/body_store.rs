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

    pub fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    pub fn multipart_dir(&self) -> PathBuf {
        self.root.join("multipart")
    }

    pub fn object_path(&self, bucket: &str, key: &str) -> io::Result<PathBuf> {
        let bucket_root = self.objects_dir().join(bucket);
        join_safe(&bucket_root, key)
    }

    pub fn part_path(&self, bucket: &str, upload_id: &str, n: u32) -> io::Result<PathBuf> {
        let upload_root = self.multipart_dir().join(bucket).join(upload_id);
        join_safe(&upload_root, &n.to_string())
    }

    pub fn write_object(&self, bucket: &str, key: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.object_path(bucket, key)?;
        atomic_write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_object(&self, bucket: &str, key: &str) -> io::Result<Vec<u8>> {
        let path = self.object_path(bucket, key)?;
        fs::read(path)
    }

    pub fn delete_object(&self, bucket: &str, key: &str) -> io::Result<()> {
        let path = self.object_path(bucket, key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn delete_bucket(&self, bucket: &str) -> io::Result<()> {
        let bucket_root = self.objects_dir().join(bucket);
        match fs::remove_dir_all(&bucket_root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn write_part(
        &self,
        bucket: &str,
        upload_id: &str,
        n: u32,
        bytes: &[u8],
    ) -> io::Result<PathBuf> {
        let path = self.part_path(bucket, upload_id, n)?;
        atomic_write(&path, bytes)?;
        Ok(path)
    }

    pub fn read_part(&self, bucket: &str, upload_id: &str, n: u32) -> io::Result<Vec<u8>> {
        let path = self.part_path(bucket, upload_id, n)?;
        fs::read(path)
    }

    pub fn delete_multipart(&self, bucket: &str, upload_id: &str) -> io::Result<()> {
        let upload_root = self.multipart_dir().join(bucket).join(upload_id);
        match fs::remove_dir_all(&upload_root) {
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
            .write_object("buck", "hello.txt", b"hi there")
            .unwrap();
        let got = store.read_object("buck", "hello.txt").unwrap();
        assert_eq!(got, b"hi there");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_then_read_nested_key() {
        let root = tmp_root("nested");
        let store = BodyStore::new(root.clone());
        store
            .write_object("buck", "folder/sub/file.txt", b"deep")
            .unwrap();
        let got = store.read_object("buck", "folder/sub/file.txt").unwrap();
        assert_eq!(got, b"deep");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_object_removes_file() {
        let root = tmp_root("delobj");
        let store = BodyStore::new(root.clone());
        store.write_object("buck", "k", b"x").unwrap();
        store.delete_object("buck", "k").unwrap();
        assert!(store.read_object("buck", "k").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_bucket_removes_dir() {
        let root = tmp_root("delbuck");
        let store = BodyStore::new(root.clone());
        store.write_object("buck", "a", b"x").unwrap();
        store.write_object("buck", "b/c", b"y").unwrap();
        store.delete_bucket("buck").unwrap();
        assert!(!store.objects_dir().join("buck").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_path_traversal() {
        let root = tmp_root("trav");
        let store = BodyStore::new(root.clone());
        assert!(store.write_object("buck", "../foo", b"x").is_err());
        assert!(store.write_object("buck", "foo/../bar", b"x").is_err());
        assert!(store.write_object("buck", "", b"x").is_err());
        assert!(store.write_object("buck", "/", b"x").is_err());
        store.write_object("buck", "/abs/path", b"x").unwrap();
        let stored = store.object_path("buck", "/abs/path").unwrap();
        assert!(stored.starts_with(store.objects_dir().join("buck")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multipart_round_trip() {
        let root = tmp_root("mp");
        let store = BodyStore::new(root.clone());
        store.write_part("buck", "uid", 1, b"part1").unwrap();
        store.write_part("buck", "uid", 2, b"part2").unwrap();
        assert_eq!(store.read_part("buck", "uid", 1).unwrap(), b"part1");
        assert_eq!(store.read_part("buck", "uid", 2).unwrap(), b"part2");
        store.delete_multipart("buck", "uid").unwrap();
        assert!(store.read_part("buck", "uid", 1).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
