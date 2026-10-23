//! On-disk store. Objects live at `objects/<2 hex>/<rest>` and branch refs at
//! `refs/heads/<name>`, under a root directory such as `.tpt`.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

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

    fn contains(&self, hash: &ContentHash) -> Result<bool, StoreError> {
        Ok(self.object_path(hash).is_file())
    }
}

impl RefStore for FileSystemStore {
    fn read_ref(&self, name: &str) -> Result<Option<ContentHash>, StoreError> {
        read_ref_file(name, &self.ref_path(name)?)
    }

    fn write_ref(&mut self, name: &str, hash: ContentHash) -> Result<(), StoreError> {
        let path = self.ref_path(name)?;
        atomic_write(&path, format!("{hash}\n").as_bytes())
    }

    fn compare_and_swap_ref(
        &mut self,
        name: &str,
        expected: Option<ContentHash>,
        new: ContentHash,
    ) -> Result<bool, StoreError> {
        let path = self.ref_path(name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        // The lock file is created exclusively, so only one writer can hold it.
        // It is removed when `_lock` drops, on every path out of this function.
        let _lock = RefLock::acquire(&path)?;
        if read_ref_file(name, &path)? != expected {
            return Ok(false);
        }
        atomic_write(&path, format!("{new}\n").as_bytes())?;
        Ok(true)
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
const LOCK_SUFFIX: &str = ".lock";

/// An exclusive lock on one ref, held for the duration of a compare-and-swap.
/// A crash can leave the lock file behind; it then has to be removed by hand.
struct RefLock {
    path: PathBuf,
}

impl RefLock {
    fn acquire(ref_path: &Path) -> Result<Self, StoreError> {
        let mut lock = ref_path.as_os_str().to_owned();
        lock.push(LOCK_SUFFIX);
        let path = PathBuf::from(lock);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(Self { path }),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(StoreError::Backend(
                format!("ref is locked by another writer: {}", path.display()),
            )),
            Err(error) => Err(io_error(error)),
        }
    }
}

impl Drop for RefLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_ref_file(name: &str, path: &Path) -> Result<Option<ContentHash>, StoreError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
    };
    text.trim()
        .parse()
        .map(Some)
        .map_err(|_| StoreError::Backend(format!("corrupt ref {name:?}")))
}

/// Source of unique temp-file suffixes, so concurrent writers to the same
/// target never share a temp file.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.{unique}{TEMP_SUFFIX}", process::id()));
    let temp = PathBuf::from(temp);
    let result = write_synced(&temp, data).and_then(|()| {
        // On Windows, rename replaces an existing target.
        fs::rename(&temp, path).map_err(io_error)
    });
    if result.is_err() {
        // Best effort: do not leave a stray temp file behind.
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Writes `data` and flushes it to disk before returning, so the rename that
/// follows never publishes a file whose contents are still in the page cache.
fn write_synced(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let mut file = fs::File::create(path).map_err(io_error)?;
    file.write_all(data).map_err(io_error)?;
    file.sync_all().map_err(io_error)
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
        } else if !name.ends_with(TEMP_SUFFIX) && !name.ends_with(LOCK_SUFFIX) {
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
