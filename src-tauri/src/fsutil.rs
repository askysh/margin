use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Write via a hidden temp file in the same directory, then rename. Keeps `mode` when
/// given, else the existing file's mode, else 0600 (minutes-core writes meetings 0600).
///
/// Windows has no file modes, so `mode` does nothing there. A new file takes the access
/// list of its folder. Margin writes to its data folder and to the meetings folder, and
/// both are inside the user's own profile unless the user chose another place.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.margin-tmp-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    #[cfg(not(unix))]
    let _ = mode;
    #[cfg(unix)]
    let mode = mode
        .or_else(|| std::fs::metadata(path).ok().map(|m| m.permissions().mode() & 0o777))
        .unwrap_or(0o600);
    // Any failure removes the temp file, so a full disk leaves no hidden copies in the
    // meetings folder.
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        replace(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(not(windows))]
fn replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, path)
}

/// Windows refuses the rename while another program has the file open without the right to
/// replace it. An editor or a virus scan does that for a moment. Try for half a second.
#[cfg(windows)]
fn replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    let mut tries = 0;
    loop {
        match std::fs::rename(tmp, path) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && path.is_file() && tries < 10 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            result => return result,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_write_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("margin-fsutil-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // The rename fails: the target is a folder that is not empty.
        let target = dir.join("note.md");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        assert!(write_atomic(&target, b"x", None).is_err());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("note.md")]);
        let ok = dir.join("ok.md");
        write_atomic(&ok, b"y", None).unwrap();
        assert_eq!(std::fs::read(&ok).unwrap(), b"y");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
