use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn app_root() -> PathBuf {
    if let Ok(dir) = std::env::var("LAPDW_ROOT") {
        return PathBuf::from(dir);
    }

    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        if let Ok(canonical) = fs::canonicalize(&dev) {
            return canonical;
        }
        return dev;
    }

    #[cfg(not(debug_assertions))]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn config_dir() -> PathBuf {
    app_root().join("config")
}

pub fn data_dir() -> PathBuf {
    app_root().join("data")
}

pub fn webview_data_dir() -> PathBuf {
    data_dir().join("webview")
}

fn user_downloads_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(dir) = known_folder_downloads() {
            return dir;
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile).join("Downloads");
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(dir) = std::env::var("XDG_DOWNLOAD_DIR") {
            if !dir.trim().is_empty() {
                return PathBuf::from(dir);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Downloads");
        }
    }

    app_root().join("downloads")
}

#[cfg(windows)]
fn known_folder_downloads() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    unsafe {
        let pwstr = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let path = pwstr.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(pwstr.0.cast()));
        path
    }
}

/// `{User Downloads}/lapdw` — created by `ensure_layout` if missing.
pub fn downloads_dir() -> PathBuf {
    user_downloads_dir().join("lapdw")
}

pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn window_state_path() -> PathBuf {
    config_dir().join("window.json")
}

pub fn tasks_path() -> PathBuf {
    data_dir().join("tasks.json")
}

fn dir_has_json(dir: &Path) -> bool {
    fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.path().extension().and_then(|e| e.to_str()) == Some("json")
            })
        })
        .unwrap_or(false)
}

pub fn locale_dir(app: &AppHandle) -> PathBuf {
    if let Ok(dir) = std::env::var("LAPDW_LOCALE_DIR") {
        return PathBuf::from(dir);
    }

    let portable = app_root().join("locale");
    if portable.is_dir() && dir_has_json(&portable) {
        return portable;
    }

    if let Ok(resource) = app.path().resource_dir() {
        let bundled = resource.join("locale");
        if bundled.is_dir() && dir_has_json(&bundled) {
            return bundled;
        }
    }

    portable
}

pub fn seed_locale_files(app: &AppHandle) {
    let portable = app_root().join("locale");
    if fs::create_dir_all(&portable).is_err() || dir_has_json(&portable) {
        return;
    }

    let Ok(resource) = app.path().resource_dir() else {
        return;
    };
    let bundled = resource.join("locale");
    let Ok(entries) = fs::read_dir(&bundled) else {
        return;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        if from.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(name) = from.file_name() {
            let _ = fs::copy(&from, portable.join(name));
        }
    }
}

/// Strip Windows `\\?\` prefix so UI paths look like `D:\Dev\...`.
pub fn display_windows_path(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    s.to_string()
}

pub fn to_display_path(path: &Path) -> String {
    display_windows_path(&path.display().to_string())
}

pub fn ensure_layout() -> Result<(), String> {
    for dir in [
        config_dir(),
        data_dir(),
        webview_data_dir(),
        downloads_dir(),
    ] {
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }

    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_none() {
        // Safety: set before WebView creation in setup.
        unsafe {
            std::env::set_var(
                "WEBVIEW2_USER_DATA_FOLDER",
                webview_data_dir().as_os_str(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_strips_verbatim_prefix() {
        assert_eq!(display_windows_path(r"\\?\D:\Dev\foo"), r"D:\Dev\foo");
        assert_eq!(
            display_windows_path(r"\\?\UNC\server\share"),
            r"\\server\share"
        );
        assert_eq!(display_windows_path(r"D:\Dev\foo"), r"D:\Dev\foo");
    }
}
