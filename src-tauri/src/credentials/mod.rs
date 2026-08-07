#[cfg(target_os = "android")]
mod android_keystore;
#[cfg(all(test, not(target_os = "android")))]
#[allow(dead_code)]
mod android_keystore;
#[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
mod keyring_store;
#[cfg(target_os = "windows")]
mod windows_dpapi;

#[cfg(target_os = "android")]
pub(crate) use android_keystore::{delete_api_key, has_api_key, load_api_key, save_api_key};
#[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
pub(crate) use keyring_store::{delete_api_key, has_api_key, load_api_key, save_api_key};
#[cfg(target_os = "windows")]
pub(crate) use windows_dpapi::{delete_api_key, has_api_key, load_api_key, save_api_key};
