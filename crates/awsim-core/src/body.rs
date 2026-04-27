use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Body {
    InMemory(Vec<u8>),
    OnDisk(PathBuf),
}

impl Body {
    pub fn read_all(&self) -> io::Result<Vec<u8>> {
        match self {
            Self::InMemory(b) => Ok(b.clone()),
            Self::OnDisk(p) => std::fs::read(p),
        }
    }

    pub fn read_string(&self) -> io::Result<String> {
        let bytes = self.read_all()?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn len_hint(&self) -> Option<u64> {
        match self {
            Self::InMemory(b) => Some(b.len() as u64),
            Self::OnDisk(p) => std::fs::metadata(p).ok().map(|m| m.len()),
        }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self::InMemory(bytes)
    }

    pub fn from_string(s: String) -> Self {
        Self::InMemory(s.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn in_memory_read_all_returns_bytes() {
        let body = Body::InMemory(b"hi".to_vec());
        assert_eq!(body.read_all().unwrap(), b"hi");
    }

    #[test]
    fn in_memory_read_string_returns_utf8() {
        let body = Body::InMemory(b"hi".to_vec());
        assert_eq!(body.read_string().unwrap(), "hi");
    }

    #[test]
    fn in_memory_read_string_invalid_utf8_errors() {
        let body = Body::InMemory(vec![0xFF, 0xFE]);
        let err = body.read_string().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn on_disk_round_trip() {
        let dir = std::env::temp_dir().join(format!("awsim-body-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let body = Body::OnDisk(path.clone());
        assert_eq!(body.read_all().unwrap(), b"hello world");
        assert_eq!(body.read_string().unwrap(), "hello world");
        assert_eq!(body.len_hint(), Some(11));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn len_hint_in_memory() {
        let body = Body::InMemory(vec![0; 42]);
        assert_eq!(body.len_hint(), Some(42));
    }

    #[test]
    fn len_hint_on_disk_missing_returns_none() {
        let body = Body::OnDisk(PathBuf::from("/nonexistent/awsim/path/does/not/exist"));
        assert_eq!(body.len_hint(), None);
    }

    #[test]
    fn from_bytes_constructs_in_memory() {
        let body = Body::from_bytes(b"abc".to_vec());
        match body {
            Body::InMemory(b) => assert_eq!(b, b"abc"),
            _ => panic!("expected InMemory"),
        }
    }

    #[test]
    fn from_string_constructs_in_memory() {
        let body = Body::from_string("abc".to_string());
        assert_eq!(body.read_string().unwrap(), "abc");
    }
}
