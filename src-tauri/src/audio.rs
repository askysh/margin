//! Playing a meeting's recording. The app hands the WebView an `asset:` URL for the
//! `.wav` next to the meeting file, after adding only that file to the asset scope. The
//! `--dev serve` bridge serves the same file over HTTP, with byte ranges, for the tests.

use crate::library;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A slice of a file for an HTTP response: 206 for a range request, else 200.
pub struct Slice {
    pub status: u16,
    pub start: u64,
    pub total: u64,
    pub bytes: Vec<u8>,
}

/// The `.wav` that `wav` names, if it belongs to a meeting in the meetings folder.
pub fn meeting_wav(wav: &Path) -> Result<PathBuf, String> {
    if wav.extension().is_none_or(|e| e != "wav") {
        return Err("Not a recording.".into());
    }
    library::audio_path(&wav.with_extension("md"))
}

/// Read the bytes a `Range: bytes=a-b` header asks for (at most 4 MB at a time, which is
/// what media elements expect), or the whole file when there is no header.
pub fn read_range(path: &Path, range: Option<&str>) -> std::io::Result<Slice> {
    const CHUNK: u64 = 4 * 1024 * 1024;
    let mut file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();
    let Some((start, end)) = range.and_then(|r| parse_range(r, total)) else {
        let mut bytes = Vec::with_capacity(total as usize);
        file.read_to_end(&mut bytes)?;
        return Ok(Slice { status: 200, start: 0, total, bytes });
    };
    let end = end.min(start + CHUNK - 1);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0; (end - start + 1) as usize];
    file.read_exact(&mut bytes)?;
    Ok(Slice { status: 206, start, total, bytes })
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` → an inclusive range inside the file.
fn parse_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let last = total.checked_sub(1)?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => (total.saturating_sub(n.parse().ok()?), last),
        (a, "") => (a.parse().ok()?, last),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(last)),
    };
    (start <= end).then_some((start, end))
}

/// The app's command: the recording's path, now readable through the asset protocol.
#[tauri::command]
pub async fn meeting_audio(app: tauri::AppHandle, path: String) -> Result<String, String> {
    use tauri::Manager;
    let wav = tauri::async_runtime::spawn_blocking(move || library::audio_path(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())??;
    app.asset_protocol_scope().allow_file(&wav).map_err(|e| e.to_string())?;
    Ok(wav.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-19", 100), Some((10, 19)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-40", 100), None);
        assert_eq!(parse_range("bytes=0-", 0), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }
}
