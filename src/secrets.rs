//! Reads connection passwords that MyQuery saved (read-only; MyMeta never writes them).
use std::{collections::BTreeMap, path::PathBuf};

fn store_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("APPDATA").ok_or("APPDATA unavailable")?;
    Ok(PathBuf::from(root).join("MyDataTools").join("passwords.dat"))
}

pub fn password(profile: &str) -> Result<String, String> {
    let path = store_path()?;
    if !path.exists() {
        return Err("未找到 MyQuery 保存的密码，请先在 MyQuery 中连接一次".into());
    }
    let plain = unprotect(&std::fs::read(path).map_err(|e| e.to_string())?)?;
    let stored: BTreeMap<String, String> = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
    stored.get(profile).cloned().ok_or_else(|| format!("MyQuery 中没有连接“{profile}”的密码"))
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData},
    };
    let input = CRYPT_INTEGER_BLOB { cbData: data.len().try_into().map_err(|_| "Password data too large")?, pbData: data.as_ptr().cast_mut() };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI allocates the output; LocalFree releases it after copying.
    unsafe {
        if CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        for index in 0..output.cbData as usize { output.pbData.add(index).write_volatile(0); }
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

// shortcut: V1 is Windows-only; V2 (Linux server) will read credentials from server config instead.
#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
    Err("Password store is only available on Windows".into())
}
