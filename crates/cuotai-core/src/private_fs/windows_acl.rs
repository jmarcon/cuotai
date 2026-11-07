//! Windows access-control checks for [`super`] (PRD 0050 §13, option A,
//! narrowed): a file or folder is *open to others* when its DACL grants a
//! broad principal (Everyone, Authenticated Users, BUILTIN\Users, Guests,
//! Anonymous, INTERACTIVE, NETWORK) any read or write right, and *writable
//! by others* when one of those grants a right that can change, add,
//! delete or re-permission it. A NULL DACL grants everything to everyone.
//!
//! The default profile ACL (the owner, SYSTEM and Administrators) is
//! private: individual users and groups other than those broad principals
//! are not judged, so existing files under `%LOCALAPPDATA%` and `%TEMP%`
//! keep working. New private folders and files get a protected DACL with
//! exactly the current user, SYSTEM and Administrators.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AclSizeInformation, CreateWellKnownSid, EqualSid, GetAce, GetAclInformation,
    GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, WinAnonymousSid,
    WinAuthenticatedUserSid, WinBuiltinGuestsSid, WinBuiltinUsersSid, WinInteractiveSid,
    WinNetworkSid, WinWorldSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
    DACL_SECURITY_INFORMATION, INHERIT_ONLY_ACE, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
    WELL_KNOWN_SID_TYPE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// What the DACL of one path allows to principals other than its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exposure {
    /// No broad principal has any read or write right.
    Private,
    /// A broad principal may read (but not change) it.
    OpenToOthers,
    /// A broad principal may write, append, delete, re-permission or take
    /// ownership of it (or the DACL is NULL).
    WritableByOthers,
    /// The security descriptor could not be read.
    Unknown,
}

impl Exposure {
    /// The stable word of `privacy::inspect`'s `acl` field.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Exposure::Private => "private",
            Exposure::OpenToOthers => "open_to_others",
            Exposure::WritableByOthers => "writable_by_others",
            Exposure::Unknown => "unknown",
        }
    }
}

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

// Generic and standard rights, and the file/folder rights that share bits
// (FILE_WRITE_DATA = FILE_ADD_FILE, FILE_APPEND_DATA = FILE_ADD_SUBDIRECTORY).
const GENERIC_ALL: u32 = 0x1000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const GENERIC_READ: u32 = 0x8000_0000;
const DELETE: u32 = 0x0001_0000;
const WRITE_DAC: u32 = 0x0004_0000;
const WRITE_OWNER: u32 = 0x0008_0000;
const FILE_READ_DATA: u32 = 0x0001;
const FILE_WRITE_DATA: u32 = 0x0002;
const FILE_APPEND_DATA: u32 = 0x0004;
const FILE_DELETE_CHILD: u32 = 0x0040;

/// Rights that let another principal replace, add or remove content, or
/// change who may.
pub(crate) const WRITE_RIGHTS: u32 = GENERIC_ALL
    | GENERIC_WRITE
    | DELETE
    | WRITE_DAC
    | WRITE_OWNER
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_DELETE_CHILD;
/// Rights that let another principal read content (or list a folder).
const READ_RIGHTS: u32 = GENERIC_ALL | GENERIC_READ | FILE_READ_DATA;

/// The broad principals whose grants make a path not private.
const BROAD: [WELL_KNOWN_SID_TYPE; 7] = [
    WinWorldSid,
    WinAuthenticatedUserSid,
    WinBuiltinUsersSid,
    WinBuiltinGuestsSid,
    WinAnonymousSid,
    WinInteractiveSid,
    WinNetworkSid,
];

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// A well-known SID in its own buffer.
pub(crate) fn well_known_sid(kind: WELL_KNOWN_SID_TYPE) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut size = SECURITY_MAX_SID_SIZE;
    // SAFETY: `buffer` holds `size` writable bytes; a null domain SID is
    // allowed for these machine-independent SIDs.
    let ok = unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (ok != 0).then(|| {
        buffer.truncate(size as usize);
        buffer
    })
}

/// Frees a `LocalAlloc`ated pointer when dropped.
struct Local(*mut core::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Win32 call that documents
            // `LocalFree` as its release.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// Classify the DACL of `path` (the object itself, following no link
/// differently from `CreateFile`).
pub(crate) fn exposure(path: &Path) -> Exposure {
    let name = wide(path);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `name` is NUL-terminated; the out pointers are valid; the
    // descriptor is released by `Local`, and `dacl` points into it.
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    let _descriptor = Local(descriptor);
    if status != ERROR_SUCCESS {
        return Exposure::Unknown;
    }
    if dacl.is_null() {
        return Exposure::WritableByOthers;
    }
    let broad: Vec<Vec<u8>> = BROAD.iter().filter_map(|k| well_known_sid(*k)).collect();
    if broad.len() != BROAD.len() {
        return Exposure::Unknown;
    }
    // SAFETY: `dacl` is a valid ACL inside the live descriptor.
    match unsafe { classify(dacl, &broad) } {
        Some(exposure) => exposure,
        None => Exposure::Unknown,
    }
}

/// Walk the allow entries of `dacl` that apply to the object itself.
///
/// # Safety
/// `dacl` must point to a valid ACL that outlives the call.
unsafe fn classify(dacl: *const ACL, broad: &[Vec<u8>]) -> Option<Exposure> {
    let mut info = ACL_SIZE_INFORMATION::default();
    // SAFETY: `info` is the structure `AclSizeInformation` fills.
    let ok = unsafe {
        GetAclInformation(
            dacl,
            (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    };
    if ok == 0 {
        return None;
    }
    let mut result = Exposure::Private;
    for index in 0..info.AceCount {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `index` is below the ACL's entry count.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
            return None;
        }
        // SAFETY: every ACE starts with an `ACE_HEADER`.
        let header = unsafe { *(ace as *const ACE_HEADER) };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE
            || u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0
        {
            continue;
        }
        let allowed = ace as *const ACCESS_ALLOWED_ACE;
        // SAFETY: an ACCESS_ALLOWED_ACE has its mask after the header and
        // its SID starting at `SidStart`.
        let (mask, sid) = unsafe {
            (
                (*allowed).Mask,
                std::ptr::addr_of!((*allowed).SidStart) as PSID,
            )
        };
        let is_broad = broad
            .iter()
            // SAFETY: both pointers are valid SIDs.
            .any(|known| unsafe { EqualSid(sid, known.as_ptr() as PSID) } != 0);
        if !is_broad {
            continue;
        }
        if mask & WRITE_RIGHTS != 0 {
            return Some(Exposure::WritableByOthers);
        }
        if mask & READ_RIGHTS != 0 {
            result = Exposure::OpenToOthers;
        }
    }
    Some(result)
}

/// The current process user's SID as an SDDL string (`S-1-5-21-…`).
fn current_user_sid() -> Option<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle of the current process is always valid.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    let mut size = 0u32;
    // SAFETY: a size query with a null buffer.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size) };
    let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
    // SAFETY: `buffer` holds at least `size` bytes, 8-aligned.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    // SAFETY: `token` was opened above.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return None;
    }
    // SAFETY: the buffer holds a TOKEN_USER filled by the call above.
    let sid = unsafe { (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid };
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is valid while `buffer` lives; `text` is released.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return None;
    }
    let _text = Local(text.cast());
    // SAFETY: `text` is a NUL-terminated UTF-16 string.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `len` units precede the terminator.
    let units = unsafe { std::slice::from_raw_parts(text, len) };
    Some(String::from_utf16_lossy(units))
}

/// Replace the DACL of `path` by a protected one granting full control to
/// the current user, SYSTEM and Administrators only (inherited by new
/// children of a folder).
pub(crate) fn make_private(path: &Path, is_dir: bool) -> std::io::Result<()> {
    let user = current_user_sid().ok_or_else(|| std::io::Error::other("user SID"))?;
    let flags = if is_dir { "OICI" } else { "" };
    let sddl = format!("D:P(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)(A;{flags};FA;;;{user})");
    let sddl: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; the descriptor is released.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let _descriptor = Local(descriptor);
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `descriptor` is valid; the DACL points into it.
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
        == 0
        || present == 0
        || dacl.is_null()
    {
        return Err(std::io::Error::other("no DACL"));
    }
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and `dacl` is a valid ACL.
    let status = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(std::io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::Authorization::{
        SetEntriesInAclW, EXPLICIT_ACCESS_W, GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, TRUSTEE_IS_SID,
        TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
    };
    use windows_sys::Win32::Security::NO_INHERITANCE;

    /// Add an allow entry for Everyone with `rights` to the DACL of `path`.
    fn grant_everyone(path: &Path, rights: u32) {
        let name = wide(path);
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
        let _descriptor = Local(descriptor);
        let mut world = well_known_sid(WinWorldSid).unwrap();
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: rights,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: world.as_mut_ptr().cast(),
            },
        };
        let mut merged: *mut ACL = std::ptr::null_mut();
        let status = unsafe { SetEntriesInAclW(1, &entry, dacl, &mut merged) };
        assert_eq!(status, ERROR_SUCCESS);
        let _merged = Local(merged.cast());
        let status = unsafe {
            SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                merged,
                std::ptr::null(),
            )
        };
        assert_eq!(status, ERROR_SUCCESS);
    }

    #[test]
    fn default_temporary_files_and_folders_are_private() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plain.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(exposure(dir.path()), Exposure::Private);
        assert_eq!(exposure(&file), Exposure::Private);
    }

    // security-invariant: SEC-WINDOWS-ACL (PRD 0050 §13, RF7.5)
    #[test]
    fn an_everyone_write_entry_makes_a_file_and_a_folder_writable_by_others() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("shared.txt");
        std::fs::write(&file, b"x").unwrap();
        grant_everyone(&file, FILE_WRITE_DATA);
        assert_eq!(exposure(&file), Exposure::WritableByOthers);
        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        grant_everyone(&folder, FILE_APPEND_DATA);
        assert_eq!(exposure(&folder), Exposure::WritableByOthers);
    }

    #[test]
    fn an_everyone_read_entry_makes_a_file_open_but_not_writable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("readable.txt");
        std::fs::write(&file, b"x").unwrap();
        grant_everyone(&file, GENERIC_READ);
        assert_eq!(exposure(&file), Exposure::OpenToOthers);
    }

    #[test]
    fn make_private_removes_a_broad_grant() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("token");
        std::fs::write(&file, b"x").unwrap();
        grant_everyone(&file, GENERIC_ALL);
        assert_eq!(exposure(&file), Exposure::WritableByOthers);
        make_private(&file, false).unwrap();
        assert_eq!(exposure(&file), Exposure::Private);
        // Still readable and writable by its owner.
        std::fs::write(&file, b"y").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"y");
    }

    #[test]
    fn a_missing_path_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(exposure(&dir.path().join("absent")), Exposure::Unknown);
    }
}
