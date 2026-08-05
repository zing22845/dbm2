use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::StoreError;
use crate::StoreResult;

static DATA_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Configure the DBM data directory. Call once at process startup before any store access.
///
/// `override_path`: explicit directory from `--data-dir` / `DBM_DATA_DIR`.
/// When `None`, defaults to `{executable_dir}/data`.
pub fn init_data_dir(override_path: Option<PathBuf>) -> StoreResult<()> {
    let dir = match override_path {
        Some(path) => path,
        None => default_data_dir()?,
    };
    *DATA_DIR
        .lock()
        .map_err(|_| StoreError::Other("data directory lock poisoned".into()))? = Some(dir);
    Ok(())
}

pub fn data_dir() -> StoreResult<PathBuf> {
    if let Some(dir) = DATA_DIR
        .lock()
        .map_err(|_| StoreError::Other("data directory lock poisoned".into()))?
        .clone()
    {
        return Ok(dir);
    }
    default_data_dir()
}

pub fn db_path() -> StoreResult<PathBuf> {
    Ok(data_dir()?.join("config.db"))
}

pub fn master_key_path() -> StoreResult<PathBuf> {
    Ok(data_dir()?.join("master.key"))
}

pub fn ensure_data_dir() -> StoreResult<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

pub fn ensure_parent(path: &Path) -> StoreResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn default_data_dir() -> StoreResult<PathBuf> {
    Ok(exe_dir()?.join("data"))
}

fn exe_dir() -> StoreResult<PathBuf> {
    std::env::current_exe()
        .map_err(StoreError::Io)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| StoreError::Other("could not determine executable directory".into()))
}

#[cfg(test)]
pub(crate) fn reset_data_dir_for_test() {
    *DATA_DIR.lock().expect("data directory lock poisoned") = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_data_dir_uses_override() {
        reset_data_dir_for_test();
        let temp = tempfile::tempdir().unwrap();
        init_data_dir(Some(temp.path().to_path_buf())).unwrap();
        assert_eq!(data_dir().unwrap(), temp.path());
        assert_eq!(db_path().unwrap(), temp.path().join("config.db"));
        reset_data_dir_for_test();
    }

    #[test]
    fn default_data_dir_is_next_to_executable() {
        reset_data_dir_for_test();
        let dir = default_data_dir().unwrap();
        assert_eq!(dir, exe_dir().unwrap().join("data"));
    }
}
