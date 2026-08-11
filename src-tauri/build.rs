fn main() {
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
