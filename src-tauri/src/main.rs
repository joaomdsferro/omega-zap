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

    // WebKitGTK 2.54 picks GStreamer's VA-API decoders when present, and on
    // AMD (radeonsi) H.264 then fails with a decode error after a few frames:
    // the video freezes on its first frame. Chat videos are small enough for
    // software decoding, so rank the hardware decoders out unless the user
    // set their own ranks.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GST_PLUGIN_FEATURE_RANK").is_none() {
        let ranks = ["vah264dec", "vah265dec", "vavp8dec", "vavp9dec", "vaav1dec", "vampeg2dec"]
            .iter()
            .chain(&["vaapih264dec", "vaapih265dec", "vaapivp8dec", "vaapivp9dec", "vaapiav1dec", "vaapimpeg2dec"])
            .map(|decoder| format!("{decoder}:NONE"))
            .collect::<Vec<_>>()
            .join(",");
        unsafe { std::env::set_var("GST_PLUGIN_FEATURE_RANK", ranks) };
    }

    omega_zap_lib::run()
}
