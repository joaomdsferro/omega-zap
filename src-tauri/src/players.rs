//! Lets the video players shown in the chat play with sound.
//!
//! WebKitGTK only lets a page play sound on its own when it is told to, and
//! Tauri never tells it, so a player started from the chat's play button
//! would come up muted or not at all. The rule is set when the app's page
//! loads, and WebKit only takes it then, as the answer to that load. Tauri
//! answers first, so the answer is given from a hook that runs before it.
//!
//! TikTok's player, once unmuted, sends the sound through Web Audio, which
//! WebKitGTK plays as silence for a video from another site: half a second
//! of sound, then none. Without Web Audio it plays the video's own sound.

use std::sync::OnceLock;

use webkit2gtk::glib::translate::{FromGlibPtrBorrow, IntoGlib};
use webkit2gtk::glib::{self, StaticType, gobject_ffi};
use webkit2gtk::{
    AutoplayPolicy, NavigationPolicyDecision, NavigationPolicyDecisionExt, PolicyDecisionExt, PolicyDecisionType,
    URIRequestExt, UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime, WebView,
    WebViewExt, WebsitePolicies,
};

/// Where the app's own page is served from.
static APP_URL: OnceLock<String> = OnceLock::new();

/// Lets the page at `app_url` play sound without a click. Once, before the
/// window is made.
pub fn allow_autoplay(app_url: String) {
    if APP_URL.set(app_url).is_err() {
        return;
    }
    // A class has its signals once it is set up, which is when the first
    // view is made. No view is yet, so it is set up here, and kept.
    std::mem::forget(glib::Class::<WebView>::from_type(WebView::static_type()));
    let Some(signal) = glib::subclass::signal::SignalId::lookup("decide-policy", WebView::static_type()) else {
        log::warn!("videos in the chat may play muted: WebKit has no decide-policy signal");
        return;
    };
    unsafe {
        gobject_ffi::g_signal_add_emission_hook(signal.into_glib(), 0, Some(hook), std::ptr::null_mut(), None);
    }
}

/// Keeps TikTok's player off Web Audio, in a window's page.
pub fn fix_tiktok(view: &WebView) {
    let Some(content) = view.user_content_manager() else {
        return;
    };
    content.add_script(&UserScript::new(
        "window.AudioContext = undefined; window.webkitAudioContext = undefined;",
        UserContentInjectedFrames::AllFrames,
        UserScriptInjectionTime::Start,
        &["https://www.tiktok.com/player/*"],
        &[],
    ));
}

unsafe extern "C" fn hook(
    _hint: *mut gobject_ffi::GSignalInvocationHint,
    count: u32,
    values: *const gobject_ffi::GValue,
    _data: glib::ffi::gpointer,
) -> glib::ffi::gboolean {
    // The view, the decision, and what kind it is.
    if count == 3 {
        let values = unsafe { std::slice::from_raw_parts(values, 3) };
        let value = |i: usize| unsafe { glib::Value::from_glib_borrow(&values[i] as *const _) };
        if value(2).get::<PolicyDecisionType>() == Ok(PolicyDecisionType::NavigationAction)
            && let Ok(decision) = value(1).get::<NavigationPolicyDecision>()
            && is_app(&decision)
        {
            let policies = WebsitePolicies::builder().autoplay(AutoplayPolicy::Allow).build();
            // Tauri's answer after this one does nothing: a decision is taken once.
            decision.use_with_policies(&policies);
        }
    }
    // Stays for the next page load.
    glib::ffi::GTRUE
}

/// Whether the load is the app's own page, never a site inside it.
fn is_app(decision: &NavigationPolicyDecision) -> bool {
    let (Some(app), Some(uri)) = (
        APP_URL.get(),
        decision.navigation_action().and_then(|action| action.request()).and_then(|request| request.uri()),
    ) else {
        return false;
    };
    uri.starts_with(app.as_str())
}
