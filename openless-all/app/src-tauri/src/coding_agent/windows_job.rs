//! Windows process-tree ownership for one Coding Agent execution.
//!
//! A CLI may exit before its workers and leave them holding inherited pipes.
//! Killing by parent PID cannot recover that tree afterwards. An unnamed Job
//! keeps ownership in the kernel, including descendants created after launch.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use openless_core::{BackendError, CancellationToken};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

use super::{invalid, platform_error};

pub(super) struct AgentProcessJob(OwnedHandle);

impl AgentProcessJob {
    pub(super) fn new() -> Result<Self, BackendError> {
        // SAFETY: no name or inheritable SECURITY_ATTRIBUTES are supplied. The
        // returned owned handle is valid and is closed on every error path.
        let handle = unsafe { CreateJobObjectW(None, None) }.map_err(platform_error)?;
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle.0) });
        job.set_kill_on_close(true)?;
        Ok(job)
    }

    fn set_kill_on_close(&self, enabled: bool) -> Result<(), BackendError> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if enabled {
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        }
        // SAFETY: the buffer and its exact size match the selected info class.
        unsafe {
            SetInformationJobObject(
                self.handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(platform_error)
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }

    pub(super) fn assign_and_resume(
        &self,
        child: &tokio::process::Child,
        cancel: &CancellationToken,
    ) -> Result<(), BackendError> {
        let process = HANDLE(
            child
                .raw_handle()
                .ok_or_else(|| invalid("missing agent process handle"))?,
        );
        // SAFETY: the process was created suspended and both handles remain
        // alive. Fail closed if a restrictive enclosing Job disallows nesting;
        // never run an agent that escaped this operation's cleanup ownership.
        unsafe { AssignProcessToJobObject(self.handle(), process) }.map_err(platform_error)?;
        if cancel.is_cancelled() {
            return Ok(());
        }
        let process_id = child
            .id()
            .ok_or_else(|| invalid("missing agent process id"))?;
        // Tokio/stdlib do not expose the primary thread handle on stable 1.88.
        // CREATE_SUSPENDED guarantees it is the only application-created thread
        // and that the CLI cannot create children before assignment to our Job.
        let snapshot =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(platform_error)?;
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.0) };
        let snapshot_handle = HANDLE(snapshot.as_raw_handle());
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = unsafe { Thread32First(snapshot_handle, &mut entry) }.is_ok();
        while found {
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: select only a thread belonging to our suspended child;
                // no user-owned or unrelated process/thread is ever resumed.
                let thread =
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                        .map_err(platform_error)?;
                let thread = unsafe { OwnedHandle::from_raw_handle(thread.0) };
                if cancel.is_cancelled() {
                    return Ok(());
                }
                if unsafe { ResumeThread(HANDLE(thread.as_raw_handle())) } == u32::MAX {
                    return Err(platform_error(windows::core::Error::from_win32()));
                }
                return Ok(());
            }
            found = unsafe { Thread32Next(snapshot_handle, &mut entry) }.is_ok();
        }
        Err(invalid("suspended agent primary thread not found"))
    }

    pub(super) fn terminate(&self) -> Result<(), BackendError> {
        // SAFETY: this Job contains only the child assigned above and its own
        // descendants. OwnedHandle also provides kill-on-close if the execute
        // future is dropped, including before normal cancellation can finish.
        unsafe { TerminateJobObject(self.handle(), 1) }.map_err(platform_error)
    }

    pub(super) fn release_completed(&self) -> Result<(), BackendError> {
        // Natural successful completion previously allowed an explicitly
        // backgrounded worker with redirected stdio to survive. Only disarm
        // once both exit and all output drains completed without cancellation;
        // a worker holding our pipes is still part of an unfinished operation.
        self.set_kill_on_close(false)
    }
}
