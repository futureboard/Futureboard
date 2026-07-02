//! Awaitable session-runtime shutdown — unloads plugins, shuts down bridge
//! hosts, and reports progress for Loading Session / project-switch UI.

use std::time::{Duration, Instant};

use crate::components::progress_dialog::ProgressBarValue;
use crate::layout::plugin_bridge_runtime::SharedPluginBridgeRuntime;

const PLUGIN_UNLOAD_WAIT: Duration = Duration::from_millis(400);
const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);
const STEP_TRANSPORT_TIMEOUT: Duration = Duration::from_secs(5);
const STEP_AUTOSAVE_TIMEOUT: Duration = Duration::from_secs(30);
const STEP_AUDIO_TIMEOUT: Duration = Duration::from_secs(10);
const STEP_PLUGIN_UNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const STEP_HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const STEP_WORKERS_TIMEOUT: Duration = Duration::from_secs(5);
const STEP_RESOURCES_TIMEOUT: Duration = Duration::from_secs(15);

macro_rules! lifecycle_log {
    ($($arg:tt)*) => {
        eprintln!("[ProjectLifecycle] {}", format!($($arg)*))
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionShutdownReason {
    ProjectClose,
    ProjectSwitch,
    ProjectReplace,
    AppExit,
}

impl SessionShutdownReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectClose => "project_close",
            Self::ProjectSwitch => "project_switch",
            Self::ProjectReplace => "project_replace",
            Self::AppExit => "app_exit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTransitionPhase {
    ClosingSession,
    LoadingSession,
    SwitchingSession,
}

/// Ordered shutdown steps surfaced in the loading dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLifecycleStep {
    StopTransport,
    FlushAutosave,
    StopAudioEngine,
    UnloadPlugins,
    TerminatePluginHosts,
    StopWorkers,
    CloseFileWatchers,
    ReleaseProjectResources,
    ClearSessionState,
}

impl SessionLifecycleStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::StopTransport => "Stopping transport",
            Self::FlushAutosave => "Flushing autosave",
            Self::StopAudioEngine => "Stopping audio engine",
            Self::UnloadPlugins => "Unloading plugins",
            Self::TerminatePluginHosts => "Terminating plugin hosts",
            Self::StopWorkers => "Stopping background workers",
            Self::CloseFileWatchers => "Closing file watchers",
            Self::ReleaseProjectResources => "Releasing project resources",
            Self::ClearSessionState => "Clearing session state",
        }
    }

    pub fn timeout(self) -> Duration {
        match self {
            Self::StopTransport => STEP_TRANSPORT_TIMEOUT,
            Self::FlushAutosave => STEP_AUTOSAVE_TIMEOUT,
            Self::StopAudioEngine => STEP_AUDIO_TIMEOUT,
            Self::UnloadPlugins => STEP_PLUGIN_UNLOAD_TIMEOUT,
            Self::TerminatePluginHosts => STEP_HOST_SHUTDOWN_TIMEOUT,
            Self::StopWorkers => STEP_WORKERS_TIMEOUT,
            Self::CloseFileWatchers => STEP_WORKERS_TIMEOUT,
            Self::ReleaseProjectResources => STEP_RESOURCES_TIMEOUT,
            Self::ClearSessionState => STEP_RESOURCES_TIMEOUT,
        }
    }

    pub fn progress_base(self) -> f32 {
        match self {
            Self::StopTransport => 0.02,
            Self::FlushAutosave => 0.08,
            Self::StopAudioEngine => 0.14,
            Self::UnloadPlugins => 0.2,
            Self::TerminatePluginHosts => 0.72,
            Self::StopWorkers => 0.82,
            Self::CloseFileWatchers => 0.86,
            Self::ReleaseProjectResources => 0.9,
            Self::ClearSessionState => 0.96,
        }
    }
}

pub const BACKGROUND_SHUTDOWN_STEPS: &[SessionLifecycleStep] = &[
    SessionLifecycleStep::UnloadPlugins,
    SessionLifecycleStep::TerminatePluginHosts,
];

pub const UI_SHUTDOWN_STEPS: &[SessionLifecycleStep] = &[
    SessionLifecycleStep::StopTransport,
    SessionLifecycleStep::FlushAutosave,
    SessionLifecycleStep::StopAudioEngine,
    SessionLifecycleStep::StopWorkers,
    SessionLifecycleStep::CloseFileWatchers,
];

pub const POST_SHUTDOWN_UI_STEPS: &[SessionLifecycleStep] =
    &[SessionLifecycleStep::ReleaseProjectResources];

#[derive(Debug, Clone)]
pub struct SessionTransitionProgress {
    pub phase: SessionTransitionPhase,
    pub title: String,
    pub stage: String,
    pub detail: Option<String>,
    pub current: Option<usize>,
    pub total: Option<usize>,
    pub bar: ProgressBarValue,
}

impl SessionTransitionProgress {
    pub fn indeterminate(
        phase: SessionTransitionPhase,
        title: impl Into<String>,
        stage: impl Into<String>,
    ) -> Self {
        Self {
            phase,
            title: title.into(),
            stage: stage.into(),
            detail: None,
            current: None,
            total: None,
            bar: ProgressBarValue::Indeterminate,
        }
    }

    pub fn indexed(
        phase: SessionTransitionPhase,
        title: impl Into<String>,
        stage: impl Into<String>,
        current: usize,
        total: usize,
        base: f32,
        span: f32,
    ) -> Self {
        let total = total.max(1);
        let p = base + span * (current as f32 / total as f32);
        Self {
            phase,
            title: title.into(),
            stage: stage.into(),
            detail: None,
            current: Some(current),
            total: Some(total),
            bar: ProgressBarValue::value(p.clamp(0.0, 1.0)),
        }
    }

    pub fn step(
        phase: SessionTransitionPhase,
        title: impl Into<String>,
        step: SessionLifecycleStep,
    ) -> Self {
        Self {
            phase,
            title: title.into(),
            stage: step.label().to_string(),
            detail: None,
            current: None,
            total: None,
            bar: ProgressBarValue::Indeterminate,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginUnloadTarget {
    pub track_id: String,
    pub insert_id: String,
    pub display_name: String,
    pub track_name: String,
    pub is_instrument: bool,
}

#[derive(Clone)]
pub struct SessionShutdownSnapshot {
    pub reason: SessionShutdownReason,
    pub plugin_targets: Vec<PluginUnloadTarget>,
    pub(crate) bridge_runtime: Option<SharedPluginBridgeRuntime>,
    pub instrument_track_ids: Vec<String>,
    pub flush_autosave_path: Option<std::path::PathBuf>,
    pub flush_autosave_project: Option<crate::project::FutureboardProject>,
}

#[derive(Debug, Clone)]
pub struct SessionShutdownError {
    pub step: SessionLifecycleStep,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SessionShutdownReport {
    pub plugins_unloaded: usize,
    pub plugins_failed: usize,
    pub hosts_shutdown: usize,
    pub hosts_killed: usize,
    pub warnings: Vec<String>,
}

pub fn run_session_shutdown(
    mut snapshot: SessionShutdownSnapshot,
    mut progress: impl FnMut(SessionTransitionProgress),
) -> Result<SessionShutdownReport, SessionShutdownError> {
    let phase = transition_phase(snapshot.reason);
    let title = transition_title(snapshot.reason);

    lifecycle_log!("begin reason={}", snapshot.reason.label());
    eprintln!("[SessionShutdown] begin reason={}", snapshot.reason.label());

    let mut report = SessionShutdownReport::default();

    for step in BACKGROUND_SHUTDOWN_STEPS {
        lifecycle_log!("step begin: {}", step.label());
        progress(SessionTransitionProgress::step(phase, title, *step));
        let started = Instant::now();
        let step_result =
            run_background_shutdown_step(*step, &mut snapshot, &mut report, &mut |p| {
                progress(p);
            });
        let elapsed = started.elapsed();
        match step_result {
            Ok(()) => lifecycle_log!("step complete: {} ({elapsed:?})", step.label()),
            Err(error) => {
                lifecycle_log!(
                    "step failed: {} ({elapsed:?}) error={}",
                    step.label(),
                    error.message
                );
                return Err(error);
            }
        }
    }

    let remaining =
        SpherePluginHost::plugin_host_lifecycle::BridgeHostManager::global().host_count();
    lifecycle_log!("complete remaining_hosts={remaining}");
    eprintln!("[SessionShutdown] complete remaining_hosts={remaining}");

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Session closed",
    ));

    if remaining > 0 {
        report.warnings.push(format!(
            "{remaining} plugin host process(es) still tracked after shutdown"
        ));
    }

    Ok(report)
}

fn transition_phase(reason: SessionShutdownReason) -> SessionTransitionPhase {
    match reason {
        SessionShutdownReason::ProjectSwitch | SessionShutdownReason::ProjectReplace => {
            SessionTransitionPhase::SwitchingSession
        }
        _ => SessionTransitionPhase::ClosingSession,
    }
}

fn transition_title(reason: SessionShutdownReason) -> &'static str {
    match reason {
        SessionShutdownReason::ProjectSwitch | SessionShutdownReason::ProjectReplace => {
            "Switching Project…"
        }
        _ => "Closing Session…",
    }
}

fn run_background_shutdown_step(
    step: SessionLifecycleStep,
    snapshot: &mut SessionShutdownSnapshot,
    report: &mut SessionShutdownReport,
    progress: &mut dyn FnMut(SessionTransitionProgress),
) -> Result<(), SessionShutdownError> {
    progress(SessionTransitionProgress::step(
        transition_phase(snapshot.reason),
        transition_title(snapshot.reason),
        step,
    ));
    let started = Instant::now();
    let result = match step {
        SessionLifecycleStep::UnloadPlugins => unload_plugins(snapshot, report, progress),
        SessionLifecycleStep::TerminatePluginHosts => {
            terminate_plugin_hosts(snapshot, report, progress)
        }
        _ => Ok(()),
    };
    if started.elapsed() > step.timeout() {
        return Err(SessionShutdownError {
            step,
            message: format!("{} timed out after {:?}", step.label(), step.timeout()),
        });
    }
    result
}

fn unload_plugins(
    snapshot: &mut SessionShutdownSnapshot,
    report: &mut SessionShutdownReport,
    mut progress: impl FnMut(SessionTransitionProgress),
) -> Result<(), SessionShutdownError> {
    let phase = transition_phase(snapshot.reason);
    let title = transition_title(snapshot.reason);
    let plugin_total = snapshot.plugin_targets.len().max(1);

    progress(SessionTransitionProgress::indeterminate(
        phase,
        title,
        "Closing plugin editors",
    ));
    eprintln!("[SessionShutdown] close plugin editors");

    for (index, target) in snapshot.plugin_targets.iter().enumerate() {
        let current = index + 1;
        progress(SessionTransitionProgress::indexed(
            phase,
            title,
            format!(
                "Unloading plugins {}/{}: {} on {}",
                current,
                snapshot.plugin_targets.len().max(1),
                target.display_name,
                target.track_name
            ),
            current,
            plugin_total,
            0.2,
            0.5,
        ));
        eprintln!(
            "[PluginUnload] unloading plugin {}/{} name={} instance_id={}",
            current,
            snapshot.plugin_targets.len(),
            target.display_name,
            target.insert_id
        );

        if let Some(runtime) = snapshot.bridge_runtime.as_ref() {
            match unload_bridge_plugin(runtime, &target.insert_id) {
                Ok(()) => {
                    report.plugins_unloaded += 1;
                    eprintln!("[PluginUnload] unloaded instance_id={}", target.insert_id);
                }
                Err(warning) => {
                    report.plugins_failed += 1;
                    report.warnings.push(warning);
                }
            }
        } else {
            report.plugins_unloaded += 1;
        }
    }

    Ok(())
}

fn terminate_plugin_hosts(
    snapshot: &mut SessionShutdownSnapshot,
    report: &mut SessionShutdownReport,
    mut progress: impl FnMut(SessionTransitionProgress),
) -> Result<(), SessionShutdownError> {
    let phase = transition_phase(snapshot.reason);
    let title = transition_title(snapshot.reason);

    if let Some(runtime) = snapshot.bridge_runtime.take() {
        let host_pid = runtime.lock().ok().and_then(|bridge| bridge.host_pid());
        progress(SessionTransitionProgress::indeterminate(
            phase,
            title,
            if let Some(pid) = host_pid {
                format!("Shutting down plugin host pid={pid}")
            } else {
                "Shutting down plugin hosts".to_string()
            },
        ));
        let host_report = crate::layout::plugin_bridge_runtime::shutdown_bridge_runtime(
            Some(runtime),
            HOST_SHUTDOWN_TIMEOUT,
            |stage, bar| {
                progress(SessionTransitionProgress {
                    phase,
                    title: title.to_string(),
                    stage,
                    detail: None,
                    current: None,
                    total: None,
                    bar,
                });
            },
        );
        report.hosts_shutdown += host_report.hosts_shutdown;
        report.hosts_killed += host_report.hosts_killed;
        report.warnings.extend(host_report.warnings);
    }

    Ok(())
}

fn unload_bridge_plugin(
    runtime: &SharedPluginBridgeRuntime,
    insert_id: &str,
) -> Result<(), String> {
    let Ok(mut bridge) = runtime.lock() else {
        return Err(format!("bridge lock poisoned for instance={insert_id}"));
    };
    if !bridge.is_loaded(insert_id) {
        return Ok(());
    }
    bridge.unload_plugin(insert_id.to_string());
    let deadline = Instant::now() + PLUGIN_UNLOAD_WAIT;
    while Instant::now() < deadline {
        bridge.drain_events();
        if !bridge.is_loaded(insert_id) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    if bridge.is_loaded(insert_id) {
        return Err(format!("plugin unload timed out instance_id={insert_id}"));
    }
    Ok(())
}

/// Flush autosave payload on a background thread with timeout.
pub fn flush_autosave_blocking(
    path: std::path::PathBuf,
    mut project: crate::project::FutureboardProject,
    timeout: Duration,
) -> Result<(), String> {
    lifecycle_log!("flush autosave path={}", path.display());
    let started = Instant::now();
    let path_for_log = path.clone();
    let result = std::thread::scope(|scope| {
        let handle = scope.spawn(move || crate::project::io::save_project(&mut project, &path));
        while !handle.is_finished() {
            if started.elapsed() >= timeout {
                return Err(format!("autosave flush timed out after {:?}", timeout));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        handle
            .join()
            .map_err(|_| "autosave worker panicked".to_string())?
            .map_err(|error| error.technical_detail())
    });
    match &result {
        Ok(()) => lifecycle_log!("flush autosave complete path={}", path_for_log.display()),
        Err(error) => lifecycle_log!(
            "flush autosave failed path={} error={error}",
            path_for_log.display()
        ),
    }
    result
}
