//! Story 16.1 — sidebar-monitor-svc: Windows Service binary.
//!
//! Runs as LocalSystem, owns the elevated sensor host, exposes a named
//! pipe for the non-elevated sidebar UI to request sensor frames.
//!
//! Cited: Story 16.1, guardrails.md G10 + G16 + G19, T-48.

#![cfg(windows)]
#![allow(
    unsafe_op_in_unsafe_fn,
    clippy::missing_safety_doc,
    clippy::ptr_as_ptr,
    clippy::borrow_as_ptr,
    clippy::cast_possible_truncation,
    clippy::undocumented_unsafe_blocks,
    clippy::ref_as_ptr,
    clippy::ptr_cast_constness
)]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{WriteFile, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
    PIPE_WAIT,
};
use windows::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SERVICE_ACCEPT_STOP, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE, SERVICE_STOPPED,
    SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
};

const PIPE_NAME: &str = r"\\.\pipe\sidebar-monitor";
const SERVICE_NAME: &str = "sidebar-monitor-svc";
const HOST_EXE_NAME: &str = "sidebar-monitor-host.exe";

static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
static SERVICE_HANDLE_RAW: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn main() {
    let name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(name.as_ptr() as *mut _),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR::null(),
            lpServiceProc: None,
        },
    ];
    // SAFETY: StartServiceCtrlDispatcherW blocks until service stops.
    if let Err(e) = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } {
        eprintln!("StartServiceCtrlDispatcherW failed: {e}");
        std::process::exit(1);
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    let name = wide(SERVICE_NAME);
    // SAFETY: RegisterServiceCtrlHandlerExW registers our handler.
    if let Ok(h) = RegisterServiceCtrlHandlerExW(PCWSTR(name.as_ptr()), Some(control_handler), None)
    {
        let _ = SERVICE_HANDLE_RAW.set(h.0 as usize);
    }
    set_status(SERVICE_START_PENDING);
    let _host = spawn_sensor_host();
    set_status(SERVICE_RUNNING);
    run_pipe_server();
    set_status(SERVICE_STOP_PENDING);
    set_status(SERVICE_STOPPED);
}

unsafe extern "system" fn control_handler(
    control: u32,
    _event_type: u32,
    _event_data: *mut std::ffi::c_void,
    _context: *mut std::ffi::c_void,
) -> u32 {
    if control == SERVICE_CONTROL_STOP {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    }
    0
}

#[allow(clippy::cast_possible_truncation)]
fn set_status(state: SERVICE_STATUS_CURRENT_STATE) {
    let Some(&raw) = SERVICE_HANDLE_RAW.get() else {
        return;
    };
    let handle = SERVICE_STATUS_HANDLE(raw as *mut _);
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING {
            SERVICE_ACCEPT_STOP
        } else {
            0u32
        },
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 5000,
    };
    // SAFETY: handle is valid from registration.
    unsafe {
        let _ = SetServiceStatus(handle, &status);
    }
}

fn spawn_sensor_host() -> Option<HANDLE> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let host_path = exe_dir.join(HOST_EXE_NAME);
    if !host_path.exists() {
        eprintln!("host exe not found: {}", host_path.display());
        return None;
    }
    let host_wide = wide(host_path.to_str()?);

    // SAFETY: CreateJobObjectW — anonymous job.
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.ok()?;
    // SAFETY: zeroed struct is valid for the extended limit info.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: SetInformationJobObject on a job we own.
    unsafe {
        let _ = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        );
    }

    // SAFETY: zeroed structs are valid for STARTUPINFOW + PROCESS_INFORMATION.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    // SAFETY: CreateProcessW spawning the host hidden.
    let result = unsafe {
        CreateProcessW(
            PCWSTR(host_wide.as_ptr()),
            None,
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &startup,
            &mut pi,
        )
    };
    if result.is_err() {
        eprintln!("failed to spawn sensor host");
        return None;
    }

    // SAFETY: AssignProcessToJobObject — both handles valid.
    unsafe {
        let _ = AssignProcessToJobObject(job, pi.hProcess);
    }
    // SAFETY: CloseHandle on owned thread handle.
    unsafe {
        let _ = CloseHandle(pi.hThread);
    }
    // Job handle is Copy; the kernel owns it for the process lifetime.
    // Explicit forget not needed (HANDLE is Copy, no Drop).
    Some(pi.hProcess)
}

fn run_pipe_server() {
    let pipe_name = wide(PIPE_NAME);
    while !SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
        // SAFETY: CreateNamedPipeW creates a duplex pipe instance.
        let pipe = unsafe {
            CreateNamedPipeW(
                PCWSTR(pipe_name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                4096,
                4096,
                1000,
                None,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            std::thread::sleep(std::time::Duration::from_secs(2));
            continue;
        }
        // SAFETY: ConnectNamedPipe blocks until a client connects.
        unsafe {
            let _ = ConnectNamedPipe(pipe, None);
        }
        if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
            // SAFETY: CloseHandle on owned pipe.
            unsafe {
                let _ = CloseHandle(pipe);
            }
            break;
        }
        // Write one response frame (initial implementation).
        let response = b"{\"status\":\"service_running\"}\n";
        let mut written: u32 = 0;
        // SAFETY: WriteFile to the connected pipe.
        unsafe {
            let _ = WriteFile(pipe, Some(response), Some(&mut written), None);
        }
        // SAFETY: DisconnectNamedPipe + CloseHandle.
        unsafe {
            let _ = DisconnectNamedPipe(pipe);
            let _ = CloseHandle(pipe);
        }
    }
}
