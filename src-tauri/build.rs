fn main() {
    klipy_key();
    tauri_build::build()
}

/// GIF and sticker search need a KLIPY key built in. It comes from the
/// `KLIPY_API_KEY` environment variable (CI sets it from a secret) or, for
/// local builds, from a `KLIPY_API_KEY=…` line in the gitignored `.env` at the
/// repository root.
fn klipy_key() {
    const FILE: &str = "../.env";
    println!("cargo:rerun-if-env-changed=KLIPY_API_KEY");
    // Only watched when it exists: a missing file would rerun this on every
    // build. Debug builds also read it when they start (`klipy::key`).
    if std::path::Path::new(FILE).exists() {
        println!("cargo:rerun-if-changed={FILE}");
    }
    if std::env::var("KLIPY_API_KEY").is_ok_and(|k| !k.trim().is_empty()) {
        return;
    }
    let found = std::fs::read_to_string(FILE).ok().and_then(|text| {
        text.lines().find_map(|line| {
            let value = line.trim().strip_prefix("KLIPY_API_KEY=")?;
            let value = value.trim().trim_matches(['"', '\'']);
            (!value.is_empty()).then(|| value.to_string())
        })
    });
    if let Some(key) = found {
        println!("cargo:rustc-env=KLIPY_API_KEY={key}");
    }
}
