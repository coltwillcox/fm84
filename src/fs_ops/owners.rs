//! User and group names and numbers, looked up without linking a library for it.

use super::*;

/// Who owns it, by name where the system can say and by number where it cannot.
#[cfg(unix)]
pub(super) fn owner_of(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    let (uid, gid) = (metadata.uid(), metadata.gid());

    format!("{}:{}", user_name(uid), group_name(gid))
}

/// A user's name, or its number where the system has no name for it.
#[cfg(unix)]
pub fn user_name(uid: u32) -> String {
    lookup_entry(|entry: &mut libc::passwd, buffer, length, found| unsafe { libc::getpwuid_r(uid, entry, buffer, length, found) }, |entry| c_name(entry.pw_name)).unwrap_or_else(|| uid.to_string())
}

/// A group's name, or its number where the system has no name for it.
#[cfg(unix)]
pub fn group_name(gid: u32) -> String {
    lookup_entry(|entry: &mut libc::group, buffer, length, found| unsafe { libc::getgrgid_r(gid, entry, buffer, length, found) }, |entry| c_name(entry.gr_name)).unwrap_or_else(|| gid.to_string())
}

/// The user a name stands for, or a number taken as it is.
#[cfg(unix)]
pub fn user_id(name: &str) -> Option<u32> {
    if let Ok(uid) = name.parse() {
        return Some(uid);
    }
    let name = std::ffi::CString::new(name).ok()?;
    lookup_entry(|entry: &mut libc::passwd, buffer, length, found| unsafe { libc::getpwnam_r(name.as_ptr(), entry, buffer, length, found) }, |entry| Some(entry.pw_uid))
}

/// The group a name stands for, or a number taken as it is.
#[cfg(unix)]
pub fn group_id(name: &str) -> Option<u32> {
    if let Ok(gid) = name.parse() {
        return Some(gid);
    }
    let name = std::ffi::CString::new(name).ok()?;
    lookup_entry(|entry: &mut libc::group, buffer, length, found| unsafe { libc::getgrnam_r(name.as_ptr(), entry, buffer, length, found) }, |entry| Some(entry.gr_gid))
}

/// A NUL-terminated name in an entry the system filled in.
#[cfg(unix)]
fn c_name(pointer: *const libc::c_char) -> Option<String> {
    // SAFETY: the name points into the buffer lookup_entry passed, which is
    // alive while `pick` runs and holds a NUL-terminated string the call
    // wrote there.
    (!pointer.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
}

/// A user or group entry, looked up with the _r calls, which write into a
/// buffer of ours, and what `pick` takes from it. The plain getpwuid and
/// getgrgid hand back storage shared by the whole process, which is only safe
/// while one thread ever asks - and the detail lines are gathered on a thread
/// of their own now. The buffer grows for an entry too long for it, as a
/// directory service's can be.
#[cfg(unix)]
fn lookup_entry<E, T>(call: impl Fn(&mut E, *mut libc::c_char, libc::size_t, *mut *mut E) -> libc::c_int, pick: impl Fn(&E) -> Option<T>) -> Option<T> {
    let mut buffer: Vec<libc::c_char> = vec![0; 1024];
    loop {
        // SAFETY: an all-zero passwd or group is a valid value of the type -
        // null pointers and zero ids - and is only read once the call has
        // filled it in and said so by setting `found`.
        let mut entry: E = unsafe { std::mem::zeroed() };
        let mut found: *mut E = std::ptr::null_mut();
        let status = call(&mut entry, buffer.as_mut_ptr(), buffer.len(), &mut found);
        if status == libc::ERANGE && buffer.len() < 1 << 20 {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 || found.is_null() {
            return None;
        }
        return pick(&entry);
    }
}

#[cfg(not(unix))]
pub(super) fn owner_of(_metadata: &fs::Metadata) -> String {
    String::new()
}
