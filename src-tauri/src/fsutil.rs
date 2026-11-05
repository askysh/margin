use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Write via a hidden temp file in the same directory, then rename. Keeps `mode` when
/// given, else the existing file's mode, else 0600 (minutes-core writes meetings 0600).
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.margin-tmp-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let mode = mode
        .or_else(|| std::fs::metadata(path).ok().map(|m| m.permissions().mode() & 0o777))
        .unwrap_or(0o600);
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// One lock per meeting file, held for a whole read-modify-write, so the notes autosave,
/// a title edit, a speaker rename and the processing watcher can't undo each other.
pub fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static LOCKS: OnceLock<Mutex<HashMap<std::path::PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let lock = LOCKS.get_or_init(Default::default).lock().unwrap().entry(key).or_default().clone();
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    f()
}
