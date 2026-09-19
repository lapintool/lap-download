use crate::json_util::{read_json, write_json};
use crate::paths;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub maximized: bool,
}

pub fn load_window_state() -> WindowState {
    read_json(&paths::window_state_path())
}

fn is_normal_position(x: i32, y: i32) -> bool {
    x > -10_000 && y > -10_000 && x < 50_000 && y < 50_000
}

fn is_normal_size(width: u32, height: u32) -> bool {
    width >= 720 && height >= 480 && width <= 20_000 && height <= 20_000
}

fn decoration_delta(window: &WebviewWindow) -> (u32, u32) {
    let Ok(outer) = window.outer_size() else {
        return (0, 0);
    };
    let Ok(inner) = window.inner_size() else {
        return (0, 0);
    };
    (
        outer.width.saturating_sub(inner.width),
        outer.height.saturating_sub(inner.height),
    )
}

struct RestoredFrame {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(windows)]
fn restored_frame(window: &WebviewWindow) -> Option<RestoredFrame> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};

    let hwnd = window.hwnd().ok()?;
    let mut place = WINDOWPLACEMENT::default();
    place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    unsafe { GetWindowPlacement(hwnd, &mut place).ok()? };

    let rect = place.rcNormalPosition;
    let width = rect.right.saturating_sub(rect.left);
    let height = rect.bottom.saturating_sub(rect.top);
    if width <= 0 || height <= 0 {
        return None;
    }

    Some(RestoredFrame {
        x: rect.left,
        y: rect.top,
        width: width as u32,
        height: height as u32,
    })
}

#[cfg(not(windows))]
fn restored_frame(window: &WebviewWindow) -> Option<RestoredFrame> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(RestoredFrame {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    })
}

fn save_normal_inner_frame(window: &WebviewWindow, state: &mut WindowState) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    if is_normal_position(pos.x, pos.y) && is_normal_size(size.width, size.height) {
        state.x = Some(pos.x);
        state.y = Some(pos.y);
        state.width = Some(size.width);
        state.height = Some(size.height);
    }
}

fn save_inner_frame_from_placement(
    window: &WebviewWindow,
    state: &mut WindowState,
    frame: &RestoredFrame,
) {
    let (pad_w, pad_h) = decoration_delta(window);
    let width = frame.width.saturating_sub(pad_w);
    let height = frame.height.saturating_sub(pad_h);
    if is_normal_position(frame.x, frame.y) && is_normal_size(width, height) {
        state.x = Some(frame.x);
        state.y = Some(frame.y);
        state.width = Some(width);
        state.height = Some(height);
    }
}

pub fn save_window_state(app: &AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    let mut state = load_window_state();
    let minimized = window.is_minimized().unwrap_or(false);
    let maximized = window.is_maximized().unwrap_or(false);

    if !minimized {
        state.maximized = maximized;
    }

    if !minimized && !maximized {
        save_normal_inner_frame(&window, &mut state);
    } else if let Some(frame) = restored_frame(&window) {
        let (pad_w, pad_h) = decoration_delta(&window);
        if pad_w > 0 || pad_h > 0 {
            save_inner_frame_from_placement(&window, &mut state, &frame);
        }
    }

    write_json(&paths::window_state_path(), &state)
}

pub fn restore_window_state(window: &WebviewWindow) {
    let state = load_window_state();

    if let (Some(width), Some(height)) = (state.width, state.height) {
        if is_normal_size(width, height) {
            let _ = window.set_size(PhysicalSize::new(width, height));
        }
    }
    if let (Some(x), Some(y)) = (state.x, state.y) {
        if is_normal_position(x, y) {
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
    if state.maximized {
        let _ = window.maximize();
    }
}
