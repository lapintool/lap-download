use crate::json_util::{read_json, write_json};
use crate::paths;
use serde::{Deserialize, Serialize};

fn default_threads() -> u32 {
    16
}

fn default_theme() -> String {
    "dark".into()
}

fn default_locale() -> String {
    "zh".into()
}

fn default_zoom() -> f64 {
    1.0
}

pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 2.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    /// Hugging Face access token → Authorization: Bearer
    #[serde(default)]
    pub hf_token: String,
    /// Optional generic bearer token (used when hf_token empty)
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub cookie: String,
    #[serde(default)]
    pub use_proxy: bool,
    #[serde(default = "default_proxy")]
    pub proxy: String,
    #[serde(default = "default_threads")]
    pub threads: u32,
    /// Empty → `{User Downloads}/lapdw`
    #[serde(default)]
    pub save_dir: String,
}

fn default_proxy() -> String {
    "http://127.0.0.1:6990".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            locale: default_locale(),
            zoom: default_zoom(),
            hf_token: String::new(),
            token: String::new(),
            cookie: String::new(),
            use_proxy: false,
            proxy: default_proxy(),
            threads: default_threads(),
            save_dir: String::new(),
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Self {
        self.theme = match self.theme.trim() {
            "light" => "light".into(),
            _ => "dark".into(),
        };
        self.locale = match self.locale.trim() {
            "en" => "en".into(),
            _ => "zh".into(),
        };
        self.zoom = self.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.hf_token = self.hf_token.trim().to_string();
        self.token = self.token.trim().to_string();
        self.cookie = self.cookie.trim().to_string();
        self.proxy = self.proxy.trim().to_string();
        self.save_dir = paths::display_windows_path(self.save_dir.trim());
        if self.threads == 0 {
            self.threads = 1;
        }
        if self.threads > 64 {
            self.threads = 64;
        }
        self
    }

    pub fn effective_token(&self) -> Option<&str> {
        if !self.hf_token.is_empty() {
            Some(self.hf_token.as_str())
        } else if !self.token.is_empty() {
            Some(self.token.as_str())
        } else {
            None
        }
    }

    pub fn resolve_save_dir(&self) -> std::path::PathBuf {
        if self.save_dir.is_empty() {
            paths::downloads_dir()
        } else {
            std::path::PathBuf::from(&self.save_dir)
        }
    }
}

pub fn load_settings() -> Settings {
    read_json::<Settings>(&paths::settings_path()).normalized()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    write_json(&paths::settings_path(), &settings.clone().normalized())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub zoom: Option<f64>,
    pub hf_token: Option<String>,
    pub token: Option<String>,
    pub cookie: Option<String>,
    pub use_proxy: Option<bool>,
    pub proxy: Option<String>,
    pub threads: Option<u32>,
    pub save_dir: Option<String>,
}

pub fn update_settings(patch: SettingsPatch) -> Result<Settings, String> {
    let mut settings = load_settings();
    if let Some(theme) = patch.theme {
        settings.theme = theme;
    }
    if let Some(locale) = patch.locale {
        settings.locale = locale;
    }
    if let Some(zoom) = patch.zoom {
        settings.zoom = zoom;
    }
    if let Some(hf_token) = patch.hf_token {
        settings.hf_token = hf_token;
    }
    if let Some(token) = patch.token {
        settings.token = token;
    }
    if let Some(cookie) = patch.cookie {
        settings.cookie = cookie;
    }
    if let Some(use_proxy) = patch.use_proxy {
        settings.use_proxy = use_proxy;
    }
    if let Some(proxy) = patch.proxy {
        settings.proxy = proxy;
    }
    if let Some(threads) = patch.threads {
        settings.threads = threads;
    }
    if let Some(save_dir) = patch.save_dir {
        settings.save_dir = save_dir;
    }
    let settings = settings.normalized();
    save_settings(&settings)?;
    Ok(settings)
}
