use std::{fs, path::PathBuf, slice};

use secrecy::{ExposeSecret, SecretString};
use tauri::{AppHandle, Manager};
use windows::Win32::{
    Foundation::{LocalFree, HLOCAL},
    Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    },
};

use super::CredentialId;

fn credential_path(app: &AppHandle, credential: CredentialId) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|directory| directory.join(format!("{}-api-key.dpapi", credential.id())))
        .map_err(|error| {
            eprintln!("Could not resolve app data directory: {error:?}");
            "APIキーの保存先を取得できませんでした。".to_string()
        })
}

use zeroize::{Zeroize, Zeroizing};

fn copy_and_free(blob: CRYPT_INTEGER_BLOB, clear_before_free: bool) -> Vec<u8> {
    let bytes = if blob.cbData == 0 || blob.pbData.is_null() {
        Vec::new()
    } else {
        // SAFETY: DPAPI returned a buffer containing exactly cbData bytes.
        unsafe { slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() }
    };

    if !blob.pbData.is_null() {
        if clear_before_free && blob.cbData > 0 {
            // SAFETY: DPAPI returned a writable allocation of exactly cbData
            // bytes. Clear plaintext before returning it to the Windows heap.
            unsafe { slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize) }.zeroize();
        }

        // SAFETY: DPAPI allocates output with LocalAlloc and transfers ownership
        // to the caller, which must release it using LocalFree.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
    }

    bytes
}

fn protect(plain_text: &[u8]) -> Result<Vec<u8>, String> {
    let input_length = u32::try_from(plain_text.len())
        .map_err(|_| "APIキーが長すぎるため保存できません。".to_string())?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: input_length,
        pbData: plain_text.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: input points to plain_text for the duration of the call and
    // output is initialized by DPAPI. Optional pointers are intentionally null.
    unsafe {
        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| {
        eprintln!("DPAPI encryption failed: {error:?}");
        "Windowsの暗号化機能でAPIキーを保護できませんでした。Windowsへサインインし直してから再試行してください。".to_string()
    })?;

    Ok(copy_and_free(output, false))
}

fn unprotect(encrypted: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let input_length = u32::try_from(encrypted.len())
        .map_err(|_| "保存済みAPIキーのデータが大きすぎます。".to_string())?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: input_length,
        pbData: encrypted.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();

    // SAFETY: input points to encrypted for the duration of the call and
    // output is initialized by DPAPI. Optional pointers are intentionally null.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .map_err(|error| {
        eprintln!("DPAPI decryption failed: {error:?}");
        "保存済みAPIキーを復号できませんでした。設定画面でキーを削除し、もう一度登録してください。"
            .to_string()
    })?;

    Ok(Zeroizing::new(copy_and_free(output, true)))
}

pub(crate) fn save(
    app: &AppHandle,
    credential: CredentialId,
    api_key: &SecretString,
) -> Result<(), String> {
    let path = credential_path(app, credential)?;
    let parent = path
        .parent()
        .ok_or_else(|| "APIキーの保存先が不正です。".to_string())?;

    fs::create_dir_all(parent).map_err(|error| {
        eprintln!("Could not create credential directory: {error:?}");
        "APIキーの保存フォルダーを作成できませんでした。".to_string()
    })?;

    let encrypted = protect(api_key.expose_secret().as_bytes())?;
    fs::write(path, encrypted).map_err(|error| {
        eprintln!("Could not write encrypted API key: {error:?}");
        "暗号化したAPIキーを保存できませんでした。保存先への書き込み権限を確認してください。"
            .to_string()
    })
}

pub(crate) fn has(app: &AppHandle, credential: CredentialId) -> Result<bool, String> {
    let path = credential_path(app, credential)?;

    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len() > 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            eprintln!("Could not inspect encrypted API key: {error:?}");
            Err("保存済みAPIキーの状態を確認できませんでした。".to_string())
        }
    }
}

pub(crate) fn load(app: &AppHandle, credential: CredentialId) -> Result<SecretString, String> {
    let path = credential_path(app, credential)?;
    let encrypted = match fs::read(path) {
        Ok(encrypted) => encrypted,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{} APIキーが未設定です。", credential.label()));
        }
        Err(error) => {
            eprintln!("Could not read encrypted API key: {error:?}");
            return Err("保存済みAPIキーを読み込めませんでした。".to_string());
        }
    };
    let plain_text = unprotect(&encrypted)?;

    let api_key = std::str::from_utf8(&plain_text).map_err(|_| {
        eprintln!("Decrypted API key was not valid UTF-8");
        "保存済みAPIキーの形式が壊れています。キーを削除して再登録してください。".to_string()
    })?;

    Ok(SecretString::from(api_key.to_owned()))
}

pub(crate) fn delete(app: &AppHandle, credential: CredentialId) -> Result<(), String> {
    let path = credential_path(app, credential)?;

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            eprintln!("Could not delete encrypted API key: {error:?}");
            Err("保存済みAPIキーを削除できませんでした。".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{protect, unprotect};

    #[test]
    fn protects_and_unprotects_with_current_windows_user() {
        let plain_text = b"sk_mutsuna_echo_test_value";

        let encrypted = protect(plain_text).expect("DPAPI encryption should succeed");
        let decrypted = unprotect(&encrypted).expect("DPAPI decryption should succeed");

        assert_ne!(encrypted, plain_text);
        assert_eq!(decrypted.as_slice(), plain_text);
    }
}
