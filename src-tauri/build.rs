use std::path::PathBuf;

const CLOUDFLARE_OAUTH_CLIENT_ID: &str = "MUTSUNA_CLOUDFLARE_OAUTH_CLIENT_ID";

fn cloudflare_oauth_client_id() -> Option<String> {
    if let Some(value) = std::env::var(CLOUDFLARE_OAUTH_CLIENT_ID)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return Some(value);
    }

    let env_path = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?)
        .parent()?
        .join(".env");
    dotenvy::from_path_iter(env_path)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|(key, value)| {
            (key == CLOUDFLARE_OAUTH_CLIENT_ID)
                .then(|| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

fn main() {
    println!("cargo:rerun-if-env-changed={CLOUDFLARE_OAUTH_CLIENT_ID}");
    if let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
        if let Some(repository_dir) = PathBuf::from(manifest_dir).parent() {
            println!(
                "cargo:rerun-if-changed={}",
                repository_dir.join(".env").display()
            );
        }
    }
    if let Some(client_id) = cloudflare_oauth_client_id() {
        println!("cargo:rustc-env={CLOUDFLARE_OAUTH_CLIENT_ID}={client_id}");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // The generated Tauri command dispatcher plus native media/model
        // integrations can exceed the PE default 1 MiB main-thread stack in
        // debug builds. Keep the event loop on the required main thread and
        // reserve explicit headroom for both development and release binaries.
        println!("cargo:rustc-link-arg-bin=mutsuna-echo=/STACK:8388608");
    }
    tauri_build::build()
}
