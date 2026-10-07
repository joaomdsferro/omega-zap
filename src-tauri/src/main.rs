// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK halves its frame rate to 30 fps while the desktop is on the
    // power-saver profile, which makes dragging and scrolling visibly lag. It
    // reads the profile from power-profiles-daemon on the system bus and has
    // no setting to opt out, so hide that bus from the webview processes.
    // Nothing else in the app uses the system bus. Must run before any thread
    // is spawned.
    #[cfg(target_os = "linux")]
    if std::env::var_os("OMEGA_ZAP_RESPECT_POWER_SAVER").is_none() {
        unsafe { std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", "unix:path=/nonexistent") };
    }

    omega_zap_lib::run()
}
