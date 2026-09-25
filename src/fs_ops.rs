use crate::app::Item;
use crate::constants::COPY_CHUNK;
use crate::utils::format_size;
use chrono::Local;
use std::env;
use std::fs::{self, File, create_dir, read_dir, remove_dir_all, remove_file, rename};
use std::io::{self, Error, Read};
use std::path::{Path, PathBuf};

/// Permission bits as `ls -l` writes them. The mode comes from the metadata the
/// listing already holds, so this costs no extra syscall.
#[cfg(unix)]
fn format_attributes(metadata: &fs::Metadata, is_dir: bool, is_symlink: bool) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    let kind = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };

    // Each triplet's execute slot doubles as the setuid/setgid/sticky flag,
    // which would otherwise be invisible.
    let triplet = |shift: u32, special: u32, on: char| {
        let bits = mode >> shift;
        let execute = bits & 0o1 != 0;
        [
            if bits & 0o4 != 0 { 'r' } else { '-' },
            if bits & 0o2 != 0 { 'w' } else { '-' },
            match (mode & special != 0, execute) {
                (true, true) => on,
                (true, false) => on.to_ascii_uppercase(),
                (false, true) => 'x',
                (false, false) => '-',
            },
        ]
    };

    let mut text = String::with_capacity(10);
    text.push(kind);
    text.extend(triplet(6, 0o4000, 's'));
    text.extend(triplet(3, 0o2000, 's'));
    text.extend(triplet(0, 0o1000, 't'));
    text
}

#[cfg(windows)]
fn format_attributes(metadata: &fs::Metadata, _is_dir: bool, is_symlink: bool) -> String {
    use std::os::windows::fs::MetadataExt;

    const READONLY: u32 = 0x0000_0001;
    const HIDDEN: u32 = 0x0000_0002;
    const SYSTEM: u32 = 0x0000_0004;
    const DIRECTORY: u32 = 0x0000_0010;
    const ARCHIVE: u32 = 0x0000_0020;

    let flags = metadata.file_attributes();
    let flag = |bit: u32, on: char| if flags & bit != 0 { on } else { '-' };
    let kind = if is_symlink { 'l' } else { flag(DIRECTORY, 'd') };

    [kind, flag(ARCHIVE, 'a'), flag(READONLY, 'r'), flag(HIDDEN, 'h'), flag(SYSTEM, 's')]
        .iter()
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn format_attributes(_metadata: &fs::Metadata, _is_dir: bool, _is_symlink: bool) -> String {
    String::new()
}

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
            attributes: String::new(),
        });
    }

    // One metadata() call per entry (one stat syscall), two for a symlink
    for entry in &entries {
        let entry_path = entry.path();
        // file_type() comes from readdir's d_type - no syscall on Linux.
        let is_symlink = entry.file_type().map(|file_type| file_type.is_symlink()).unwrap_or(false);
        // DirEntry::metadata() describes the link itself, which would list a link
        // to a directory as a file: sorted among the files, sized in bytes and
        // impossible to enter. Follow it, and fall back to the link when the
        // target is missing so a broken link still lists as an ordinary entry.
        let metadata = if is_symlink {
            fs::metadata(&entry_path).or_else(|_| entry.metadata()).ok()
        } else {
            entry.metadata().ok()
        };
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

        let attributes = metadata
            .as_ref()
            .map(|metadata| format_attributes(metadata, is_dir, is_symlink))
            .unwrap_or_default();

        children.push(Item {
            name_full,
            name,
            extension,
            is_dir,
            size,
            size_bytes,
            modified,
            attributes,
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

/// What kind of place a mount is, so the UI can pick an icon for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
    Home,
    Disk,
    /// Detected on Linux and Windows; other platforms have no cheap way to tell.
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    Removable,
    Network,
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    Optical,
}

/// Somewhere a panel can jump to: a filesystem mount, or home.
#[derive(Debug, Clone)]
pub struct Mount {
    pub path: PathBuf,
    pub label: String,
    pub kind: MountKind,
}

/// Home, listed first after root because it is where people actually go.
fn home_mount() -> Option<Mount> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(Mount { path: PathBuf::from(home), label: "~".to_string(), kind: MountKind::Home })
}

/// Mount points worth offering, root first, then home, then the rest by path.
#[cfg(target_os = "linux")]
pub fn list_mounts() -> Vec<Mount> {
    let mut mounts = vec![Mount { path: PathBuf::from("/"), label: "/".to_string(), kind: MountKind::Disk }];
    mounts.extend(home_mount());
    if let Ok(table) = fs::read_to_string("/proc/mounts") {
        mounts.extend(parse_proc_mounts(&table));
    }
    mounts
}

/// Pick the interesting lines out of /proc/mounts. Filesystem type alone is not
/// enough to tell machinery from media: what separates them is being backed by
/// a real device, or being a fuse mount somewhere the user chose.
#[cfg(target_os = "linux")]
fn parse_proc_mounts(table: &str) -> Vec<Mount> {
    let mut found: Vec<Mount> = Vec::new();

    for line in table.lines() {
        let mut fields = line.split_whitespace();
        let (Some(source), Some(target), Some(fstype)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        // /proc/mounts escapes spaces and tabs as octal.
        let target = target.replace("\\040", " ").replace("\\011", "\t");

        // Root is listed separately. The rest of these trees are machinery:
        // credentials, portals and gvfs under /run, and fusectl under /sys,
        // which a filesystem-type check alone would let through.
        const MACHINERY: [&str; 4] = ["/run", "/sys", "/proc", "/dev"];
        if target == "/" || MACHINERY.iter().any(|prefix| target.starts_with(prefix)) {
            continue;
        }

        let is_device = source.starts_with("/dev/");
        let is_fuse = fstype.starts_with("fuse");
        if !is_device && !is_fuse {
            continue;
        }

        let path = PathBuf::from(&target);
        if found.iter().any(|mount| mount.path == path) {
            continue;
        }
        let label = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| target.clone());
        found.push(Mount { path, label, kind: classify_mount(source, fstype, is_fuse) });
    }

    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// Optical media announce themselves by filesystem type; removable media by a
/// flag the kernel exposes for the block device underneath.
#[cfg(target_os = "linux")]
fn classify_mount(source: &str, fstype: &str, is_fuse: bool) -> MountKind {
    if matches!(fstype, "iso9660" | "udf") {
        return MountKind::Optical;
    }
    if is_fuse {
        return MountKind::Network;
    }
    if is_removable(source) {
        return MountKind::Removable;
    }
    MountKind::Disk
}

#[cfg(target_os = "linux")]
fn is_removable(source: &str) -> bool {
    let Some(device) = source.strip_prefix("/dev/") else {
        return false;
    };
    // A whole device has its own /sys/block entry; a partition has to be
    // reduced to the device it sits on.
    for candidate in [device, base_device(device)] {
        if let Ok(flag) = fs::read_to_string(format!("/sys/block/{candidate}/removable")) {
            return flag.trim() == "1";
        }
    }
    false
}

/// The whole-disk name behind a partition. NVMe and SD cards separate the
/// partition number with a 'p'; everything else just appends it.
#[cfg(target_os = "linux")]
fn base_device(name: &str) -> &str {
    if let Some(index) = name.rfind('p') {
        let (head, tail) = name.split_at(index);
        let digits = &tail[1..];
        if !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && head.bytes().last().is_some_and(|byte| byte.is_ascii_digit())
        {
            return head;
        }
    }
    name.trim_end_matches(|c: char| c.is_ascii_digit())
}

#[cfg(all(test, target_os = "linux"))]
mod mount_tests {
    use super::{MountKind, parse_proc_mounts};

    /// Verbatim lines from a real /proc/mounts, machinery and media mixed.
    const TABLE: &str = "\
devtmpfs /dev devtmpfs rw,nosuid 0 0
run /run tmpfs rw,nosuid 0 0
/dev/nvme0n1p4 / ext4 rw,relatime 0 0
tmpfs /dev/shm tmpfs rw,nosuid 0 0
/dev/nvme0n1p3 /tmp ext4 rw,relatime 0 0
/dev/nvme0n1p5 /mnt/laserbeak ext4 rw,relatime 0 0
/dev/sda1 /mnt/grimlock ext4 rw,relatime 0 0
/dev/nvme0n1p2 /boot vfat rw,relatime 0 0
none /run/credentials/getty@tty1.service tmpfs ro 0 0
tmpfs /run/user/1000 tmpfs rw,nosuid 0 0
pcloud: /mnt/pcloud fuse.rclone rw,nosuid 0 0
portal /run/user/1000/doc fuse.portal rw,nosuid 0 0
gvfsd-fuse /run/user/1000/gvfs fuse.gvfsd-fuse rw,nosuid 0 0
fusectl /sys/fs/fuse/connections fusectl rw,nosuid 0 0
/dev/sdb1 /media/My\\040Backup ext4 rw,relatime 0 0
/dev/sr0 /run/media/colt/AUDIO iso9660 ro,nosuid 0 0
/dev/sdc1 /media/stick vfat rw,nosuid 0 0";

    #[test]
    fn keeps_media_and_drops_machinery() {
        let mounts = parse_proc_mounts(TABLE);
        let paths: Vec<String> = mounts.iter().map(|m| m.path.display().to_string()).collect();

        assert_eq!(
            paths,
            ["/boot", "/media/My Backup", "/media/stick", "/mnt/grimlock", "/mnt/laserbeak", "/mnt/pcloud", "/tmp"],
            "should keep device-backed and user fuse mounts, sorted"
        );

        // Root is added by the caller, not here.
        assert!(!paths.iter().any(|p| p == "/"));
        // fusectl matches a bare "fuse" prefix but is machinery, not media.
        assert!(!paths.iter().any(|p| p.starts_with("/sys")));
        // A fuse mount the user chose is network-ish; a partition is a disk.
        let pcloud = mounts.iter().find(|m| m.label == "pcloud").unwrap();
        assert_eq!(pcloud.kind, MountKind::Network);
        let boot = mounts.iter().find(|m| m.label == "boot").unwrap();
        assert_eq!(boot.kind, MountKind::Disk);
        // Octal escapes are decoded for display.
        assert!(mounts.iter().any(|m| m.label == "My Backup"));
    }

    #[test]
    fn optical_filesystems_are_recognised() {
        assert_eq!(super::classify_mount("/dev/sr0", "iso9660", false), MountKind::Optical);
        assert_eq!(super::classify_mount("/dev/sr0", "udf", false), MountKind::Optical);
        assert_eq!(super::classify_mount("pcloud:", "fuse.rclone", true), MountKind::Network);
    }

    #[test]
    fn partitions_reduce_to_their_whole_device() {
        assert_eq!(super::base_device("sda1"), "sda");
        assert_eq!(super::base_device("sda"), "sda");
        assert_eq!(super::base_device("nvme0n1p5"), "nvme0n1");
        assert_eq!(super::base_device("mmcblk0p1"), "mmcblk0");
        // 'p' only splits when a digit precedes it, so sdp1 is not sd.
        assert_eq!(super::base_device("sdp1"), "sdp");
    }
}

/// Every mounted volume on macOS shows up under /Volumes.
#[cfg(target_vendor = "apple")]
pub fn list_mounts() -> Vec<Mount> {
    let mut mounts = vec![Mount { path: PathBuf::from("/"), label: "/".to_string(), kind: MountKind::Disk }];
    mounts.extend(home_mount());

    let Ok(entries) = read_dir("/Volumes") else {
        return mounts;
    };
    let mut found: Vec<Mount> = entries
        .flatten()
        .map(|entry| Mount {
            label: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path(),
            kind: MountKind::Disk,
        })
        .collect();

    found.sort_by(|a, b| a.path.cmp(&b.path));
    mounts.extend(found);
    mounts
}

#[cfg(windows)]
pub fn list_mounts() -> Vec<Mount> {
    // Declared here rather than pulling in windows-sys, as elsewhere.
    unsafe extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_CDROM: u32 = 5;

    // SAFETY: no arguments, and the bitmask is just read back.
    let mask = unsafe { GetLogicalDrives() };
    let mut mounts = Vec::new();
    mounts.extend(home_mount());

    for letter in 0..26u32 {
        if mask & (1 << letter) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + letter as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: wide is NUL-terminated and outlives the call.
        let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
            DRIVE_REMOVABLE => MountKind::Removable,
            DRIVE_REMOTE => MountKind::Network,
            DRIVE_CDROM => MountKind::Optical,
            DRIVE_FIXED => MountKind::Disk,
            _ => continue,
        };
        mounts.push(Mount { path: PathBuf::from(&root), label: root[..2].to_string(), kind });
    }
    mounts
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
pub fn list_mounts() -> Vec<Mount> {
    home_mount().into_iter().collect()
}

/// Walk up from `path` until a directory that still exists is found. A panel's
/// directory can be removed underneath it - and so can several of its parents,
/// if something deleted a whole tree - so this climbs until it lands somewhere
/// listable. None only if even the root is unreachable.
pub fn nearest_existing_dir(path: &Path) -> Option<PathBuf> {
    let mut candidate = Some(path);
    while let Some(dir) = candidate {
        if dir.is_dir() {
            return Some(dir.to_path_buf());
        }
        candidate = dir.parent();
    }
    None
}

/// Bytes used and total for the filesystem holding `path`, as `df` counts them.
/// None when the platform call fails, so a dead network mount shows nothing
/// rather than an error.
#[cfg(all(unix, not(target_vendor = "apple")))]
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: c_path is a valid NUL-terminated string and stats is only read
    // back after statvfs reports success.
    let stats = unsafe {
        let mut stats = std::mem::zeroed::<libc::statvfs>();
        if libc::statvfs(c_path.as_ptr(), &mut stats) != 0 {
            return None;
        }
        stats
    };

    let block = stats.f_frsize as u64;
    let total = (stats.f_blocks as u64).checked_mul(block)?;
    let free = (stats.f_bfree as u64).checked_mul(block)?;
    Some((total.saturating_sub(free), total))
}

/// Apple platforms declare statvfs block counts as a 32-bit fsblkcnt_t, so it
/// truncates past about 16 TiB. Their native statfs carries the counts as u64.
#[cfg(target_vendor = "apple")]
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: c_path is a valid NUL-terminated string and stats is only read
    // back after statfs reports success.
    let stats = unsafe {
        let mut stats = std::mem::zeroed::<libc::statfs>();
        if libc::statfs(c_path.as_ptr(), &mut stats) != 0 {
            return None;
        }
        stats
    };

    let block = stats.f_bsize as u64;
    let total = stats.f_blocks.checked_mul(block)?;
    let free = stats.f_bfree.checked_mul(block)?;
    Some((total.saturating_sub(free), total))
}

#[cfg(windows)]
pub fn disk_usage(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;

    // Declared here rather than pulling in windows-sys for one call.
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_to_caller: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut free_to_caller, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: wide is NUL-terminated and the three outputs are only read back
    // after the call reports success.
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_to_caller, &mut total, &mut total_free) != 0
    };
    if !ok {
        return None;
    }
    Some((total.saturating_sub(total_free), total))
}

#[cfg(not(any(unix, windows)))]
pub fn disk_usage(_path: &Path) -> Option<(u64, u64)> {
    None
}

pub fn get_current_dir() -> Result<PathBuf, Error> {
    env::current_dir()
}

pub fn rename_path(original_path: PathBuf, new_path: PathBuf) -> Result<(), Error> {
    rename(original_path, new_path)?;
    Ok(())
}

pub fn delete_path(path: PathBuf, is_dir: bool) -> Result<(), Error> {
    // A symlink is removed as a link, never followed - including one pointing at
    // a directory, which reaches here with is_dir set because the panel treats it
    // as one. remove_dir_all on a link would be wrong.
    let is_symlink = path
        .symlink_metadata()
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);

    if is_dir && !is_symlink {
        remove_dir_all(path)?;
    } else {
        remove_file(path)?;
    }
    Ok(())
}

/// True if anything occupies this path, including a dangling symlink - which
/// Path::exists() reports as absent because it follows the link.
pub fn path_exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

/// Create an empty file, refusing if anything is already there - create_new
/// fails rather than truncating, matching how create_dir refuses.
pub fn create_file(path: PathBuf) -> Result<(), Error> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    Ok(())
}

pub fn create_directory(path: PathBuf) -> Result<(), Error> {
    create_dir(path)?;
    Ok(())
}

/// What a running transfer reports as it goes.
pub enum Step<'a> {
    /// Starting this file. Bytes reported after it belong to it.
    Starting(&'a Path),
    /// Another `bytes` of the current file are written.
    Copied(u64),
}

/// How a transfer ended. Cancelled leaves everything already finished where it
/// is; only the one file that was in flight is cleaned up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transfer {
    Done,
    Cancelled,
}

/// Takes progress and answers whether to carry on. Returning false stops the
/// transfer at the next chunk or file, whichever comes first.
pub type Report<'a> = &'a mut dyn FnMut(Step<'_>) -> bool;

/// Move by renaming, which only works within one filesystem. True if it did;
/// the caller copies instead when it did not. Kept separate from move_path so
/// a caller can get the instant moves out of the way before measuring the rest.
pub fn rename_in_place(source: &Path, dest: &Path) -> bool {
    rename(source, dest).is_ok()
}

/// Bytes a transfer will move, for the progress bar to count against. A
/// directory is walked; anything unreadable counts as nothing rather than
/// failing the job, since this is only ever a denominator.
pub fn measure(items: &[(PathBuf, PathBuf, bool)]) -> u64 {
    items
        .iter()
        .map(|(source, _, is_dir)| {
            // A symlink is recreated, not followed, so it carries no bytes.
            if fs::symlink_metadata(source).map(|meta| meta.file_type().is_symlink()).unwrap_or(false) {
                0
            } else if *is_dir {
                calculate_dir_size(source).unwrap_or(0)
            } else {
                fs::metadata(source).map(|meta| meta.len()).unwrap_or(0)
            }
        })
        .sum()
}

pub fn copy_path(source: PathBuf, dest: PathBuf, is_dir: bool, report: Report<'_>) -> Result<Transfer, Error> {
    // A symlink is copied as the link itself, never as its target, matching
    // cp -r. is_dir can't decide this: it comes from DirEntry::metadata(),
    // which doesn't follow links, so a link to a directory arrives false here
    // and would otherwise be handed to copy_file_content.
    if fs::symlink_metadata(&source)?.file_type().is_symlink() {
        copy_symlink(&source, &dest)?;
        Ok(Transfer::Done)
    } else if is_dir {
        copy_dir_recursive(&source, &dest, report)
    } else {
        copy_file_content(&source, &dest, report)
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
///
/// The copy runs in COPY_CHUNK pieces so there is somewhere to report progress
/// from and somewhere to notice a cancel. Each piece is still an io::copy, over
/// a reader limited to the chunk rather than a buffer of our own, which is what
/// keeps the platform's fast path - copy_file_range on Linux.
fn copy_file_content(source: &Path, dest: &Path, report: Report<'_>) -> Result<Transfer, Error> {
    if !report(Step::Starting(source)) {
        return Ok(Transfer::Cancelled);
    }

    let mut src_file = File::open(source)?;
    let mut dst_file = File::create(dest)?;
    loop {
        let copied = io::copy(&mut (&mut src_file).take(COPY_CHUNK), &mut dst_file)?;
        if copied == 0 {
            return Ok(Transfer::Done);
        }
        if !report(Step::Copied(copied)) {
            // What is on disk is half a file that will never be finished.
            // Left alone it would sit there looking like a complete copy.
            drop(dst_file);
            let _ = remove_file(dest);
            return Ok(Transfer::Cancelled);
        }
    }
}

fn copy_dir_recursive(source: &Path, dest: &Path, report: Report<'_>) -> Result<Transfer, Error> {
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
        let outcome = if file_type.is_symlink() {
            copy_symlink(&entry_path, &dest_path)?;
            Transfer::Done
        } else if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path, &mut *report)?
        } else {
            copy_file_content(&entry_path, &dest_path, &mut *report)?
        };
        if outcome == Transfer::Cancelled {
            return Ok(Transfer::Cancelled);
        }
    }

    Ok(Transfer::Done)
}

pub fn move_path(source: PathBuf, dest: PathBuf, is_dir: bool, report: Report<'_>) -> Result<Transfer, Error> {
    // Try rename first (fast, same filesystem)
    match rename(&source, &dest) {
        Ok(_) => Ok(Transfer::Done),
        Err(e) => {
            // Check for cross-device error:
            // - EXDEV (18) on Linux/macOS/Unix
            // - ERROR_NOT_SAME_DEVICE (17) on Windows
            if matches!(e.raw_os_error(), Some(17) | Some(18)) {
                // Cross-device move: copy then delete
                if copy_path(source.clone(), dest.clone(), is_dir, report)? == Transfer::Cancelled {
                    // The copy stopped partway, so the source has to stay.
                    return Ok(Transfer::Cancelled);
                }

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
                Ok(Transfer::Done)
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

#[cfg(test)]
mod live_mount_probe {
    #[test]
    #[ignore = "machine-specific; run with --ignored to eyeball this host's mounts"]
    fn show() {
        for mount in super::list_mounts() {
            println!("  {:<24} {:<14} {:?}", mount.path.display(), mount.label, mount.kind);
        }
    }
}

#[cfg(test)]
mod transfer_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fm84-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn measure_adds_up_files_and_walks_directories() {
        let dir = scratch("measure");
        fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
        fs::create_dir(dir.join("sub")).unwrap();
        fs::write(dir.join("sub").join("b.bin"), vec![0u8; 2500]).unwrap();

        let items = vec![
            (dir.join("a.bin"), dir.join("copy-a.bin"), false),
            (dir.join("sub"), dir.join("copy-sub"), true),
        ];
        assert_eq!(measure(&items), 3500);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_copy_reports_every_file_and_every_byte() {
        let dir = scratch("report");
        let source = dir.join("src");
        fs::create_dir(&source).unwrap();
        // One file longer than a chunk, so the inner loop reports several times.
        fs::write(source.join("big.bin"), vec![7u8; COPY_CHUNK as usize * 2 + 13]).unwrap();
        fs::write(source.join("small.bin"), vec![7u8; 10]).unwrap();
        let dest = dir.join("dst");

        let mut started = Vec::new();
        let mut bytes = 0u64;
        {
            let mut report = |step: Step<'_>| {
                match step {
                    Step::Starting(path) => started.push(path.file_name().unwrap().to_string_lossy().into_owned()),
                    Step::Copied(n) => bytes += n,
                }
                true
            };
            assert_eq!(copy_path(source.clone(), dest.clone(), true, &mut report).unwrap(), Transfer::Done);
        }

        started.sort();
        assert_eq!(started, ["big.bin", "small.bin"]);
        // What the bar counts has to reach what the counting pass promised.
        assert_eq!(bytes, measure(&[(source, dest, true)]));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancelling_clears_the_half_written_file() {
        let dir = scratch("cancel");
        let source = dir.join("big.bin");
        let dest = dir.join("copy.bin");
        fs::write(&source, vec![7u8; COPY_CHUNK as usize * 3]).unwrap();

        // Stop after the first chunk, the way Esc does partway through.
        let mut chunks = 0;
        {
            let mut report = |step: Step<'_>| {
                if let Step::Copied(_) = step {
                    chunks += 1;
                }
                chunks < 1
            };
            assert_eq!(copy_path(source, dest.clone(), false, &mut report).unwrap(), Transfer::Cancelled);
        }
        // A fragment left behind would sit there looking like a finished copy.
        assert!(!dest.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refusing_the_first_file_copies_nothing() {
        let dir = scratch("refuse");
        let source = dir.join("src");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("a.bin"), vec![1u8; 10]).unwrap();
        let dest = dir.join("dst");

        let mut report = |_: Step<'_>| false;
        assert_eq!(copy_path(source, dest.clone(), true, &mut report).unwrap(), Transfer::Cancelled);
        assert!(!dest.join("a.bin").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_rename_within_one_filesystem_moves_no_bytes() {
        let dir = scratch("rename");
        let source = dir.join("a.bin");
        let dest = dir.join("b.bin");
        fs::write(&source, vec![3u8; 4096]).unwrap();

        assert!(rename_in_place(&source, &dest));
        assert!(!source.exists() && dest.exists());
        // Across filesystems it has to say so rather than pretend.
        assert!(!rename_in_place(&dest, Path::new("/proc/fm84-cannot-go-here")));
        fs::remove_dir_all(&dir).unwrap();
    }
}
