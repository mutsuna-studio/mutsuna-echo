use libloading::Library;
use std::{path::Path, sync::{OnceLock, RwLock}};

struct RuntimeLibraries {
    libraries: Vec<Library>,
}

static RUNTIME: OnceLock<RwLock<Option<RuntimeLibraries>>> = OnceLock::new();

fn runtime() -> &'static RwLock<Option<RuntimeLibraries>> {
    RUNTIME.get_or_init(|| RwLock::new(None))
}

/// Loads dependencies first and the sherpa C API library last. Libraries stay
/// resident for the process lifetime so C objects never outlive their symbols.
pub fn load_runtime(paths: &[impl AsRef<Path>]) -> Result<(), String> {
    let mut guard = runtime().write().map_err(|_| "runtime lock poisoned".to_string())?;
    if guard.is_some() {
        return Ok(());
    }

    let mut libraries = Vec::with_capacity(paths.len());
    for path in paths {
        let path = path.as_ref();
        let library = unsafe { Library::new(path) }
            .map_err(|error| format!("failed to load {}: {error}", path.display()))?;
        libraries.push(library);
    }
    if libraries.is_empty() {
        return Err("runtime pack contains no libraries".to_string());
    }
    *guard = Some(RuntimeLibraries { libraries });
    Ok(())
}

pub fn is_loaded() -> bool {
    runtime().read().map(|guard| guard.is_some()).unwrap_or(false)
}

/// Unloads the runtime after the host has proven that no inference object is alive.
pub fn unload_runtime() -> Result<(), String> {
    let mut guard = runtime().write().map_err(|_| "runtime lock poisoned".to_string())?;
    *guard = None;
    Ok(())
}

pub(crate) unsafe fn symbol<T: Copy>(name: &[u8]) -> Result<T, String> {
    let guard = runtime().read().map_err(|_| "runtime lock poisoned".to_string())?;
    let runtime = guard.as_ref().ok_or_else(|| "local AI runtime is not loaded".to_string())?;
    for library in runtime.libraries.iter().rev() {
        if let Ok(symbol) = unsafe { library.get::<T>(name) } {
            return Ok(*symbol);
        }
    }
    Err(format!("runtime symbol is missing: {}", String::from_utf8_lossy(name)))
}
