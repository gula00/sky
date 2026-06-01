use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub app: String,
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub id: String,
    pub display_name: String,
    pub last_used_date: String,
    pub use_count: u32,
    pub is_running: bool,
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Deserialize)]
pub struct WindowStateInput {
    pub window: WindowInfo,
    #[serde(default)]
    pub include_screenshot: bool,
    #[serde(default)]
    pub include_text: bool,
}

#[derive(Debug, Serialize)]
pub struct WindowState {
    pub window: WindowInfo,
    pub screenshots: Vec<ScreenshotInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility: Option<AccessibilityInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotInfo {
    pub id: String,
    pub url: String,
    pub width: i32,
    pub height: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub z_index: i32,
}

#[derive(Debug, Serialize)]
pub struct AccessibilityInfo {
    pub tree: String,
    pub focused_element: Option<String>,
    pub selected_text: Option<String>,
    pub selected_elements: Vec<String>,
    pub document_text: String,
}

#[cfg(windows)]
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    windows_impl::list_windows()
}

#[cfg(not(windows))]
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    Ok(Vec::new())
}

pub fn list_apps() -> Result<Vec<AppInfo>> {
    let windows = list_windows()?;
    let mut grouped = BTreeMap::<String, Vec<WindowInfo>>::new();

    for window in windows {
        grouped.entry(window.app.clone()).or_default().push(window);
    }

    Ok(grouped
        .into_iter()
        .map(|(id, windows)| AppInfo {
            display_name: display_name_from_app_id(&id),
            id,
            last_used_date: "1970-01-01T00:00:00.000Z".to_string(),
            use_count: windows.len() as u32,
            is_running: true,
            windows,
        })
        .collect())
}

#[cfg(windows)]
pub fn get_window(input: WindowInfo) -> Result<WindowInfo> {
    windows_impl::get_window(input.id)
}

pub fn get_window_state(input: WindowStateInput) -> Result<WindowState> {
    let window = get_window(input.window)?;
    let accessibility = if input.include_text {
        Some(capture_accessibility(window.id)?)
    } else {
        None
    };

    let screenshots = if input.include_screenshot {
        vec![capture_window_screenshot(window.id)?]
    } else {
        Vec::new()
    };

    Ok(WindowState {
        window,
        screenshots,
        accessibility,
    })
}

#[cfg(windows)]
fn capture_accessibility(window_id: i64) -> Result<AccessibilityInfo> {
    windows_impl::capture_accessibility(window_id)
}

#[cfg(not(windows))]
fn capture_accessibility(_window_id: i64) -> Result<AccessibilityInfo> {
    Ok(AccessibilityInfo {
        tree: String::new(),
        focused_element: None,
        selected_text: None,
        selected_elements: Vec::new(),
        document_text: String::new(),
    })
}

#[cfg(windows)]
fn capture_window_screenshot(window_id: i64) -> Result<ScreenshotInfo> {
    windows_impl::capture_window_screenshot(window_id)
}

#[cfg(not(windows))]
fn capture_window_screenshot(_window_id: i64) -> Result<ScreenshotInfo> {
    anyhow::bail!("screenshot capture is only supported on Windows")
}

#[cfg(not(windows))]
pub fn get_window(input: WindowInfo) -> Result<WindowInfo> {
    list_windows()?
        .into_iter()
        .find(|window| window.id == input.id)
        .ok_or_else(|| anyhow::anyhow!("window not found: {}", input.id))
}

fn display_name_from_app_id(id: &str) -> String {
    let value = id.strip_prefix("process:").unwrap_or(id);
    let normalized = value.replace('\\', "/");
    let file_name = normalized.rsplit('/').next().unwrap_or(value);
    let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);

    if stem.is_empty() {
        id.to_string()
    } else {
        stem.to_string()
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::{AccessibilityInfo, ScreenshotInfo, WindowInfo};
    use anyhow::Result;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use image::{ImageBuffer, ImageFormat, Rgba};
    use std::{ffi::c_void, io::Cursor, mem::size_of, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT},
        Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
            GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
            DIB_RGB_COLORS, RGBQUAD, SRCCOPY,
        },
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{
            EnumChildWindows, EnumWindows, GetAncestor, GetClassNameW, GetGUIThreadInfo,
            GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsWindow,
            GetWindowThreadProcessId, IsWindowVisible, GUITHREADINFO, GA_ROOT,
        },
    };

    pub fn list_windows() -> Result<Vec<WindowInfo>> {
        let mut windows = Vec::<WindowInfo>::new();
        let lparam = &mut windows as *mut Vec<WindowInfo> as LPARAM;
        unsafe {
            EnumWindows(Some(enum_window), lparam);
        }
        Ok(windows)
    }

    pub fn get_window(id: i64) -> Result<WindowInfo> {
        let hwnd = id as isize as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            anyhow::bail!("window not found: {id}");
        }

        window_from_hwnd(hwnd).ok_or_else(|| anyhow::anyhow!("window is not targetable: {id}"))
    }

    pub fn capture_window_screenshot(id: i64) -> Result<ScreenshotInfo> {
        let hwnd = id as isize as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            anyhow::bail!("window not found: {id}");
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            anyhow::bail!("failed to read window rectangle: {id}");
        }

        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            anyhow::bail!("window has invalid screenshot bounds: {id}");
        }

        let (capture_width, capture_height, rgba) =
            match crate::windows::screenshot_wgc::capture_window(id) {
                Ok(capture) => (capture.width, capture.height, capture.rgba),
                Err(_) => (width, height, capture_desktop_region(rect.left, rect.top, width, height)?),
            };
        let url = encode_png_data_url(capture_width, capture_height, rgba)?;

        Ok(ScreenshotInfo {
            id: "screenshot-0".to_string(),
            url,
            width: capture_width,
            height: capture_height,
            origin_x: rect.left,
            origin_y: rect.top,
            z_index: 0,
        })
    }

    pub fn capture_accessibility(id: i64) -> Result<AccessibilityInfo> {
        if let Ok(accessibility) = crate::windows::uia::capture_accessibility(id) {
            return Ok(accessibility);
        }

        let hwnd = id as isize as HWND;
        if unsafe { IsWindow(hwnd) } == 0 {
            anyhow::bail!("window not found: {id}");
        }

        let mut elements = Vec::<AccessibleElement>::new();
        let root = accessible_element(hwnd, 0);
        elements.push(root);

        let lparam = &mut elements as *mut Vec<AccessibleElement> as LPARAM;
        unsafe {
            EnumChildWindows(hwnd, Some(enum_child_window), lparam);
        }

        let focused_hwnd = focused_hwnd_for_window(hwnd);
        let focused_element = focused_hwnd.and_then(|focused| {
            elements
                .iter()
                .find(|element| element.hwnd == focused as isize as i64)
                .map(format_accessible_element)
        });

        let lines = elements
            .iter()
            .map(format_accessible_element)
            .collect::<Vec<_>>();

        Ok(AccessibilityInfo {
            tree: lines.join("\n"),
            focused_element,
            selected_text: None,
            selected_elements: Vec::new(),
            document_text: String::new(),
        })
    }

    unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if let Some(window) = window_from_hwnd(hwnd) {
            let windows = unsafe { &mut *(lparam as *mut Vec<WindowInfo>) };
            windows.push(window);
        }
        1
    }

    unsafe extern "system" fn enum_child_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let elements = unsafe { &mut *(lparam as *mut Vec<AccessibleElement>) };
        let index = elements.len();
        elements.push(accessible_element(hwnd, index));
        1
    }

    fn window_from_hwnd(hwnd: HWND) -> Option<WindowInfo> {
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return None;
        }

        if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
            return None;
        }

        let title = window_title(hwnd)?;
        if title.trim().is_empty() {
            return None;
        }

        let process_id = process_id_for_window(hwnd)?;
        let app = process_app_id(process_id);
        if crate::policy::ensure_target_allowed(&app, &title).is_err() {
            return None;
        }

        Some(WindowInfo {
            app,
            id: hwnd as isize as i64,
            title,
        })
    }

    fn window_title(hwnd: HWND) -> Option<String> {
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return None;
        }

        let mut buffer = vec![0_u16; len as usize + 1];
        let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        if copied <= 0 {
            return None;
        }

        Some(String::from_utf16_lossy(&buffer[..copied as usize]))
    }

    fn window_text(hwnd: HWND) -> String {
        window_title(hwnd).unwrap_or_default()
    }

    fn class_name(hwnd: HWND) -> String {
        let mut buffer = vec![0_u16; 256];
        let copied = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
        if copied <= 0 {
            return "unknown".to_string();
        }
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    fn accessible_element(hwnd: HWND, index: usize) -> AccessibleElement {
        let rect = hwnd_rect(hwnd).unwrap_or(RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        });

        AccessibleElement {
            index,
            hwnd: hwnd as isize as i64,
            class_name: class_name(hwnd),
            name: window_text(hwnd),
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
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

    fn hwnd_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        (unsafe { GetWindowRect(hwnd, &mut rect) } != 0).then_some(rect)
    }

    fn process_id_for_window(hwnd: HWND) -> Option<u32> {
        let mut process_id = 0_u32;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process_id);
        }
        (process_id != 0).then_some(process_id)
    }

    fn process_app_id(process_id: u32) -> String {
        process_image_path(process_id)
            .map(|path| format!("process:{path}"))
            .unwrap_or_else(|| format!("process:{process_id}"))
    }

    fn process_image_path(process_id: u32) -> Option<String> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if handle == null_mut::<c_void>() {
            return None;
        }

        let mut buffer = vec![0_u16; 32768];
        let mut size = buffer.len() as u32;
        let ok = unsafe {
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size)
        };
        unsafe {
            CloseHandle(handle);
        }

        if ok == 0 || size == 0 {
            return None;
        }

        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }

    fn capture_desktop_region(x: i32, y: i32, width: i32, height: i32) -> Result<Vec<u8>> {
        let screen_dc = unsafe { GetDC(null_mut()) };
        if screen_dc == null_mut::<c_void>() {
            anyhow::bail!("failed to acquire screen device context");
        }

        let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
        if memory_dc == null_mut::<c_void>() {
            unsafe {
                ReleaseDC(null_mut(), screen_dc);
            }
            anyhow::bail!("failed to create compatible device context");
        }

        let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
        if bitmap == null_mut::<c_void>() {
            unsafe {
                DeleteDC(memory_dc);
                ReleaseDC(null_mut(), screen_dc);
            }
            anyhow::bail!("failed to create compatible bitmap");
        }

        let old_object = unsafe { SelectObject(memory_dc, bitmap) };
        let copied = unsafe {
            BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                screen_dc,
                x,
                y,
                SRCCOPY | CAPTUREBLT,
            )
        };

        if copied == 0 {
            unsafe {
                SelectObject(memory_dc, old_object);
                DeleteObject(bitmap);
                DeleteDC(memory_dc);
                ReleaseDC(null_mut(), screen_dc);
            }
            anyhow::bail!("failed to copy screen pixels");
        }

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: (width * height * 4) as u32,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };
        let mut bgra = vec![0_u8; (width * height * 4) as usize];
        let lines = unsafe {
            GetDIBits(
                memory_dc,
                bitmap,
                0,
                height as u32,
                bgra.as_mut_ptr().cast(),
                &mut bitmap_info,
                DIB_RGB_COLORS,
            )
        };

        unsafe {
            SelectObject(memory_dc, old_object);
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(null_mut(), screen_dc);
        }

        if lines == 0 {
            anyhow::bail!("failed to read bitmap pixels");
        }

        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        Ok(bgra)
    }

    fn encode_png_data_url(width: i32, height: i32, rgba: Vec<u8>) -> Result<String> {
        let image = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width as u32, height as u32, rgba)
            .ok_or_else(|| anyhow::anyhow!("failed to construct screenshot image buffer"))?;
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;
        Ok(format!("data:image/png;base64,{}", STANDARD.encode(png)))
    }

    fn escape_tree_text(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
    }

    fn format_accessible_element(element: &AccessibleElement) -> String {
        let name = escape_tree_text(&element.name);
        format!(
            "{} {} \"{}\" bounds=({}, {}, {}, {}) hwnd={}",
            element.index,
            element.class_name,
            name,
            element.left,
            element.top,
            element.right,
            element.bottom,
            element.hwnd
        )
    }

    struct AccessibleElement {
        index: usize,
        hwnd: i64,
        class_name: String,
        name: String,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
}

#[cfg(test)]
mod tests {
    use super::display_name_from_app_id;

    #[test]
    fn display_name_uses_process_file_stem() {
        assert_eq!(
            display_name_from_app_id(r"process:C:\Windows\System32\notepad.exe"),
            "notepad"
        );
    }

    #[test]
    fn display_name_falls_back_to_id() {
        assert_eq!(display_name_from_app_id("process:"), "process:");
    }
}
