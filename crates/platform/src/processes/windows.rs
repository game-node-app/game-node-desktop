//! Toolhelp discovery with cached handles and thread-pool exit waits.
use gamenode_core::ports::{ProcessError, ProcessInfo, Processes};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, RegisterWaitForSingleObject, UnregisterWaitEx, WT_EXECUTEONLYONCE,
    WaitForSingleObject,
};

struct Handle(HANDLE);
// SAFETY: process and snapshot handles may be used from any thread; ownership closes them once.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn filetime(t: FILETIME) -> SystemTime {
    let ticks = ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
    let epoch_ticks = 116444736000000000;
    if ticks >= epoch_ticks {
        UNIX_EPOCH + Duration::from_nanos((ticks - epoch_ticks) * 100)
    } else {
        UNIX_EPOCH - Duration::from_nanos((epoch_ticks - ticks) * 100)
    }
}
fn times(handle: HANDLE) -> Option<(SystemTime, SystemTime)> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: handle stays owned by the caller, and all output pointers reference initialized storage.
    (unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } != 0)
        .then(|| (filetime(creation), filetime(exit)))
}
fn inspect(pid: u32) -> Option<(ProcessInfo, Arc<Handle>)> {
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if raw.is_null() {
        tracing::debug!(pid, "skipped inaccessible process");
        return None;
    }
    let handle = Arc::new(Handle(raw));
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(raw, 0, path.as_mut_ptr(), &mut length) } == 0 {
        return None;
    }
    let (created_at, _) = times(raw)?;
    let exe_path = String::from_utf16(&path[..length as usize]).ok()?;
    Some((
        ProcessInfo {
            pid,
            created_at,
            exe_path,
        },
        handle,
    ))
}
type Callback = Box<dyn FnOnce(SystemTime) + Send>;
struct Context {
    process: Arc<Handle>,
    callback: Mutex<Option<Callback>>,
    done: AtomicBool,
}
unsafe extern "system" fn exited(context: *mut std::ffi::c_void, _timed_out: bool) {
    // SAFETY: Watch keeps the Box alive until UnregisterWaitEx has waited for callbacks to finish.
    let context = unsafe { &*(context as *const Context) };
    if let Some((_, at)) = times(context.process.0)
        && let Some(callback) = context
            .callback
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    {
        // Never unwind through the Windows ABI boundary.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(at)));
    }
    context.done.store(true, Ordering::Release);
}
struct Watch {
    registration: HANDLE,
    context: Box<Context>,
}
// SAFETY: registration is synchronized by Windows, context mutation by its mutex/atomic.
unsafe impl Send for Watch {}
impl Drop for Watch {
    fn drop(&mut self) {
        // Blocking unregister joins any callback before the context or process handle is freed.
        unsafe {
            UnregisterWaitEx(self.registration, INVALID_HANDLE_VALUE);
        }
    }
}
#[derive(Default)]
struct State {
    cache: BTreeMap<u32, (ProcessInfo, Arc<Handle>)>,
    watches: Vec<Watch>,
}
#[derive(Default)]
pub struct HostProcesses(Mutex<State>);
impl HostProcesses {
    pub fn new() -> Self {
        Self::default()
    }
}
impl Processes for HostProcesses {
    fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state
            .watches
            .retain(|w| !w.context.done.load(Ordering::Acquire));
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if raw == INVALID_HANDLE_VALUE {
            return Err(ProcessError::Failed);
        }
        let snapshot = Handle(raw);
        let mut item = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut seen = BTreeSet::new();
        let mut has = unsafe { Process32FirstW(snapshot.0, &mut item) } != 0;
        while has {
            let pid = item.th32ProcessID;
            seen.insert(pid);
            let alive = state
                .cache
                .get(&pid)
                .is_some_and(|(_, h)| unsafe { WaitForSingleObject(h.0, 0) } == 258);
            if !alive {
                state.cache.remove(&pid);
                if let Some(process) = inspect(pid) {
                    state.cache.insert(pid, process);
                }
            }
            has = unsafe { Process32NextW(snapshot.0, &mut item) } != 0;
        }
        state.cache.retain(|pid, _| seen.contains(pid));
        Ok(state.cache.values().map(|(p, _)| p.clone()).collect())
    }
    fn watch(&self, process: &ProcessInfo, on_exit: Callback) -> Result<(), ProcessError> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let handle = if let Some((cached, handle)) = state
            .cache
            .get(&process.pid)
            .filter(|(cached, _)| cached.created_at == process.created_at)
        {
            let _ = cached;
            handle.clone()
        } else {
            let (current, handle) = inspect(process.pid).ok_or(ProcessError::Failed)?;
            if current.created_at != process.created_at {
                return Err(ProcessError::Failed);
            }
            handle
        };
        let context = Box::new(Context {
            process: handle,
            callback: Mutex::new(Some(on_exit)),
            done: AtomicBool::new(false),
        });
        let mut registration = std::ptr::null_mut();
        let ok = unsafe {
            RegisterWaitForSingleObject(
                &mut registration,
                context.process.0,
                Some(exited),
                (&*context as *const Context).cast(),
                INFINITE,
                WT_EXECUTEONLYONCE,
            )
        };
        if ok == 0 {
            return Err(ProcessError::Failed);
        }
        state.watches.push(Watch {
            registration,
            context,
        });
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_in_temp_folder_is_listed_and_reports_its_real_exit_time_once() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("child.exe");
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(std::path::Path::new(&system).join("System32/cmd.exe"), &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["/C", "ping -n 3 127.0.0.1 > nul"])
            .spawn()
            .unwrap();
        let adapter = HostProcesses::default();
        let process = adapter
            .list()
            .unwrap()
            .into_iter()
            .find(|p| p.pid == child.id())
            .unwrap();
        assert_eq!(std::path::Path::new(&process.exe_path), exe);
        let (tx, rx) = std::sync::mpsc::channel();
        adapter
            .watch(
                &process,
                Box::new(move |at| {
                    tx.send(at).unwrap();
                }),
            )
            .unwrap();
        child.wait().unwrap();
        let at = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(at > process.created_at);
        assert!(at <= SystemTime::now());
        assert!(rx.try_recv().is_err());
    }
}
