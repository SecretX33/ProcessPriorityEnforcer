use crate::config::AppPriorityConfig;
use crate::debug_log;
use color_eyre::Result;
use color_eyre::eyre::bail;
use std::ffi::{OsString, c_void};
use std::io;
use std::io::Error;
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use windows::Win32::Foundation::{CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_DEBUG_NAME,
    SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetCurrentProcess,
    HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, OpenProcess, OpenProcessToken,
    PROCESS_CREATION_FLAGS, PROCESS_NAME_WIN32, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, ProcessPowerThrottling,
    QueryFullProcessImageNameW, SetPriorityClass, SetProcessInformation,
};
use windows::core::Result as WindowsResult;
use windows::core::{PCWSTR, PWSTR};

pub enum HandleOpenType {
    QueryInfo,
    SetInfo,
}

pub fn open_process(process_id: u32, open_type: HandleOpenType) -> Result<ManagedHandle> {
    let access = match open_type {
        HandleOpenType::QueryInfo => PROCESS_QUERY_LIMITED_INFORMATION,
        HandleOpenType::SetInfo => PROCESS_SET_INFORMATION,
    };

    let process_handle = unsafe { OpenProcess(access, false, process_id) }?
        .require_valid(|| {
            format!(
                "Failed to open process {}. Error: {:?}",
                process_id,
                Error::last_os_error()
            )
        })?
        .to_managed();
    Ok(process_handle)
}

#[allow(dead_code)] // Used in release builds only
pub fn enable_debug_privilege() -> Result<()> {
    let mut token_handle = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES,
            &mut token_handle,
        )?;
    }
    let token_handle = token_handle.to_managed();

    let mut luid = Default::default();
    unsafe {
        LookupPrivilegeValueW(PCWSTR::null(), SE_DEBUG_NAME, &mut luid)?;
    }

    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };

    let result =
        unsafe { AdjustTokenPrivileges(*token_handle, false, Some(&privileges), 0, None, None) };
    let last_error = unsafe { GetLastError() };
    result?;

    if last_error == ERROR_NOT_ALL_ASSIGNED {
        bail!(
            "SeDebugPrivilege is not assigned to this process token. Run the application as administrator and verify the local Debug programs policy"
        );
    }

    debug_log!("SeDebugPrivilege enabled successfully");
    Ok(())
}

pub struct Process {
    pub id: u32,
    pub name: String,
}

pub fn get_running_processes() -> Result<Vec<Process>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }?
        .require_valid(|| {
            format!(
                "Failed to create process list snapshot. Error: {:?}",
                Error::last_os_error()
            )
        })?
        .to_managed();

    let mut process_entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut running_processes = vec![];

    if unsafe { Process32FirstW(*snapshot, &mut process_entry) }.is_ok() {
        loop {
            let name = get_process_name(&process_entry);
            running_processes.push(Process {
                id: process_entry.th32ProcessID,
                name,
            });
            if unsafe { Process32NextW(*snapshot, &mut process_entry) }.is_err() {
                break;
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        running_processes.sort_by_key(|a| a.name.to_lowercase());
    }
    Ok(running_processes)
}

fn get_process_name(process_entry: &PROCESSENTRY32W) -> String {
    String::from_utf16_lossy(strip_trailing_nulls(&process_entry.szExeFile))
}

pub fn get_image_path_from_handle(handle: HANDLE) -> windows::core::Result<PathBuf> {
    // Windows paths represented internally by UNICODE_STRING cannot exceed roughly 32K UTF-16 code units
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;

    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )?;
    }

    // length is the actual number of characters, excluding the null terminator
    buffer.truncate(length as usize);

    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

pub fn change_current_process_to_idle() -> Result<()> {
    let handle = unsafe { GetCurrentProcess() }.to_managed();
    enable_efficiency_mode(*handle).map_err(|err| err.into())
}

pub fn strip_trailing_nulls(slice: &[u16]) -> &[u16] {
    let stripped_len = slice.iter().position(|&e| e == 0).unwrap_or(slice.len());
    &slice[..stripped_len]
}

/// A HANDLE that automatically closes itself when dropped.
#[derive(Debug)]
pub struct ManagedHandle(HANDLE);

impl Drop for ManagedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

impl Deref for ManagedHandle {
    type Target = HANDLE;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ManagedHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub trait HandleExt {
    fn to_managed(&self) -> ManagedHandle;
    fn require_valid<F, M>(&self, error_message: F) -> Result<&Self>
    where
        F: FnOnce() -> M,
        M: Into<String>;
}

impl HandleExt for HANDLE {
    fn to_managed(&self) -> ManagedHandle {
        ManagedHandle(*self)
    }

    fn require_valid<F, M>(&self, error_message: F) -> Result<&Self>
    where
        F: FnOnce() -> M,
        M: Into<String>,
    {
        if self.is_invalid() {
            bail!(error_message().into());
        }
        Ok(self)
    }
}

/// Native PROCESSINFOCLASS value.
const PROCESS_IO_PRIORITY: u32 = 33;

#[derive(Debug, Clone, Copy, Default)]
pub enum CpuPriority {
    VeryLow,
    Low,
    #[default]
    Normal,
    High,
    VeryHigh,
}

impl CpuPriority {
    fn as_windows_flag(self) -> PROCESS_CREATION_FLAGS {
        match self {
            Self::VeryLow => IDLE_PRIORITY_CLASS,
            Self::Low => BELOW_NORMAL_PRIORITY_CLASS,
            Self::Normal => NORMAL_PRIORITY_CLASS,
            Self::High => ABOVE_NORMAL_PRIORITY_CLASS,
            Self::VeryHigh => HIGH_PRIORITY_CLASS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(u32)]
pub enum IoPriority {
    VeryLow = 0,
    Low = 1,
    #[default]
    Normal = 2,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum PowerQos {
    /// Windows chooses the QoS using its own heuristics.
    #[default]
    SystemManaged,
    /// Prefer energy-efficient cores, frequencies and scheduling.
    Eco,
    /// Explicitly request performance-oriented execution.
    High,
}

pub fn set_cpu_priority(process: HANDLE, priority: CpuPriority) -> WindowsResult<()> {
    unsafe { SetPriorityClass(process, priority.as_windows_flag()) }
}

pub fn set_io_priority(process: HANDLE, priority: IoPriority) -> io::Result<()> {
    let priority = priority as u32;

    let status = unsafe {
        NtSetInformationProcess(
            process,
            PROCESS_IO_PRIORITY,
            &priority as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        )
    };

    if status >= 0 {
        Ok(())
    } else {
        let win32_error = unsafe { RtlNtStatusToDosError(status) };
        Err(Error::from_raw_os_error(win32_error as i32))
    }
}

pub fn set_power_qos(process: HANDLE, qos: PowerQos) -> WindowsResult<()> {
    let (control_mask, state_mask) = match qos {
        PowerQos::SystemManaged => (0, 0), // Let Windows decide (default)
        PowerQos::Eco => (
            // Enable execution-speed throttling
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        ),
        PowerQos::High => (
            // Disable execution-speed throttling
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0,
        ),
    };

    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: control_mask,
        StateMask: state_mask,
    };

    unsafe {
        SetProcessInformation(
            process,
            ProcessPowerThrottling,
            &state as *const PROCESS_POWER_THROTTLING_STATE as *const c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
}

pub fn apply_process_priorities_config(
    handle: HANDLE,
    priorities_config: &AppPriorityConfig,
) -> Result<()> {
    if let Some(cpu_priority) = priorities_config.cpu {
        set_cpu_priority(handle, cpu_priority)?;
    }
    if let Some(io_priority) = priorities_config.io {
        set_io_priority(handle, io_priority)?;
    }
    if let Some(power_qos) = priorities_config.power {
        set_power_qos(handle, power_qos)?;
    }
    Ok(())
}

fn enable_efficiency_mode(process: HANDLE) -> WindowsResult<()> {
    set_cpu_priority(process, CpuPriority::VeryLow)?;
    set_io_priority(process, IoPriority::VeryLow)?;
    set_power_qos(process, PowerQos::Eco)?;
    Ok(())
}

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetInformationProcess(
        process_handle: HANDLE,
        process_information_class: u32,
        process_information: *const c_void,
        process_information_length: u32,
    ) -> i32;

    fn RtlNtStatusToDosError(status: i32) -> u32;
}
