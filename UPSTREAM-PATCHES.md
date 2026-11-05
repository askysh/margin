# Upstream patches (candidates for PRs to silverstein/minutes)

Each entry: what, why, where. Nothing here has been applied to upstream. We work around
each one locally.

## 1. `voice::db_path()` should honour `MINUTES_DATA_DIR`
- **Where:** `crates/core/src/voice.rs` `db_path()` (v0.32.1 ~line 135). It hardcodes
  `dirs::home_dir()/.minutes/voices.db`.
- **Why:** every other piece of state follows `Config::minutes_dir()` (`MINUTES_DATA_DIR`).
  `voices.db` alone leaks into `~/.minutes`, so a host that isolates its state still shares
  voice profiles with any other Minutes install.
- **Patch:** `Config::minutes_dir().join("voices.db")`, with a one-time fallback that reads
  the legacy path.
- **Workaround:** we share `~/.minutes/voices.db`, and Reset leaves it alone unless asked.

## 2. Public model-download API with progress
- **Where:** the CLI's private `download_file` and `cmd_setup` in `crates/cli/src/main.rs`.
  Upstream's Tauri app re-implements the same thing in `commands.rs`.
- **Why:** every GUI host needs "download whisper/VAD/diarization models with a progress
  bar". Today each host copies the URLs and the loop.
- **Patch:** `minutes_core::models::{required_models(&Config) -> Vec<ModelSpec>,
  download(&ModelSpec, &mut dyn FnMut(u64, u64))}` with checksums.
- **Workaround:** our own downloader in `src-tauri/src/models.rs`.
