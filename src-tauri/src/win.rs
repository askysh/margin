//! The Windows API calls that Margin makes, in one place (DECISIONS.md D61).
//!
//! The macOS build does the same work with process groups, `open`, TCC and thread QoS.
//! Windows has other tools for each one:
//!
//! - A Job Object holds a child process and every process that the child starts. Margin
//!   stops Ask with it. A plain "kill the child" leaves `node` running when `claude` is a
//!   `claude.cmd` shim.
//! - The shell opens a URL (`ShellExecuteW`), and Explorer shows a file.
//! - The registry holds the microphone privacy switches.
//! - The account's display name comes from `GetUserNameExW`.
//!
//! Each function fails soft: a call that Windows refuses gives `None`, `false` or an
//! `io::Error`, never a panic.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, OpenThread, ResumeThread, SetThreadPriority, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    THREAD_PRIORITY_BELOW_NORMAL, THREAD_SUSPEND_RESUME,
};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------- child processes

/// A Job Object. Every process in it stops when Margin calls `terminate`, and also when
/// Margin itself goes away (`KILL_ON_JOB_CLOSE`).
pub struct Job(HANDLE);

// SAFETY: a job handle is a kernel object. Windows lets any thread use it.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    fn new() -> std::io::Result<Job> {
        // SAFETY: null arguments ask for a new job with no name and default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Job(handle);
        // SAFETY: the struct is plain data, and zero is a valid value for each field.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` lives for the call, and the size is the size of its type.
        let ok = unsafe {
            SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Stop every process in the job, now. Windows has no SIGTERM for a process tree.
    pub fn terminate(&self) {
        // SAFETY: the handle is a live job for as long as `self` is.
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed only here.
        unsafe { CloseHandle(self.0) };
    }
}

/// Start `command` inside a new Job Object, with no console window.
///
/// The child starts suspended and joins the job before it runs any code. So a process that
/// it starts at once, as `cmd.exe` does for a `.cmd` file, is in the job too.
pub fn spawn_in_job(command: &mut Command) -> std::io::Result<(Child, Arc<Job>)> {
    let job = Job::new()?;
    let mut child = command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW).spawn()?;
    // SAFETY: the process handle is live while `child` is.
    let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) } != 0;
    let error = std::io::Error::last_os_error();
    if !assigned || !resume(child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(if assigned { std::io::Error::other("could not start the suspended process") } else { error });
    }
    Ok((child, Arc::new(job)))
}

/// Resume the threads of a process that started suspended. A new suspended process has
/// one thread. The standard library does not give its handle, so find it by process ID.
fn resume(pid: u32) -> bool {
    // SAFETY: the snapshot handle is checked and closed. `entry` has its size set, as
    // Thread32First requires. Each thread handle is checked and closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = false;
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    resumed |= ResumeThread(thread) != u32::MAX;
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        resumed
    }
}

/// For a child that must outlive Margin and take no Ctrl-C meant for it: the processing
/// worker. The same purpose as a new process group on macOS. No console window opens.
pub fn detach(command: &mut Command) {
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

/// For a short helper that has a console, such as `claude --help`: no console window opens.
pub fn no_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Below-normal priority for the calling thread. Windows does not pass a thread's priority
/// to the threads it starts, so Whisper's own worker threads keep the normal priority.
pub fn lower_thread_priority() -> bool {
    // SAFETY: GetCurrentThread gives a pseudo-handle that is always valid.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) != 0 }
}

// ---------------------------------------------------------------- the shell

/// Open a URL or a file with the program that Windows has for it. No shell reads the text,
/// so the characters in it have no special meaning.
pub fn shell_open(target: &str) -> std::io::Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (verb, target) = (wide("open"), wide(target));
    // SAFETY: both strings end with a zero and live for the call. The other pointers are
    // null, which ShellExecuteW allows.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    // ShellExecuteW gives a value above 32 when it worked.
    if result as usize > 32 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("Windows could not open it (code {})", result as usize)))
    }
}

/// Show a file in Explorer, selected. Explorer needs the comma form with no space.
pub fn reveal(path: &std::path::Path) {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    // Explorer gives exit code 1 also when it worked, so the result says nothing.
    let _ = Command::new("explorer.exe").arg(arg).spawn();
}

// ---------------------------------------------------------------- the registry

fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(value));
    let mut buf = [0u16; 512];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: the strings end with a zero, and `bytes` is the size of `buf` in bytes.
    let rc = unsafe { RegGetValueW(root, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut bytes) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The microphone switches in Settings, Privacy and security, Microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicConsent {
    Allowed,
    Denied,
    Unknown,
}

/// What Windows says about microphone access for this app. It reads the same registry
/// values as the Settings page and opens no device, so no prompt can appear.
pub fn microphone_consent() -> MicConsent {
    const STORE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    let read = |root: HKEY, sub: Option<&str>| {
        let key = sub.map_or(STORE.to_string(), |s| format!(r"{STORE}\{s}"));
        reg_string(root, &key, "Value")
    };
    // A Store package has its own switch, by package family name. Other desktop programs
    // share one switch.
    let app = package_family_name().and_then(|name| read(HKEY_CURRENT_USER, Some(&name))).or_else(|| read(HKEY_CURRENT_USER, Some("NonPackaged")));
    mic_consent_from(read(HKEY_LOCAL_MACHINE, None).as_deref(), read(HKEY_CURRENT_USER, None).as_deref(), app.as_deref())
}

/// `device`: the switch for the whole PC. `user`: the switch for this account. `app`: the
/// switch for this app, or for all desktop apps. One "Deny" is enough to block the
/// microphone. With no "Deny", one "Allow" is enough to say that it is allowed. A missing
/// value means that Windows uses its default, which Margin cannot read.
pub fn mic_consent_from(device: Option<&str>, user: Option<&str>, app: Option<&str>) -> MicConsent {
    let all = [device, user, app];
    if all.iter().flatten().any(|v| v.eq_ignore_ascii_case("Deny")) {
        MicConsent::Denied
    } else if all.iter().flatten().any(|v| v.eq_ignore_ascii_case("Allow")) {
        MicConsent::Allowed
    } else {
        MicConsent::Unknown
    }
}

/// The package family name when Margin runs from its Store package, else `None`.
fn package_family_name() -> Option<String> {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: `len` is the size of `buf` in characters, as the function requires.
    let rc = unsafe { GetCurrentPackageFamilyName(&mut len, buf.as_mut_ptr()) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

// ---------------------------------------------------------------- the account

/// The account's display name, such as "Priya Sharma". `None` for an account that has
/// none, which is usual for a local account.
pub fn display_name() -> Option<String> {
    use windows_sys::Win32::Security::Authentication::Identity::{GetUserNameExW, NameDisplay};
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: `len` is the size of `buf` in characters, as the function requires.
    let ok = unsafe { GetUserNameExW(NameDisplay, buf.as_mut_ptr(), &mut len) };
    if ok == 0 {
        return None;
    }
    let name = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_deny_blocks_the_microphone() {
        assert_eq!(mic_consent_from(Some("Allow"), Some("Allow"), Some("Allow")), MicConsent::Allowed);
        assert_eq!(mic_consent_from(Some("Allow"), Some("Deny"), Some("Allow")), MicConsent::Denied);
        assert_eq!(mic_consent_from(Some("Deny"), None, None), MicConsent::Denied);
        assert_eq!(mic_consent_from(Some("Allow"), Some("Allow"), Some("deny")), MicConsent::Denied);
        assert_eq!(mic_consent_from(None, Some("Allow"), None), MicConsent::Allowed);
        assert_eq!(mic_consent_from(None, None, None), MicConsent::Unknown);
        assert_eq!(mic_consent_from(Some("Prompt"), None, None), MicConsent::Unknown);
    }

    /// A job stops the whole tree: `cmd.exe` and the `ping` that it starts.
    #[test]
    fn a_job_stops_the_child_and_its_children() {
        let mut command = Command::new("cmd.exe");
        command.args(["/c", "ping -n 30 127.0.0.1 >nul"]).stdin(std::process::Stdio::null());
        let (mut child, job) = spawn_in_job(&mut command).expect("spawn in a job");
        // Give cmd.exe time to start ping.
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(child.try_wait().unwrap().is_none(), "the child runs after the resume");
        let started = std::time::Instant::now();
        job.terminate();
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "ping ran for 30 s without the job");
        assert_eq!(processes_in(&job), 0, "no process of the tree is left");
    }

    fn processes_in(job: &Job) -> u32 {
        use windows_sys::Win32::System::JobObjects::{JobObjectBasicAccountingInformation, QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION};
        // SAFETY: the struct is plain data, and the size is the size of its type.
        unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            let ok = QueryInformationJobObject(job.0, JobObjectBasicAccountingInformation, &mut info as *mut _ as *mut _, std::mem::size_of_val(&info) as u32, std::ptr::null_mut());
            assert!(ok != 0);
            info.ActiveProcesses
        }
    }
}
