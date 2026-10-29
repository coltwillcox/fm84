//! The mounts the drive strip offers, and how full each is.

use super::*;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: PathBuf,
    pub label: String,
    pub kind: MountKind,
}

/// Home, listed first after root because it is where people actually go.
fn home_mount() -> Option<Mount> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(Mount {
        path: PathBuf::from(home),
        label: "~".to_string(),
        kind: MountKind::Home,
    })
}

/// Mount points worth offering, root first, then home, then the rest by path.
#[cfg(target_os = "linux")]
pub fn list_mounts() -> Vec<Mount> {
    let mut mounts = vec![Mount {
        path: PathBuf::from("/"),
        label: "/".to_string(),
        kind: MountKind::Disk,
    }];
    mounts.extend(home_mount());
    // As bytes: a mount point is any name at all, and one that is not UTF-8
    // would otherwise lose every mount in the table.
    if let Ok(table) = fs::read("/proc/mounts") {
        mounts.extend(parse_proc_mounts(&table));
    }
    mounts
}

/// Pick the interesting lines out of /proc/mounts. Filesystem type alone is not
/// enough to tell machinery from media: what separates them is being backed by
/// a real device, or being a fuse mount somewhere the user chose.
#[cfg(target_os = "linux")]
fn parse_proc_mounts(table: &[u8]) -> Vec<Mount> {
    use std::os::unix::ffi::OsStringExt;
    let mut found: Vec<Mount> = Vec::new();

    for line in table.split(|&byte| byte == b'\n') {
        let mut fields = line.split(u8::is_ascii_whitespace).filter(|field| !field.is_empty());
        let (Some(source), Some(target), Some(fstype)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        let (source, fstype) = (String::from_utf8_lossy(source), String::from_utf8_lossy(fstype));
        let path = PathBuf::from(OsString::from_vec(unescape_octal(target)));

        // Root is listed separately. The rest of these trees are machinery:
        // credentials, portals and gvfs under /run, and fusectl under /sys,
        // which a filesystem-type check alone would let through. Compared a
        // component at a time, so /development or /system is not among them.
        const MACHINERY: [&str; 4] = ["/run", "/sys", "/proc", "/dev"];
        // One branch of /run is not machinery: udisks2 mounts removable media
        // under /run/media, which is where a stick plugged into a desktop
        // actually turns up - inside it, not the directory itself.
        const REMOVABLE_MEDIA: &str = "/run/media";
        let removable = path.starts_with(REMOVABLE_MEDIA) && path != Path::new(REMOVABLE_MEDIA);
        let machinery = MACHINERY.iter().any(|prefix| path.starts_with(prefix)) && !removable;
        if path == Path::new("/") || machinery {
            continue;
        }

        let is_device = source.starts_with("/dev/");
        let is_fuse = fstype.starts_with("fuse");
        if !is_device && !is_fuse {
            continue;
        }

        if found.iter().any(|mount| mount.path == path) {
            continue;
        }
        let label = path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
        found.push(Mount {
            label,
            kind: classify_mount(&source, &fstype, is_fuse),
            path,
        });
    }

    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

/// A field of /proc/mounts as the name it stands for. The kernel writes
/// space, tab, newline and backslash as `\\` and three octal digits, which
/// would otherwise split the line or be read as the escapes themselves.
#[cfg(target_os = "linux")]
fn unescape_octal(field: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(field.len());
    let mut index = 0;
    while index < field.len() {
        let digits = field.get(index + 1..index + 4).filter(|digits| digits.iter().all(|digit| (b'0'..=b'7').contains(digit)));
        match (field[index], digits) {
            (b'\\', Some(digits)) => {
                out.push(digits.iter().fold(0u8, |byte, digit| byte.wrapping_mul(8).wrapping_add(digit - b'0')));
                index += 4;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    out
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
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) && head.bytes().last().is_some_and(|byte| byte.is_ascii_digit()) {
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
/dev/sdb1 /run/media/colt/ravage ntfs3 rw,nosuid 0 0
/dev/sdc1 /media/stick vfat rw,nosuid 0 0";

    #[test]
    fn keeps_media_and_drops_machinery() {
        let mounts = parse_proc_mounts(TABLE.as_bytes());
        let paths: Vec<String> = mounts.iter().map(|m| m.path.display().to_string()).collect();

        assert_eq!(
            paths,
            ["/boot", "/media/My Backup", "/media/stick", "/mnt/grimlock", "/mnt/laserbeak", "/mnt/pcloud", "/run/media/colt/AUDIO", "/run/media/colt/ravage", "/tmp",],
            "should keep device-backed and user fuse mounts, sorted"
        );

        // The rest of /run is still machinery: the portal and gvfs are fuse
        // mounts, which the check below this would otherwise wave through.
        assert!(!paths.iter().any(|path| path.starts_with("/run/user")));

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

    /// Every escape the kernel writes is read back, a name need not be
    /// UTF-8, and the machinery is matched by whole directory names.
    #[test]
    fn escapes_and_lookalike_names_are_read_as_they_are() {
        use std::os::unix::ffi::OsStrExt;
        let table = b"/dev/sdd1 /mnt/a\\134b\\012c ext4 rw 0 0\n\
/dev/sde1 /development ext4 rw 0 0\n\
/dev/sdf1 /system ext4 rw 0 0\n\
/dev/sdg1 /mnt/caf\xe9 ext4 rw 0 0\n\
/dev/sdh1 /run/media ext4 rw 0 0\n";
        let mounts = parse_proc_mounts(table);
        let paths: Vec<&[u8]> = mounts.iter().map(|m| m.path.as_os_str().as_bytes()).collect();
        assert_eq!(paths, [&b"/development"[..], b"/mnt/a\\b\nc", b"/mnt/caf\xe9", b"/system"]);
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
    let mut mounts = vec![Mount {
        path: PathBuf::from("/"),
        label: "/".to_string(),
        kind: MountKind::Disk,
    }];
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
        mounts.push(Mount {
            path: PathBuf::from(&root),
            label: root[..2].to_string(),
            kind,
        });
    }
    mounts
}

#[cfg(not(any(target_os = "linux", target_vendor = "apple", windows)))]
pub fn list_mounts() -> Vec<Mount> {
    home_mount().into_iter().collect()
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
        fn GetDiskFreeSpaceExW(directory: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let (mut free_to_caller, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: wide is NUL-terminated and the three outputs are only read back
    // after the call reports success.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_to_caller, &mut total, &mut total_free) != 0 };
    if !ok {
        return None;
    }
    Some((total.saturating_sub(total_free), total))
}

#[cfg(not(any(unix, windows)))]
pub fn disk_usage(_path: &Path) -> Option<(u64, u64)> {
    None
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
