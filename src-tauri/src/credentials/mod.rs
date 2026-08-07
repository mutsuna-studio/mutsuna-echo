#[cfg(not(target_os = "windows"))]
mod keyring_store;
#[cfg(target_os = "windows")]
mod windows_dpapi;

#[cfg(not(target_os = "windows"))]
pub(crate) use keyring_store::{delete_api_key, has_api_key, load_api_key, save_api_key};
#[cfg(target_os = "windows")]
pub(crate) use windows_dpapi::{delete_api_key, has_api_key, load_api_key, save_api_key};
