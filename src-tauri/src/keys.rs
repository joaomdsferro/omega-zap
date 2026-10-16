//! The app's own keys for outside services, built in at build time so nobody
//! installing the app needs one. The same variable at run time overrides the
//! built-in one, and a development build also reads the repository's
//! gitignored `.env`, when asked, so adding a key there needs no rebuild.

/// The key named `$name`, if there is one.
macro_rules! built_in {
    ($name:literal) => {
        $crate::keys::pick($name, option_env!($name))
    };
}
pub(crate) use built_in;

pub fn pick(name: &str, built_in: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| built_in.map(str::to_string))
        .or_else(|| dev(name))
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn dev(name: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env")).ok()?;
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix(name)?.strip_prefix('=')?.trim().trim_matches(['"', '\'']);
        (!value.is_empty()).then(|| value.to_string())
    })
}
