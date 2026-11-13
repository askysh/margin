/** What differs between the macOS app and the Windows app (DECISIONS.md D61): the names
 * the app shows, the keys, and the features that the Windows build does not have yet.
 * Each macOS value is the text the app had before the Windows build. */

/** WebView2 on Windows says "Windows NT" in its user agent. WKWebView on macOS does not. */
export const isWindows = typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);

/** The product name: the window, setup and About show it. */
export const APP_NAME = isWindows ? "Margin Notes" : "Margin";
/** As in "on this Mac". */
export const DEVICE = isWindows ? "PC" : "Mac";
export const OS_NAME = isWindows ? "Windows" : "macOS";
/** Where the icon lives while the window is closed. */
export const TRAY = isWindows ? "notification area" : "menu bar";
export const SHOW_IN_FILES = isWindows ? "Show in File Explorer" : "Show in Finder";
export const OS_SETTINGS = isWindows ? "Windows Settings" : "System Settings";
/** The key that setup calls "Return" on a Mac. */
export const ENTER_KEY = isWindows ? "Enter" : "Return";
/** The global shortcut, as shortcut.rs registers it. */
export const GLOBAL_SHORTCUT = isWindows ? "Ctrl+Alt+Shift+R" : "⌃⌥⌘R";

/** Windows cannot record the other people on a call yet. */
export const canRecordCalls = !isWindows;
/** The recording chip, Open at login and Apple Speech are macOS only for now. The Store
 * updates the Windows app, so it has no Updates section. */
export const hasRecordingChip = !isWindows;
export const hasOpenAtLogin = !isWindows;
export const hasAppleSpeech = !isWindows;
export const hasOwnUpdater = !isWindows;
/** Windows has no permission for system audio. */
export const hasSystemAudioPermission = !isWindows;

/** The label of an in-window shortcut: `keys("⌘⇧N", "Ctrl+Shift+N")`. */
export function keys(mac: string, windows: string): string {
  return isWindows ? windows : mac;
}

/** The modifier of the in-window shortcuts: ⌘ on macOS, Ctrl on Windows. AltGr on a
 * Windows keyboard is Ctrl+Alt, and it types characters, so Alt rules it out. */
export function hasModKey(e: { metaKey: boolean; ctrlKey: boolean; altKey: boolean }): boolean {
  return isWindows ? e.ctrlKey && !e.altKey : e.metaKey;
}

/** `dir` plus one more name, with the separator that `dir` already uses. */
export function joinPath(dir: string, name: string): string {
  return `${dir}${dir.includes("\\") ? "\\" : "/"}${name}`;
}
