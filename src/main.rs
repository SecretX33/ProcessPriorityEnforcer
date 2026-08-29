use crate::config::{AppConfig, read_app_config};
use crate::process::{HandleOpenType, ManagedHandle, change_current_process_to_idle, change_process_to_idle, enable_debug_privilege, open_process};
use color_eyre::eyre::{Context, Result, bail};
use latches::sync::Latch;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::{collections::HashMap, time::Duration};
use wmi::{FilterValue, WMIConnection};

mod config;
mod log_macros;
mod process;
mod util;

#[derive(Debug, serde::Deserialize)]
#[serde(rename = "__InstanceCreationEvent")]
#[serde(rename_all = "PascalCase")]
struct WinProcessStart {
    target_instance: WinProcess,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename = "Win32_Process")]
#[serde(rename_all = "PascalCase")]
struct WinProcess {
    #[serde(rename = "ProcessId")]
    id: u32,
    name: String,
}

impl From<WinProcess> for process::Process {
    fn from(value: WinProcess) -> Self {
        process::Process {
            id: value.id,
            name: value.name,
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    enable_debug_privilege()?;

    log!("Starting Idle Process Enforcer...");
    let app_config = get_config()?;

    let monitor_start_latch = Arc::new(Latch::new(1));
    let join_handle = start_monitoring_processes(&app_config, monitor_start_latch.clone());
    monitor_start_latch.wait();
    update_running_processes(&app_config)?;
    change_current_process_to_idle()?;

    join_handle.join().expect("Failed to join thread");
    Ok(())
}

fn get_config() -> Result<AppConfig> {
    let Some(first_arg) = std::env::args().nth(1) else {
        bail!("Usage: processpriorityenforcer <config_path>");
    };
    let app_config_path = std::path::absolute(PathBuf::from_str(&first_arg)?)?;
    let config = read_app_config(app_config_path.as_path())?;
    Ok(config)
}

fn update_running_processes(app_config: &AppConfig) -> Result<()> {
    let running_processes = process::get_running_processes()?;

    log!("Checking for running processes...");
    let mut updated_count = 0;

    for process in &running_processes {
        let Some(OpenedProcess{ handle, executable_path}) = open_process_handle_if_matches(process, app_config) else { continue };

        log!("Found running: '{}' ({}) ({}), updating to idle priority", process.name, process.id, executable_path.display());
        if let Err(err) = change_process_to_idle(&handle) {
            log!("Failed to change priority of {} ({}) ({}): {}", process.name, process.id, executable_path.display(), err);
            continue;
        }
        updated_count += 1;
    }

    if updated_count > 0 {
        log!("Updated a total of {} running processes", updated_count);
    }
    Ok(())
}

fn start_monitoring_processes(app_config: &AppConfig, latch: Arc<Latch>) -> JoinHandle<()> {
    let app_config = app_config.clone();
    let latch = latch.clone();

    std::thread::spawn(move || {
        if let Err(err) = monitor_processes(&app_config, latch) {
            log!("Error on monitor process: {}", err);
            std::process::exit(1);
        }
    })
}

fn monitor_processes(app_config: &AppConfig, latch: Arc<Latch>) -> Result<()> {
    let connection = WMIConnection::new()?;
    let mut filters = HashMap::new();
    filters.insert("TargetInstance".to_owned(), FilterValue::is_a::<WinProcess>()?);
    let events =
        connection.filtered_notification::<WinProcessStart>(&filters, Some(Duration::from_secs(1)))
            .wrap_err("Failed to subscribe to process creation events")?
            .filter_map(|e| e.ok());

    log!("Monitoring for process creation events...");
    latch.count_down();

    for event in events {
        let process = process::Process::from(event.target_instance);
        let Some(OpenedProcess {
            handle,
            executable_path,
        }) = open_process_handle_if_matches(&process, app_config)
        else {
            continue;
        };

        log!("Started: '{}' ({}) ({}), updating to idle priority", process.name, process.id, executable_path.display());
        change_process_to_idle(&handle)?;
    }
    Ok(())
}

struct OpenedProcess {
    handle: ManagedHandle,
    executable_path: PathBuf,
}

fn open_process_handle_if_matches(
    process: &process::Process,
    app_config: &AppConfig,
) -> Option<OpenedProcess> {
    let Ok(handle) = open_process(process.id, HandleOpenType::QueryInfo)
    else { return None };

    let Ok(executable_path) = process::get_image_path_from_handle(*handle)
        .inspect_err(|err| debug_log!("Failed to get image path for process {} ({}): {}", process.name, process.id, err))
        else { return None };
    if !app_config.paths.is_match(&executable_path) {
        return None;
    }

    let handle = open_process(process.id, HandleOpenType::SetInfo)
        .inspect_err(|err| log!("Matched process, but could not open handle to set info {} ({}): {}", process.name, process.id, err))
        .ok()?;
    Some(OpenedProcess { handle, executable_path })
}
