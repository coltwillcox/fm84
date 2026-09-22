use crate::app::Item;
use crate::utils::format_size;
use chrono::Local;
use std::env;
use std::fs::{self, File, create_dir, read_dir, remove_dir_all, remove_file, rename};
use std::io::{self, Error};
use std::path::{Path, PathBuf};

pub fn load_directory_rows(path: &Path) -> Result<Vec<Item>, Error> {
    let entries: Vec<_> = read_dir(path)?
        .filter_map(|entry| entry.ok())
        .collect();

    let has_parent = path.parent().is_some();
    let mut children = Vec::with_capacity(entries.len() + usize::from(has_parent));

    // Don't add ".." on root folder.
    if has_parent {
        children.push(Item {
            name_full: "..".to_string(),
            name: "..".to_string(),
            extension: String::new(),
            is_dir: true,
            size: String::new(),
            size_bytes: 0,
            modified: String::new(),
        });
    }

    // Build Items with a single metadata() call per entry (one stat syscall)
    for entry in &entries {
        let entry_path = entry.path();
        let metadata = entry.metadata().ok();
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let name_full = entry_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = if is_dir { name_full.clone() } else { entry_path.file_stem().and_then(|n| n.to_str()).unwrap_or("").to_string() };
        let extension = if is_dir { String::new() } else { entry_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string() };
        let size_bytes = if is_dir { 0 } else { metadata.as_ref().map(|m| m.len()).unwrap_or(0) };
        let size = if is_dir { "<DIR>".to_string() } else { format_size(size_bytes) };
        let modified = metadata.as_ref()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: chrono::DateTime<Local> = t.into();
                dt.format("%d/%m/%y %H:%M").to_string()
            })
            .unwrap_or_default();

        children.push(Item {
            name_full,
            name,
            extension,
            is_dir,
            size,
            size_bytes,
            modified,
        });
    }

    // Sort items on already-computed fields (no stat syscalls during sort)
    let sort_start = usize::from(has_parent);
    children[sort_start..].sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (true, true) => a.name_full.to_lowercase().cmp(&b.name_full.to_lowercase()),
        (false, false) => {
            a.extension.to_lowercase().cmp(&b.extension.to_lowercase()).then_with(|| {
                a.name_full.to_lowercase().cmp(&b.name_full.to_lowercase())
            })
        }
    });

    Ok(children)
}

pub fn get_current_dir() -> Result<PathBuf, Error> {
    env::current_dir()
}

pub fn rename_path(original_path: PathBuf, new_path: PathBuf) -> Result<(), Error> {
    rename(original_path, new_path)?;
    Ok(())
}

pub fn delete_path(path: PathBuf, is_dir: bool) -> Result<(), Error> {
    if is_dir {
        remove_dir_all(path)?;
    } else {
        remove_file(path)?;
    }
    Ok(())
}

pub fn create_directory(path: PathBuf) -> Result<(), Error> {
    create_dir(path)?;
    Ok(())
}

pub fn copy_path(source: PathBuf, dest: PathBuf, is_dir: bool) -> Result<(), Error> {
    // A symlink is copied as the link itself, never as its target, matching
    // cp -r. is_dir can't decide this: it comes from DirEntry::metadata(),
    // which doesn't follow links, so a link to a directory arrives false here
    // and would otherwise be handed to copy_file_content.
    if fs::symlink_metadata(&source)?.file_type().is_symlink() {
        copy_symlink(&source, &dest)
    } else if is_dir {
        copy_dir_recursive(&source, &dest)
    } else {
        copy_file_content(&source, &dest)
    }
}

/// Recreate a symlink at the destination, pointing where the original pointed.
#[cfg(unix)]
fn copy_symlink(source: &Path, dest: &Path) -> Result<(), Error> {
    std::os::unix::fs::symlink(fs::read_link(source)?, dest)
}

#[cfg(windows)]
fn copy_symlink(source: &Path, dest: &Path) -> Result<(), Error> {
    let target = fs::read_link(source)?;
    // Windows picks the call by link kind, and creating one needs Developer
    // Mode or elevation - the error surfaces to the user either way.
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(target, dest)
    } else {
        std::os::windows::fs::symlink_file(target, dest)
    }
}

/// Copy file content without trying to preserve Unix permissions.
/// This works across filesystems (e.g., ext4 to exFAT) where permission
/// preservation would fail with EPERM.
/// Uses io::copy which leverages copy_file_range (zero-copy) on Linux.
fn copy_file_content(source: &Path, dest: &Path) -> Result<(), Error> {
    let mut src_file = File::open(source)?;
    let mut dst_file = File::create(dest)?;
    io::copy(&mut src_file, &mut dst_file)?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<(), Error> {
    fs::create_dir_all(dest)?;

    for entry in read_dir(source)? {
        let entry = entry?;
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        // file_type() describes the entry itself; is_dir() would follow the
        // link, copying the target's contents - and a link pointing at an
        // ancestor recurses until the path outgrows PATH_MAX. cp -r recreates
        // the link, so do the same.
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            copy_symlink(&entry_path, &dest_path)?;
        } else if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            copy_file_content(&entry_path, &dest_path)?;
        }
    }

    Ok(())
}

pub fn move_path(source: PathBuf, dest: PathBuf, is_dir: bool) -> Result<(), Error> {
    // Try rename first (fast, same filesystem)
    match rename(&source, &dest) {
        Ok(_) => Ok(()),
        Err(e) => {
            // Check for cross-device error:
            // - EXDEV (18) on Linux/macOS/Unix
            // - ERROR_NOT_SAME_DEVICE (17) on Windows
            if matches!(e.raw_os_error(), Some(17) | Some(18)) {
                // Cross-device move: copy then delete
                copy_path(source.clone(), dest.clone(), is_dir)?;

                // Delete source - if this fails, the copy succeeded but source remains
                if let Err(del_err) = delete_path(source, is_dir) {
                    return Err(Error::new(
                        del_err.kind(),
                        format!(
                            "Move partially complete: copied to {} but failed to delete source: {}",
                            dest.display(),
                            del_err
                        ),
                    ));
                }
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

pub fn calculate_dir_size(path: &Path) -> Result<u64, Error> {
    let mut total_size = 0u64;

    for entry in read_dir(path)? {
        let entry = entry?;
        // file_type() uses readdir's d_type on Linux - no extra stat syscall
        if entry.file_type()?.is_dir() {
            // Continue on subdirectory errors, just skip that dir
            if let Ok(size) = calculate_dir_size(&entry.path()) {
                total_size += size;
            }
        } else {
            total_size += entry.metadata()?.len();
        }
    }

    Ok(total_size)
}
