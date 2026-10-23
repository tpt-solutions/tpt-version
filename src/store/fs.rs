//! On-disk store. Objects live at `objects/<2 hex>/<rest>` and branch refs at
//! `refs/heads/<name>`, under a root directory such as `.tpt`.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use alloc::string::String;
use alloc::vec::Vec;

use super::{CasStore, RefStore, verify};
use crate::error::StoreError;
use crate::object::ContentHash;

/// A store backed by the local file system.
///
/// Writes go to a temporary file and are renamed into place, so a crash never
/// leaves a half-written object or ref. Concurrent writers to the same ref are
/// not coordinated: the last rename wins.
#[derive(Debug, Clone)]
pub struct FileSystemStore {
    root: PathBuf,
}

impl FileSystemStore {
    /// Opens the store rooted at `root`, creating its directories if needed.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(root.join("objects")).map_err(io_error)?;
        fs::create_dir_all(root.join("refs").join("heads")).map_err(io_error)?;
        Ok(Self { root })
    }

    fn object_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_string();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    fn ref_path(&self, name: &str) -> Result<PathBuf, StoreError> {
        let invalid = name.is_empty()
            || name.contains('\\')
            || name.starts_with('/')
            || name.split('/').any(|part| part.is_empty() || part == "..");
        if invalid {
            return Err(StoreError::Backend(format!("unsafe ref name {name:?}")));
        }
        Ok(name
            .split('/')
            .fold(self.root.join("refs").join("heads"), |path, part| {
                path.join(part)
            }))
    }
}

impl CasStore for FileSystemStore {
    fn put(&mut self, hash: ContentHash, data: &[u8]) -> Result<(), StoreError> {
        verify(&hash, data)?;
        let path = self.object_path(&hash);
        if path.is_file() {
            return Ok(());
        }
        atomic_write(&path, data)
    }

    fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, StoreError> {
        let data = match fs::read(self.object_path(hash)) {
            Ok(data) => data,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(StoreError::NotFound(*hash));
            }
            Err(error) => return Err(io_error(error)),
        };
        verify(hash, &data)?;
        Ok(data)
    }

    fn contains(&self, hash: &ContentHash) -> bool {
        self.object_path(hash).is_file()
    }
}

impl RefStore for FileSystemStore {
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError> {
        let text = match fs::read_to_string(self.ref_path(name)?) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        text.trim()
            .parse()
            .map(Some)
            .map_err(|_| StoreError::Backend(format!("corrupt ref {name:?}")))
    }

    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError> {
        let path = self.ref_path(name)?;
        atomic_write(&path, format!("{hash}\n").as_bytes())
    }

    fn delete_ref(&mut self, name: &str) -> Result<bool, StoreError> {
        match fs::remove_file(self.ref_path(name)?) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(error)),
        }
    }

    fn list_refs(&self) -> Result<Vec<String>, StoreError> {
        let mut names = Vec::new();
        collect_refs(&self.root.join("refs").join("heads"), "", &mut names)?;
        names.sort();
        Ok(names)
    }
}

const TEMP_SUFFIX: &str = ".writing";

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(TEMP_SUFFIX);
    let temp = PathBuf::from(temp);
    fs::write(&temp, data).map_err(io_error)?;
    // On Windows, rename replaces an existing target.
    fs::rename(&temp, path).map_err(io_error)
}

fn collect_refs(dir: &Path, prefix: &str, names: &mut Vec<String>) -> Result<(), StoreError> {
    for entry in fs::read_dir(dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let name = if prefix.is_empty() {
            file_name
        } else {
            format!("{prefix}/{file_name}")
        };
        if entry.file_type().map_err(io_error)?.is_dir() {
            collect_refs(&entry.path(), &name, names)?;
        } else if !name.ends_with(TEMP_SUFFIX) {
            names.push(name);
        }
    }
    Ok(())
}

fn io_error(error: io::Error) -> StoreError {
    StoreError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::CasStore;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-version-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn persists_objects_and_refs_across_reopen() {
        let root = scratch_dir("fs-persist");
        let hash = ContentHash::of(b"payload");
        {
            let mut store = FileSystemStore::open(&root).unwrap();
            store.put(hash, b"payload").unwrap();
            store.write_ref("feature/x", hash).unwrap();
        }
        let store = FileSystemStore::open(&root).unwrap();
        assert_eq!(store.get(&hash).unwrap(), b"payload");
        assert_eq!(store.read_ref("feature/x").unwrap(), Some(hash));
        assert_eq!(store.list_refs().unwrap(), ["feature/x"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn detects_corruption_on_disk() {
        let root = scratch_dir("fs-corrupt");
        let mut store = FileSystemStore::open(&root).unwrap();
        let hash = ContentHash::of(b"clean");
        store.put(hash, b"clean").unwrap();
        fs::write(store.object_path(&hash), b"dirty").unwrap();
        assert!(matches!(
            store.get(&hash),
            Err(StoreError::HashMismatch { .. })
        ));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rejects_path_traversal_in_ref_names() {
        let root = scratch_dir("fs-traversal");
        let mut store = FileSystemStore::open(&root).unwrap();
        let hash = ContentHash::of(b"x");
        assert!(store.write_ref("../escape", hash).is_err());
        assert!(store.write_ref("a//b", hash).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
