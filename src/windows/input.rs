use super::discovery::{get_window, WindowInfo};
use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Deserialize)]
pub struct WindowActionInput {
    pub window: WindowInfo,
}

#[derive(Debug, Deserialize)]
pub struct LaunchAppInput {
    pub app: String,
}

#[derive(Debug, Deserialize)]
pub struct ClickInput {
    pub window: WindowInfo,
    #[serde(default)]
    pub element_index: Option<usize>,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default = "default_click_count")]
    pub click_count: u32,
    #[serde(default)]
    pub mouse_button: Option<Value>,
    #[serde(default, rename = "screenshotId")]
    #[allow(dead_code)]
    pub screenshot_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ClickElementInput {
    pub window: WindowInfo,
    pub element_index: usize,
    #[serde(default = "default_click_count")]
    pub click_count: u32,
    #[serde(default)]
    pub mouse_button: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct PressKeyInput {
    pub window: WindowInfo,
    pub key: String,
}

#[derive(Debug, Deserialize)]
pub struct TypeTextInput {
    pub window: WindowInfo,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct SetValueInput {
    pub window: WindowInfo,
    pub element_index: usize,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct SecondaryActionInput {
    pub window: WindowInfo,
    pub element_index: usize,
    pub action: String,
}

#[derive(Debug, Deserialize)]
pub struct ScrollInput {
    pub window: WindowInfo,
    pub x: i32,
    pub y: i32,
    #[serde(default, rename = "scrollX")]
    pub scroll_x: i32,
    #[serde(default, rename = "scrollY")]
    pub scroll_y: i32,
    #[serde(default, rename = "screenshotId")]
    #[allow(dead_code)]
    pub screenshot_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DragInput {
    pub window: WindowInfo,
    pub from_x: i32,
    pub from_y: i32,
    pub to_x: i32,
    pub to_y: i32,
    #[serde(default, rename = "screenshotId")]
    #[allow(dead_code)]
    pub screenshot_id: Option<String>,
}

fn default_click_count() -> u32 {
    1
}

pub fn activate_window(input: WindowActionInput) -> Result<()> {
    let window = get_window(input.window)?;
    activate_window_by_id(window.id)
}

pub fn launch_app(input: LaunchAppInput) -> Result<()> {
    let app = input.app.trim();
    if app.is_empty() {
        anyhow::bail!("app is required");
    }

    crate::policy::ensure_app_allowed(app)?;
    let executable = app.strip_prefix("process:").unwrap_or(app);
    Command::new(executable)
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("failed to launch app {app}: {error}"))
}

pub fn click(input: ClickInput) -> Result<()> {
    if input.click_count == 0 {
        anyhow::bail!("click_count must be >= 1");
    }

    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    let mouse_button = parse_mouse_button(input.mouse_button)?;
    let (screen_x, screen_y) = if let Some(element_index) = input.element_index {
        if input.click_count == 1
            && mouse_button == MouseButton::Left
            && (invoke_element(window.id, element_index)? || focus_text_or_value_element(window.id, element_index)?)
        {
            return Ok(());
        }
        element_center(window.id, element_index)?
    } else {
        screenshot_point_to_screen(window.id, input.x, input.y)?
    };
    click_screen_point(screen_x, screen_y, mouse_button, input.click_count)
}

pub fn click_element(input: ClickElementInput) -> Result<()> {
    if input.click_count == 0 {
        anyhow::bail!("click_count must be >= 1");
    }

    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    if input.click_count == 1
        && parse_mouse_button(input.mouse_button.clone())? == MouseButton::Left
        && (invoke_element(window.id, input.element_index)?
            || focus_text_or_value_element(window.id, input.element_index)?)
    {
        return Ok(());
    }
    let (screen_x, screen_y) = element_center(window.id, input.element_index)?;
    click_screen_point(screen_x, screen_y, parse_mouse_button(input.mouse_button)?, input.click_count)
}

pub fn press_key(input: PressKeyInput) -> Result<()> {
    if input.key.trim().is_empty() {
        anyhow::bail!("key is required");
    }

    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    send_key_chord(&input.key)
}

pub fn type_text(input: TypeTextInput) -> Result<()> {
    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    if type_text_into_focused_edit(window.id, &input.text)? {
        return Ok(());
    }
    send_unicode_text(&input.text)
}

pub fn set_value(input: SetValueInput) -> Result<()> {
    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    if set_element_value(window.id, input.element_index, &input.value)? {
        return Ok(());
    }
    set_element_text(window.id, input.element_index, &input.value)
}

pub fn perform_secondary_action(input: SecondaryActionInput) -> Result<()> {
    if input.action.trim().is_empty() {
        anyhow::bail!("action is required");
    }

    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    if perform_uia_secondary_action(window.id, input.element_index, &input.action)? {
        return Ok(());
    }

    if !allows_secondary_right_click_fallback(&input.action) {
        anyhow::bail!(
            "unsupported secondary action for element_index {}: {}",
            input.element_index,
            input.action
        );
    }

    let (screen_x, screen_y) = element_center(window.id, input.element_index)?;
    click_screen_point(screen_x, screen_y, MouseButton::Right, 1)
}

pub fn scroll(input: ScrollInput) -> Result<()> {
    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    let (screen_x, screen_y) = screenshot_point_to_screen(window.id, input.x, input.y)?;
    scroll_screen_point(screen_x, screen_y, input.scroll_x, input.scroll_y)
}

pub fn drag(input: DragInput) -> Result<()> {
    let window = get_window(input.window)?;
    activate_window_by_id(window.id)?;
    let (from_x, from_y) = screenshot_point_to_screen(window.id, input.from_x, input.from_y)?;
    let (to_x, to_y) = screenshot_point_to_screen(window.id, input.to_x, input.to_y)?;
    drag_screen_points(from_x, from_y, to_x, to_y)
}

#[cfg(windows)]
fn activate_window_by_id(window_id: i64) -> Result<()> {
    windows_impl::activate_window_by_id(window_id)
}

#[cfg(not(windows))]
fn activate_window_by_id(_window_id: i64) -> Result<()> {
    anyhow::bail!("window activation is only supported on Windows")
}

#[cfg(windows)]
fn screenshot_point_to_screen(window_id: i64, x: i32, y: i32) -> Result<(i32, i32)> {
    windows_impl::screenshot_point_to_screen(window_id, x, y)
}

#[cfg(not(windows))]
fn screenshot_point_to_screen(_window_id: i64, _x: i32, _y: i32) -> Result<(i32, i32)> {
    anyhow::bail!("coordinate input is only supported on Windows")
}

#[cfg(windows)]
fn element_center(window_id: i64, element_index: usize) -> Result<(i32, i32)> {
    if let Ok(Some(point)) = crate::windows::uia::element_center(window_id, element_index) {
        return Ok(point);
    }
    windows_impl::element_center(window_id, element_index)
}

#[cfg(not(windows))]
fn element_center(_window_id: i64, _element_index: usize) -> Result<(i32, i32)> {
    anyhow::bail!("element actions are only supported on Windows")
}

#[cfg(windows)]
fn set_element_text(window_id: i64, element_index: usize, value: &str) -> Result<()> {
    windows_impl::set_element_text(window_id, element_index, value)
}

#[cfg(not(windows))]
fn set_element_text(_window_id: i64, _element_index: usize, _value: &str) -> Result<()> {
    anyhow::bail!("element actions are only supported on Windows")
}

#[cfg(windows)]
fn click_screen_point(x: i32, y: i32, button: MouseButton, click_count: u32) -> Result<()> {
    windows_impl::click_screen_point(x, y, button, click_count)
}

#[cfg(not(windows))]
fn click_screen_point(_x: i32, _y: i32, _button: MouseButton, _click_count: u32) -> Result<()> {
    anyhow::bail!("mouse input is only supported on Windows")
}

#[cfg(windows)]
fn scroll_screen_point(x: i32, y: i32, scroll_x: i32, scroll_y: i32) -> Result<()> {
    windows_impl::scroll_screen_point(x, y, scroll_x, scroll_y)
}

#[cfg(not(windows))]
fn scroll_screen_point(_x: i32, _y: i32, _scroll_x: i32, _scroll_y: i32) -> Result<()> {
    anyhow::bail!("scroll input is only supported on Windows")
}

#[cfg(windows)]
fn drag_screen_points(from_x: i32, from_y: i32, to_x: i32, to_y: i32) -> Result<()> {
    windows_impl::drag_screen_points(from_x, from_y, to_x, to_y)
}

#[cfg(not(windows))]
fn drag_screen_points(_from_x: i32, _from_y: i32, _to_x: i32, _to_y: i32) -> Result<()> {
    anyhow::bail!("drag input is only supported on Windows")
}

#[cfg(windows)]
fn send_key_chord(key: &str) -> Result<()> {
    windows_impl::send_key_chord(key)
}

#[cfg(not(windows))]
fn send_key_chord(_key: &str) -> Result<()> {
    anyhow::bail!("keyboard input is only supported on Windows")
}

#[cfg(windows)]
fn send_unicode_text(text: &str) -> Result<()> {
    windows_impl::send_unicode_text(text)
}

#[cfg(not(windows))]
fn send_unicode_text(_text: &str) -> Result<()> {
    anyhow::bail!("text input is only supported on Windows")
}

#[cfg(windows)]
fn type_text_into_focused_edit(window_id: i64, text: &str) -> Result<bool> {
    windows_impl::type_text_into_focused_edit(window_id, text)
}

#[cfg(not(windows))]
fn type_text_into_focused_edit(_window_id: i64, _text: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn invoke_element(window_id: i64, element_index: usize) -> Result<bool> {
    crate::windows::uia::invoke_element(window_id, element_index)
}

#[cfg(not(windows))]
fn invoke_element(_window_id: i64, _element_index: usize) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn set_element_value(window_id: i64, element_index: usize, value: &str) -> Result<bool> {
    crate::windows::uia::set_element_value(window_id, element_index, value)
}

#[cfg(not(windows))]
fn set_element_value(_window_id: i64, _element_index: usize, _value: &str) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn focus_text_or_value_element(window_id: i64, element_index: usize) -> Result<bool> {
    crate::windows::uia::focus_text_or_value_element(window_id, element_index)
}

#[cfg(not(windows))]
fn focus_text_or_value_element(_window_id: i64, _element_index: usize) -> Result<bool> {
    Ok(false)
}

#[cfg(windows)]
fn perform_uia_secondary_action(window_id: i64, element_index: usize, action: &str) -> Result<bool> {
    crate::windows::uia::perform_secondary_action(window_id, element_index, action)
}

#[cfg(not(windows))]
fn perform_uia_secondary_action(
    _window_id: i64,
    _element_index: usize,
    _action: &str,
) -> Result<bool> {
    Ok(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseButton {
    Left,
    Right,
    Middle,
}

fn parse_mouse_button(value: Option<Value>) -> Result<MouseButton> {
    match value {
        None => Ok(MouseButton::Left),
        Some(Value::Number(number)) => match number.as_i64() {
            Some(0) => Ok(MouseButton::Left),
            Some(1) => Ok(MouseButton::Right),
            Some(2) => Ok(MouseButton::Middle),
            _ => anyhow::bail!("mouse_button number must be 0, 1, or 2"),
        },
        Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "l" | "left" => Ok(MouseButton::Left),
            "r" | "right" => Ok(MouseButton::Right),
            "m" | "middle" => Ok(MouseButton::Middle),
            _ => anyhow::bail!("mouse_button must be left, right, middle, l, r, m, 0, 1, or 2"),
        },
        Some(_) => anyhow::bail!("mouse_button must be a string or number"),
    }
}

fn allows_secondary_right_click_fallback(action: &str) -> bool {
    matches!(
        normalize_action(action).as_str(),
        "context menu" | "contextmenu" | "right click" | "rightclick" | "secondary click"
    )
}

fn normalize_action(action: &str) -> String {
    action
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
mod windows_impl {
    use super::MouseButton;
    use anyhow::Result;
    use std::{mem::size_of, thread, time::Duration};
    use windows_sys::Win32::{
        Foundation::{BOOL, HWND, LPARAM, RECT},
        UI::{
            Controls::EM_REPLACESEL,
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
                KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
                MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
                MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
                MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT,
                VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
                VK_LEFT, VK_MENU, VK_NEXT, VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3,
                VK_NUMPAD4, VK_NUMPAD5, VK_NUMPAD6, VK_NUMPAD7, VK_NUMPAD8, VK_NUMPAD9,
                VK_OEM_2, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_PRIOR,
                VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
            },
            WindowsAndMessaging::{
                EnumChildWindows, GetClassNameW, GetGUIThreadInfo, GetSystemMetrics,
                GetWindowRect, GetWindowThreadProcessId, IsWindow, SendMessageW, SetCursorPos,
                SetForegroundWindow, ShowWindow, GUITHREADINFO, SM_CXVIRTUALSCREEN,
                SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, WM_SETTEXT,
            },
        },
    };

    const WHEEL_DELTA: i32 = 120;

    pub fn activate_window_by_id(window_id: i64) -> Result<()> {
        let hwnd = hwnd_from_id(window_id)?;
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
        thread::sleep(Duration::from_millis(60));
        Ok(())
    }

    pub fn screenshot_point_to_screen(window_id: i64, x: i32, y: i32) -> Result<(i32, i32)> {
        let rect = window_rect(window_id)?;
        Ok((rect.left + x, rect.top + y))
    }

    pub fn click_screen_point(
        x: i32,
        y: i32,
        button: MouseButton,
        click_count: u32,
    ) -> Result<()> {
        set_cursor(x, y)?;
        let (down, up) = match button {
            MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        };

        for _ in 0..click_count {
            send_mouse_event(down, 0)?;
            send_mouse_event(up, 0)?;
        }

        Ok(())
    }

    pub fn scroll_screen_point(x: i32, y: i32, scroll_x: i32, scroll_y: i32) -> Result<()> {
        set_cursor(x, y)?;
        if scroll_y != 0 {
            send_mouse_event(MOUSEEVENTF_WHEEL, (-scroll_y * WHEEL_DELTA) as u32)?;
        }
        if scroll_x != 0 {
            send_mouse_event(MOUSEEVENTF_HWHEEL, (scroll_x * WHEEL_DELTA) as u32)?;
        }
        Ok(())
    }

    pub fn drag_screen_points(from_x: i32, from_y: i32, to_x: i32, to_y: i32) -> Result<()> {
        set_cursor(from_x, from_y)?;
        send_mouse_event(MOUSEEVENTF_LEFTDOWN, 0)?;
        thread::sleep(Duration::from_millis(40));
        set_cursor(to_x, to_y)?;
        thread::sleep(Duration::from_millis(40));
        send_mouse_event(MOUSEEVENTF_LEFTUP, 0)?;
        Ok(())
    }

    pub fn element_center(window_id: i64, element_index: usize) -> Result<(i32, i32)> {
        let hwnd = element_hwnd(window_id, element_index)?;
        let rect = hwnd_rect(hwnd)?;
        Ok(((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2))
    }

    pub fn set_element_text(window_id: i64, element_index: usize, value: &str) -> Result<()> {
        let hwnd = element_hwnd(window_id, element_index)?;
        let mut wide = value.encode_utf16().collect::<Vec<_>>();
        wide.push(0);
        let result = unsafe { SendMessageW(hwnd, WM_SETTEXT, 0, wide.as_ptr() as isize) };
        if result == 0 {
            anyhow::bail!("failed to set element text");
        }
        Ok(())
    }

    pub fn send_key_chord(key: &str) -> Result<()> {
        let parts = key
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            anyhow::bail!("key is required");
        }

        let mut virtual_keys = Vec::new();
        for part in parts {
            virtual_keys.push(parse_virtual_key(part)?);
        }

        for vk in &virtual_keys {
            send_key(*vk, false)?;
        }
        for vk in virtual_keys.iter().rev() {
            send_key(*vk, true)?;
        }
        Ok(())
    }

    pub fn send_unicode_text(text: &str) -> Result<()> {
        for unit in text.encode_utf16() {
            send_unicode_unit(unit, false)?;
            send_unicode_unit(unit, true)?;
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    pub fn type_text_into_focused_edit(window_id: i64, text: &str) -> Result<bool> {
        let hwnd = hwnd_from_id(window_id)?;
        let Some(focused) = focused_hwnd_for_window(hwnd) else {
            return Ok(false);
        };
        if !is_edit_class(focused) {
            return Ok(false);
        }

        let mut wide = text.encode_utf16().collect::<Vec<_>>();
        wide.push(0);
        unsafe {
            SendMessageW(focused, EM_REPLACESEL, 1, wide.as_ptr() as isize);
        }
        Ok(true)
    }

    fn hwnd_from_id(window_id: i64) -> Result<HWND> {
        let hwnd = window_id as isize as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            anyhow::bail!("window not found: {window_id}");
        }
        Ok(hwnd)
    }

    fn window_rect(window_id: i64) -> Result<RECT> {
        let hwnd = hwnd_from_id(window_id)?;
        hwnd_rect(hwnd)
    }

    fn hwnd_rect(hwnd: HWND) -> Result<RECT> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            anyhow::bail!("failed to read window rectangle");
        }
        Ok(rect)
    }

    fn element_hwnd(window_id: i64, element_index: usize) -> Result<HWND> {
        let root = hwnd_from_id(window_id)?;
        if element_index == 0 {
            return Ok(root);
        }

        let mut state = ElementSearch {
            target_index: element_index,
            current_index: 0,
            found: std::ptr::null_mut(),
        };
        unsafe {
            EnumChildWindows(root, Some(enum_child_for_index), &mut state as *mut ElementSearch as LPARAM);
        }

        if state.found.is_null() {
            anyhow::bail!("element_index not found: {element_index}");
        }
        Ok(state.found)
    }

    fn focused_hwnd_for_window(hwnd: HWND) -> Option<HWND> {
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        if thread_id == 0 {
            return None;
        }

        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            flags: 0,
            hwndActive: std::ptr::null_mut(),
            hwndFocus: std::ptr::null_mut(),
            hwndCapture: std::ptr::null_mut(),
            hwndMenuOwner: std::ptr::null_mut(),
            hwndMoveSize: std::ptr::null_mut(),
            hwndCaret: std::ptr::null_mut(),
            rcCaret: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
        };

        if unsafe { GetGUIThreadInfo(thread_id, &mut info) } == 0 || info.hwndFocus.is_null() {
            None
        } else {
            Some(info.hwndFocus)
        }
    }

    fn is_edit_class(hwnd: HWND) -> bool {
        let mut buffer = vec![0_u16; 256];
        let copied = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        if copied <= 0 {
            return false;
        }
        let class_name = String::from_utf16_lossy(&buffer[..copied as usize]).to_ascii_lowercase();
        class_name.contains("edit")
    }

    unsafe extern "system" fn enum_child_for_index(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam as *mut ElementSearch) };
        state.current_index += 1;
        if state.current_index == state.target_index {
            state.found = hwnd;
            return 0;
        }
        1
    }

    fn set_cursor(x: i32, y: i32) -> Result<()> {
        if unsafe { SetCursorPos(x, y) } != 0 {
            return Ok(());
        }
        send_absolute_mouse_move(x, y)
            .map_err(|error| anyhow::anyhow!("failed to set cursor position: ({x}, {y}): {error}"))
    }

    fn send_absolute_mouse_move(x: i32, y: i32) -> Result<()> {
        let virtual_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let virtual_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let virtual_width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1);
        let virtual_height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1);
        let normalized_x = ((x - virtual_x) * 65535) / virtual_width;
        let normalized_y = ((y - virtual_y) * 65535) / virtual_height;
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: normalized_x,
                    dy: normalized_y,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_inputs(&[input])
    }

    fn send_mouse_event(flags: u32, mouse_data: u32) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: mouse_data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_inputs(&[input])
    }

    fn send_key(vk: VIRTUAL_KEY, key_up: bool) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if key_up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_inputs(&[input])
    }

    fn send_unicode_unit(unit: u16, key_up: bool) -> Result<()> {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | if key_up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        send_inputs(&[input])
    }

    fn send_inputs(inputs: &[INPUT]) -> Result<()> {
        let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
        if sent != inputs.len() as u32 {
            anyhow::bail!("SendInput sent {sent} of {} events", inputs.len());
        }
        Ok(())
    }

    fn parse_virtual_key(key: &str) -> Result<VIRTUAL_KEY> {
        let normalized = key.trim().to_ascii_lowercase();
        let vk = match normalized.as_str() {
            "ctrl" | "control" | "control_l" | "control_r" | "ctrl_l" | "ctrl_r" => VK_CONTROL,
            "shift" | "shift_l" | "shift_r" => VK_SHIFT,
            "alt" | "option" | "alt_l" | "alt_r" | "menu" => VK_MENU,
            "enter" | "return" => VK_RETURN,
            "tab" => VK_TAB,
            "escape" | "esc" => VK_ESCAPE,
            "space" => VK_SPACE,
            "delete" | "del" => VK_DELETE,
            "period" | "dot" => VK_OEM_PERIOD,
            "comma" => VK_OEM_COMMA,
            "minus" | "hyphen" => VK_OEM_MINUS,
            "plus" | "equal" | "equals" => VK_OEM_PLUS,
            "slash" => VK_OEM_2,
            "left" => VK_LEFT,
            "right" => VK_RIGHT,
            "up" => VK_UP,
            "down" => VK_DOWN,
            "home" => VK_HOME,
            "end" => VK_END,
            "pageup" | "page_up" => VK_PRIOR,
            "pagedown" | "page_down" => VK_NEXT,
            "kp_0" | "numpad_0" | "numpad0" => VK_NUMPAD0,
            "kp_1" | "numpad_1" | "numpad1" => VK_NUMPAD1,
            "kp_2" | "numpad_2" | "numpad2" => VK_NUMPAD2,
            "kp_3" | "numpad_3" | "numpad3" => VK_NUMPAD3,
            "kp_4" | "numpad_4" | "numpad4" => VK_NUMPAD4,
            "kp_5" | "numpad_5" | "numpad5" => VK_NUMPAD5,
            "kp_6" | "numpad_6" | "numpad6" => VK_NUMPAD6,
            "kp_7" | "numpad_7" | "numpad7" => VK_NUMPAD7,
            "kp_8" | "numpad_8" | "numpad8" => VK_NUMPAD8,
            "kp_9" | "numpad_9" | "numpad9" => VK_NUMPAD9,
            value if value.len() == 1 => {
                let ch = value.as_bytes()[0];
                match ch {
                    b'a'..=b'z' => (ch.to_ascii_uppercase()) as u16,
                    b'0'..=b'9' => ch as u16,
                    _ => anyhow::bail!("unsupported key: {key}"),
                }
            }
            value if value.starts_with('f') => {
                let number = value[1..]
                    .parse::<u16>()
                    .map_err(|_| anyhow::anyhow!("unsupported key: {key}"))?;
                if !(1..=24).contains(&number) {
                    anyhow::bail!("unsupported key: {key}");
                }
                0x70 + number - 1
            }
            _ => anyhow::bail!("unsupported key: {key}"),
        };
        Ok(vk)
    }

    struct ElementSearch {
        target_index: usize,
        current_index: usize,
        found: HWND,
    }
}

#[cfg(test)]
mod tests {
    use super::{allows_secondary_right_click_fallback, parse_mouse_button, MouseButton};
    use serde_json::json;

    #[test]
    fn parses_mouse_buttons() {
        assert_eq!(parse_mouse_button(None).unwrap(), MouseButton::Left);
        assert_eq!(parse_mouse_button(Some(json!("right"))).unwrap(), MouseButton::Right);
        assert_eq!(parse_mouse_button(Some(json!(2))).unwrap(), MouseButton::Middle);
    }

    #[test]
    fn rejects_bad_mouse_button() {
        assert!(parse_mouse_button(Some(json!("bad"))).is_err());
    }

    #[test]
    fn classifies_right_click_secondary_fallbacks() {
        assert!(allows_secondary_right_click_fallback("context menu"));
        assert!(allows_secondary_right_click_fallback("Right-Click"));
        assert!(!allows_secondary_right_click_fallback("definitely unsupported"));
    }
}
