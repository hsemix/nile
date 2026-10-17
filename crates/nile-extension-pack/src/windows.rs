//! Protected user-profile cache ACLs; never change process-wide DLL search paths.
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, *},
};
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn os_error() -> String {
    std::io::Error::last_os_error().to_string()
}
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0 as _);
        }
    }
}
/// Only the object's owner and SYSTEM inherit full access. Administrators retain
/// their OS privileges but are not added as ordinary cache readers/writers.
pub fn protect(path: &Path) -> Result<(), String> {
    unsafe {
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
            .encode_utf16()
            .collect();
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(os_error());
        }
        let sd = Descriptor(sd);
        let (mut present, mut defaulted, mut dacl) = (0, 0, std::ptr::null_mut());
        if GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) == 0
            || present == 0
        {
            return Err(os_error());
        }
        let status = SetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null(),
        );
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
        }
    }
    Ok(())
}
pub fn verify(path: &Path) -> Result<(), String> {
    unsafe {
        let (mut dacl, mut sd) = (std::ptr::null_mut(), std::ptr::null_mut());
        let status = GetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut sd,
        );
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
        }
        let _sd = Descriptor(sd);
        if dacl.is_null() || (*dacl).AceCount == 0 {
            return Err("cache directory lacks a private DACL".into());
        }
        let mut owner_rights = [0u32; 17];
        let mut system = [0u32; 17];
        let mut size = 68;
        if CreateWellKnownSid(
            WinCreatorOwnerRightsSid,
            std::ptr::null_mut(),
            owner_rights.as_mut_ptr().cast(),
            &mut size,
        ) == 0
        {
            return Err(os_error());
        }
        size = 68;
        if CreateWellKnownSid(
            WinLocalSystemSid,
            std::ptr::null_mut(),
            system.as_mut_ptr().cast(),
            &mut size,
        ) == 0
        {
            return Err(os_error());
        }
        for index in 0..(*dacl).AceCount {
            let mut ace = std::ptr::null_mut();
            if GetAce(dacl, u32::from(index), &mut ace) == 0 {
                return Err(os_error());
            }
            let ace = ace as *const ACCESS_ALLOWED_ACE;
            // Reject unknown/object/callback ACL forms rather than guess permissions.
            if (*ace).Header.AceType != 0 {
                return Err("unsupported cache directory ACE".into());
            }
            let sid = std::ptr::addr_of!((*ace).SidStart) as *mut std::ffi::c_void;
            if EqualSid(sid, owner_rights.as_mut_ptr().cast()) == 0
                && EqualSid(sid, system.as_mut_ptr().cast()) == 0
            {
                return Err("cache directory grants access beyond owner/SYSTEM".into());
            }
        }
    }
    Ok(())
}

/// Install the private descriptor atomically with directory creation, so another
/// launcher never observes a temporarily inherited, broader DACL.
pub fn create_private(path: &Path) -> std::io::Result<()> {
    unsafe {
        let text: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
            .encode_utf16()
            .collect();
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let sd = Descriptor(sd);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        if windows_sys::Win32::Storage::FileSystem::CreateDirectoryW(
            wide(path).as_ptr(),
            &attributes,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
