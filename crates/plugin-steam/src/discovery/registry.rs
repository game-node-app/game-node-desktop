#[cfg(windows)]
pub(super) mod platform {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(crate) fn read_current_user_string(key: &str, value: &str) -> Option<String> {
        if key.contains('\0') || value.contains('\0') {
            return None;
        }
        let key = wide(key);
        let value = wide(value);
        let mut bytes: u32 = 0;
        // SAFETY: both strings are NUL-terminated and outlive the call; a null data pointer with
        // a size pointer asks only for the required size.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status != 0 || bytes == 0 || bytes > 64 * 1024 {
            return None;
        }
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
        let mut capacity = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` is writable for `capacity` bytes and the strings are as above.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut capacity,
            )
        };
        if status != 0 {
            return None;
        }
        let length = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16(&buffer[..length]).ok()
    }
}

#[cfg(not(windows))]
pub(super) mod platform {
    pub(crate) fn read_current_user_string(_: &str, _: &str) -> Option<String> {
        None
    }
}
