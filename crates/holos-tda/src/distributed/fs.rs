use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::model::DistributedInterfaceError;

pub(super) fn read_bounded(
    path: &Path,
    maximum: usize,
) -> Result<Vec<u8>, DistributedInterfaceError> {
    let file = File::open(path).map_err(|error| {
        DistributedInterfaceError::new(format!("read {}: {error}", path.display()))
    })?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| DistributedInterfaceError::new(format!("read object: {error}")))?;
    if bytes.len() > maximum {
        return Err(DistributedInterfaceError::new(
            "stored artifact exceeds the byte limit",
        ));
    }
    Ok(bytes)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), DistributedInterfaceError> {
    for nonce in 0..100u32 {
        let Some((temporary, mut file)) = reserve_temporary(path, nonce)? else {
            continue;
        };
        write_temporary(&mut file, bytes)?;
        if publish_temporary(&temporary, path, bytes)? {
            return Ok(());
        }
    }
    Err(DistributedInterfaceError::new(
        "cannot reserve an atomic temporary path",
    ))
}

pub(super) fn reserve_temporary(
    path: &Path,
    nonce: u32,
) -> Result<Option<(PathBuf, File)>, DistributedInterfaceError> {
    let temporary = temporary_path(path, nonce);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
    {
        Ok(file) => Ok(Some((temporary, file))),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
        Err(error) => Err(DistributedInterfaceError::new(format!(
            "create {}: {error}",
            temporary.display()
        ))),
    }
}

pub(super) fn write_temporary(
    file: &mut File,
    bytes: &[u8],
) -> Result<(), DistributedInterfaceError> {
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| DistributedInterfaceError::new(format!("write object: {error}")))
}

pub(super) fn publish_temporary(
    temporary: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<bool, DistributedInterfaceError> {
    match fs::rename(temporary, path) {
        Ok(()) => {
            sync_parent(path)?;
            Ok(true)
        }
        Err(_) if path.is_file() => check_existing_destination(temporary, path, bytes),
        Err(error) => {
            let _ = fs::remove_file(temporary);
            Err(DistributedInterfaceError::new(format!(
                "publish {}: {error}",
                path.display()
            )))
        }
    }
}

pub(super) fn check_existing_destination(
    temporary: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<bool, DistributedInterfaceError> {
    let _ = fs::remove_file(temporary);
    if read_bounded(path, bytes.len())? == bytes {
        return Ok(true);
    }
    Err(DistributedInterfaceError::new(
        "atomic destination contains different bytes",
    ))
}

pub(super) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), DistributedInterfaceError> {
    for nonce in 0..100u32 {
        let temporary = temporary_path(path, nonce);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(DistributedInterfaceError::new(format!(
                    "create progress: {error}"
                )));
            }
        };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| DistributedInterfaceError::new(format!("write progress: {error}")))?;
        fs::rename(&temporary, path).map_err(|error| {
            let _ = fs::remove_file(&temporary);
            DistributedInterfaceError::new(format!("publish progress: {error}"))
        })?;
        sync_parent(path)?;
        return Ok(());
    }
    Err(DistributedInterfaceError::new(
        "cannot reserve a progress temporary path",
    ))
}

pub(super) fn temporary_path(path: &Path, nonce: u32) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.{}.{nonce}.tmp", std::process::id()))
}

pub(super) fn sync_parent(path: &Path) -> Result<(), DistributedInterfaceError> {
    let parent = path
        .parent()
        .ok_or_else(|| DistributedInterfaceError::new("durable path has no parent"))?;
    sync_directory(parent)
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<(), DistributedInterfaceError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            DistributedInterfaceError::new(format!(
                "synchronize directory {}: {error}",
                path.display()
            ))
        })
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path) -> Result<(), DistributedInterfaceError> {
    Ok(())
}
