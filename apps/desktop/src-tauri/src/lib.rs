#![cfg_attr(not(feature = "native-runtime"), allow(dead_code))]

mod capture;
mod desktop_host;
mod gateway;
#[cfg(feature = "wdio-test")]
mod startup_trace;

#[cfg(feature = "native-runtime")]
use tauri::Manager;

#[cfg(all(
    feature = "native-runtime",
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
pub fn run() {
    #[cfg(feature = "wdio-test")]
    startup_trace::record_process_start();
    let builder = tauri::Builder::default();
    #[cfg(feature = "wdio-test")]
    let builder = builder
        .plugin(tauri_plugin_wdio_webdriver::init())
        .plugin(tauri_plugin_wdio::init());
    #[cfg(feature = "wdio-test")]
    let builder = builder.setup(|app| {
        if app.get_webview_window("workbench").is_none() {
            return Err("Workbench window was not created during Desktop setup".into());
        }
        startup_trace::record_window_ready();
        Ok(())
    });

    builder
        .manage(gateway::GatewayBridge::default())
        .manage(gateway::ManagedGatewayResolver::default())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let bridge = window.app_handle().state::<gateway::GatewayBridge>();
                gateway::remove_bridge_window(bridge.inner(), window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            desktop_host::desktop_fallback_cwd,
            desktop_host::desktop_platform_capabilities,
            desktop_host::floating_begin_region_picker,
            desktop_host::floating_capture_region,
            desktop_host::floating_capture_selection,
            desktop_host::floating_initial_activation,
            gateway::download_session_artifact,
            gateway::gateway_connect,
            gateway::gateway_disconnect,
            gateway::gateway_endpoint,
            gateway::gateway_send,
            desktop_host::open_thread_in_workbench
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Psychevo Desktop");
}

#[cfg(all(
    not(feature = "native-runtime"),
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
pub fn run() {
    panic!("Psychevo Desktop was built without the native-runtime feature");
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn run() {
    panic!("Psychevo Desktop is supported only on macOS, Windows, and Linux");
}
