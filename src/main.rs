use crate::config::{AppConfig, AppPriorityConfig, read_app_config};
use crate::process::{
    HandleOpenType, ManagedHandle, apply_process_priorities_config, change_current_process_to_idle,
    open_process,
};
use arc_swap::ArcSwap;
use color_eyre::eyre::{Context, Result, bail};
use latches::sync::Latch;
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, new_debouncer, notify};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, mpsc};
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
    #[cfg(not(debug_assertions))]
    {
        crate::process::enable_debug_privilege()?;
    }

    log!("Starting ProcessPriorityEnforcer...");
    let (app_config, app_config_path) = get_config()?;

    let monitor_start_latch = Arc::new(Latch::new(1));
    let join_handle = start_monitoring_processes(&app_config, &monitor_start_latch);
    monitor_start_latch.wait();

    update_running_processes(&app_config.load())?;
    change_current_process_to_idle()?;
    watch_config(&app_config, &app_config_path)?;

    join_handle.join().expect("Failed to join thread");
    Ok(())
}

fn get_config() -> Result<(Arc<ArcSwap<AppConfig>>, PathBuf)> {
    let Some(first_arg) = std::env::args().nth(1) else {
        bail!("Usage: processpriorityenforcer <config_path>");
    };
    let app_config_path = std::path::absolute(PathBuf::from_str(&first_arg)?)?;
    let app_config = Arc::new(ArcSwap::new(Arc::new(read_app_config(
        app_config_path.as_path(),
    )?)));

    Ok((app_config, app_config_path))
}

fn watch_config(app_config: &ArcSwap<AppConfig>, app_config_path: &Path) -> Result<()> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();
    let _debouncer = create_debouncer(app_config_path, Duration::from_millis(300), tx.clone())?;

    log!("Starting config hot-reload watcher...");

    for event_result in rx {
        match event_result {
            Ok(events) => events
                .into_iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        EventKind::Create { .. } | EventKind::Modify { .. }
                    )
                })
                .take(1) // Prevent multiple updates in a short timespan if somehow they're not debounced
                .for_each(|_| {
                    log!("Config file changed, reloading...");
                    match read_app_config(app_config_path) {
                        Ok(config) => {
                            app_config.store(Arc::new(config));
                            log!("Successfully reloaded config file");
                        }
                        Err(err) => {
                            log!(
                                "Error reading config file, skipping hot-reload... Error: {:?}",
                                err
                            );
                            return;
                        }
                    }

                    if let Err(err) = update_running_processes(&app_config.load()) {
                        log!("Error updating running processes: {:?}", err);
                    }
                }),
            Err(err) => log!("Error watching config file: {:?}", err),
        }
    }

    log!("Config hot-reload watcher thread exited, but this should never happen");
    Ok(())
}

fn create_debouncer(
    path: &Path,
    timeout: Duration,
    tx: mpsc::Sender<DebounceEventResult>,
) -> Result<Debouncer<notify::RecommendedWatcher, notify_debouncer_full::RecommendedCache>> {
    let mut debouncer = new_debouncer(timeout, None, tx.clone())?;
    debouncer
        .watch(path, RecursiveMode::NonRecursive)
        .wrap_err("Error starting file watcher")?;
    Ok(debouncer)
}

fn start_monitoring_processes(
    app_config: &Arc<ArcSwap<AppConfig>>,
    latch: &Arc<Latch>,
) -> JoinHandle<()> {
    let app_config = app_config.clone();
    let latch = latch.clone();

    std::thread::spawn(move || {
        if let Err(err) = monitor_processes(app_config, latch) {
            log!("Error on monitor process: {}", err);
            std::process::exit(1);
        }
    })
}

fn monitor_processes(app_config: Arc<ArcSwap<AppConfig>>, latch: Arc<Latch>) -> Result<()> {
    let connection = WMIConnection::new()?;
    let mut filters = HashMap::new();
    filters.insert(
        "TargetInstance".to_owned(),
        FilterValue::is_a::<WinProcess>()?,
    );
    let events = connection
        .filtered_notification::<WinProcessStart>(&filters, Some(Duration::from_secs(1)))
        .wrap_err("Failed to subscribe to process creation events")?
        .filter_map(|e| e.ok());

    log!("Monitoring for process creation events...");
    latch.count_down();

    for event in events {
        let process = process::Process::from(event.target_instance);
        let Some(OpenedProcess {
            handle,
            executable_path,
            priority_config,
        }) = open_process_handle_if_matches(&process, &app_config.load())
        else {
            continue;
        };

        log!(
            "Started: '{}' ({}) ({}), updating its priorities: {:?}",
            process.name,
            process.id,
            executable_path.display(),
            priority_config
        );
        apply_process_priorities_config(*handle, &priority_config)?;
    }
    Ok(())
}

fn update_running_processes(app_config: &AppConfig) -> Result<()> {
    let running_processes = process::get_running_processes()?;

    log!("Checking for running processes...");
    let mut updated_count = 0;

    for process in &running_processes {
        let Some(OpenedProcess {
            handle,
            executable_path,
            priority_config,
        }) = open_process_handle_if_matches(process, app_config)
        else {
            continue;
        };

        log!(
            "Found running: '{}' ({}) ({}), updating its priorities: {:?}",
            process.name,
            process.id,
            executable_path.display(),
            priority_config
        );
        if let Err(err) = apply_process_priorities_config(*handle, &priority_config) {
            log!(
                "Failed to change priority of {} ({}) ({}): {}",
                process.name,
                process.id,
                executable_path.display(),
                err
            );
            continue;
        }
        updated_count += 1;
    }

    if updated_count > 0 {
        log!("Updated a total of {} running processes", updated_count);
    }
    Ok(())
}

struct OpenedProcess {
    handle: ManagedHandle,
    executable_path: PathBuf,
    priority_config: AppPriorityConfig,
}

fn open_process_handle_if_matches(
    process: &process::Process,
    app_config: &AppConfig,
) -> Option<OpenedProcess> {
    let Ok(handle) = open_process(process.id, HandleOpenType::QueryInfo) else {
        return None;
    };

    let Ok(executable_path) = process::get_image_path_from_handle(*handle).inspect_err(|err| {
        debug_log!(
            "Failed to get image path for process {} ({}): {}",
            process.name,
            process.id,
            err
        )
    }) else {
        return None;
    };

    let priority_config = app_config
        .groups
        .iter()
        .find(|group| group.paths.is_match(&executable_path))
        .map(|e| e.priorities.clone())?;

    let handle = open_process(process.id, HandleOpenType::SetInfo)
        .inspect_err(|err| {
            log!(
                "Matched process, but could not open handle to set info {} ({}): {}",
                process.name,
                process.id,
                err
            )
        })
        .ok()?;
    Some(OpenedProcess {
        handle,
        executable_path,
        priority_config,
    })
}
