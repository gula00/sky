#![cfg(windows)]

use super::discovery::AccessibilityInfo;
use anyhow::{Context, Result};
use std::{ffi::c_void, mem::size_of, ptr::null_mut};
use windows::{
    core::{BSTR, Interface},
    Win32::{
        Foundation::{HWND as WinHwnd, RECT, RPC_E_CHANGED_MODE},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::Accessibility::{
            CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomationElement,
            IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
            IUIAutomationScrollItemPattern, IUIAutomationSelectionItemPattern,
            IUIAutomationTextPattern, IUIAutomationTreeWalker, IUIAutomationValuePattern,
            UIA_AppBarControlTypeId, UIA_ButtonControlTypeId, UIA_CalendarControlTypeId,
            UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId, UIA_CustomControlTypeId,
            UIA_DataGridControlTypeId, UIA_DataItemControlTypeId, UIA_DocumentControlTypeId,
            UIA_EditControlTypeId, UIA_ExpandCollapsePatternId, UIA_GroupControlTypeId,
            UIA_HeaderControlTypeId, UIA_HeaderItemControlTypeId, UIA_HyperlinkControlTypeId,
            UIA_ImageControlTypeId, UIA_InvokePatternId, UIA_ListControlTypeId,
            UIA_ListItemControlTypeId, UIA_MenuBarControlTypeId, UIA_MenuControlTypeId,
            UIA_MenuItemControlTypeId, UIA_PaneControlTypeId, UIA_ProgressBarControlTypeId,
            UIA_RadioButtonControlTypeId, UIA_ScrollBarControlTypeId, UIA_ScrollItemPatternId,
            UIA_SelectionItemPatternId, UIA_SemanticZoomControlTypeId,
            UIA_SeparatorControlTypeId, UIA_SliderControlTypeId, UIA_SpinnerControlTypeId,
            UIA_SplitButtonControlTypeId, UIA_StatusBarControlTypeId, UIA_TabControlTypeId,
            UIA_TabItemControlTypeId, UIA_TableControlTypeId, UIA_TextControlTypeId,
            UIA_TextPatternId, UIA_ThumbControlTypeId, UIA_TitleBarControlTypeId,
            UIA_ToolBarControlTypeId, UIA_ToolTipControlTypeId, UIA_TreeControlTypeId,
            UIA_TreeItemControlTypeId, UIA_ValuePatternId, UIA_WindowControlTypeId,
            UIA_CONTROLTYPE_ID,
        },
    },
};
use windows_sys::Win32::{
    Foundation::{HWND as SysHwnd, RECT as SysRect},
    UI::{
        Controls::EM_GETSEL,
        WindowsAndMessaging::{
            GetGUIThreadInfo as SysGetGUIThreadInfo,
            GetWindowThreadProcessId as SysGetWindowThreadProcessId,
            SendMessageW as SysSendMessageW, GUITHREADINFO as SysGuiThreadInfo, WM_GETTEXT,
            WM_GETTEXTLENGTH,
        },
    },
};

const MAX_UIA_ELEMENTS: usize = 512;
const MAX_TEXT_LENGTH: i32 = 16_384;

pub fn capture_accessibility(window_id: i64) -> Result<AccessibilityInfo> {
    let client = UiaClient::new()?;
    let mut elements = client.collect_window_elements(window_id)?;
    if elements.is_empty() {
        anyhow::bail!("UI Automation returned no elements");
    }

    let focused_hwnd = focused_hwnd_for_window(window_id);
    if let Some(focused_hwnd) = focused_hwnd {
        for element in &mut elements {
            if element.native_hwnd == Some(focused_hwnd) {
                element.has_keyboard_focus = true;
            }
        }
    }

    let lines = elements
        .iter()
        .map(format_uia_element)
        .collect::<Vec<_>>();
    let focused_element = elements
        .iter()
        .find(|element| element.has_keyboard_focus)
        .map(format_uia_element)
        .or_else(|| client.focused_element_line(&elements).ok().flatten());
    let selected_elements = elements
        .iter()
        .filter(|element| element.is_selected)
        .map(format_uia_element)
        .collect::<Vec<_>>();
    let selected_text = elements
        .iter()
        .find_map(|element| selected_text_for_element(&element.element))
        .or_else(|| focused_hwnd.and_then(selected_text_from_hwnd));
    let document_text = document_text_for_elements(&elements);

    Ok(AccessibilityInfo {
        tree: lines.join("\n"),
        focused_element,
        selected_text,
        selected_elements,
        document_text,
    })
}

pub fn element_center(window_id: i64, element_index: usize) -> Result<Option<(i32, i32)>> {
    let client = UiaClient::new()?;
    let element = client.element_by_index(window_id, element_index)?;
    let rect = unsafe { element.CurrentBoundingRectangle() }?;
    if !valid_rect(&rect) {
        return Ok(None);
    }
    Ok(Some(((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2)))
}

pub fn invoke_element(window_id: i64, element_index: usize) -> Result<bool> {
    let client = UiaClient::new()?;
    let element = client.element_by_index(window_id, element_index)?;
    match current_pattern::<IUIAutomationInvokePattern>(&element, UIA_InvokePatternId) {
        Some(pattern) => {
            unsafe { pattern.Invoke() }?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn focus_text_or_value_element(window_id: i64, element_index: usize) -> Result<bool> {
    let client = UiaClient::new()?;
    let element = client.element_by_index(window_id, element_index)?;
    let has_value = current_pattern::<IUIAutomationValuePattern>(&element, UIA_ValuePatternId)
        .is_some();
    let has_text = current_pattern::<IUIAutomationTextPattern>(&element, UIA_TextPatternId)
        .is_some();
    if !has_value && !has_text {
        return Ok(false);
    }
    unsafe { element.SetFocus() }?;
    Ok(true)
}

pub fn set_element_value(window_id: i64, element_index: usize, value: &str) -> Result<bool> {
    let client = UiaClient::new()?;
    let element = client.element_by_index(window_id, element_index)?;
    match current_pattern::<IUIAutomationValuePattern>(&element, UIA_ValuePatternId) {
        Some(pattern) => {
            if unsafe { pattern.CurrentIsReadOnly() }?.as_bool() {
                anyhow::bail!("element_index is read-only: {element_index}");
            }
            unsafe { pattern.SetValue(&BSTR::from(value)) }?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn perform_secondary_action(window_id: i64, element_index: usize, action: &str) -> Result<bool> {
    let normalized = normalize_action(action);
    let client = UiaClient::new()?;
    let element = client.element_by_index(window_id, element_index)?;

    match normalized.as_str() {
        "invoke" | "press" | "activate" | "raise" => invoke_element(window_id, element_index),
        "expand" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationExpandCollapsePattern>(&element, UIA_ExpandCollapsePatternId)
            {
                unsafe { pattern.Expand() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "collapse" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationExpandCollapsePattern>(&element, UIA_ExpandCollapsePatternId)
            {
                unsafe { pattern.Collapse() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "select" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationSelectionItemPattern>(&element, UIA_SelectionItemPatternId)
            {
                unsafe { pattern.Select() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "add to selection" | "addtoselection" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationSelectionItemPattern>(&element, UIA_SelectionItemPatternId)
            {
                unsafe { pattern.AddToSelection() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "remove from selection" | "removefromselection" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationSelectionItemPattern>(&element, UIA_SelectionItemPatternId)
            {
                unsafe { pattern.RemoveFromSelection() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        "scroll into view" | "scrollintoview" | "scroll up" | "scroll down" | "scroll left"
        | "scroll right" => {
            if let Some(pattern) =
                current_pattern::<IUIAutomationScrollItemPattern>(&element, UIA_ScrollItemPatternId)
            {
                unsafe { pattern.ScrollIntoView() }?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
        _ => Ok(false),
    }
}

struct UiaClient {
    _com: Option<ComGuard>,
    uia: IUIAutomation,
}

impl UiaClient {
    fn new() -> Result<Self> {
        let com = initialize_com()?;
        let uia = unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .or_else(|_| unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) })
            .context("failed to create UI Automation client")?;
        Ok(Self { _com: com, uia })
    }

    fn collect_window_elements(&self, window_id: i64) -> Result<Vec<UiaElement>> {
        let root = unsafe { self.uia.ElementFromHandle(to_windows_hwnd(window_id)) }
            .context("failed to get UIA element from HWND")?;
        let walker = unsafe { self.uia.ControlViewWalker() }.context("failed to create UIA walker")?;
        let mut elements = Vec::new();
        self.collect_element(&walker, &root, 0, &mut elements);
        Ok(elements)
    }

    fn collect_element(
        &self,
        walker: &IUIAutomationTreeWalker,
        element: &IUIAutomationElement,
        depth: usize,
        elements: &mut Vec<UiaElement>,
    ) {
        if elements.len() >= MAX_UIA_ELEMENTS {
            return;
        }

        let index = elements.len();
        elements.push(snapshot_element(element, index, depth));

        let mut child = unsafe { walker.GetFirstChildElement(element) };
        while let Ok(child_element) = child {
            self.collect_element(walker, &child_element, depth + 1, elements);
            if elements.len() >= MAX_UIA_ELEMENTS {
                return;
            }
            child = unsafe { walker.GetNextSiblingElement(&child_element) };
        }
    }

    fn element_by_index(&self, window_id: i64, element_index: usize) -> Result<IUIAutomationElement> {
        let elements = self.collect_window_elements(window_id)?;
        elements
            .into_iter()
            .find(|element| element.index == element_index)
            .map(|element| element.element)
            .ok_or_else(|| anyhow::anyhow!("element_index not found: {element_index}"))
    }

    fn focused_element_line(&self, elements: &[UiaElement]) -> Result<Option<String>> {
        let focused = unsafe { self.uia.GetFocusedElement() }?;
        for element in elements {
            let same = unsafe { self.uia.CompareElements(&focused, &element.element) }?;
            if same.as_bool() {
                return Ok(Some(format_uia_element(element)));
            }
        }
        Ok(None)
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn initialize_com() -> Result<Option<ComGuard>> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_ok() {
        Ok(Some(ComGuard))
    } else if hr == RPC_E_CHANGED_MODE {
        Ok(None)
    } else {
        Err(windows::core::Error::from(hr)).context("failed to initialize COM for UI Automation")
    }
}

fn snapshot_element(element: &IUIAutomationElement, index: usize, depth: usize) -> UiaElement {
    let control_type = unsafe { element.CurrentControlType() }.ok();
    let localized_control_type = unsafe { element.CurrentLocalizedControlType() }
        .ok()
        .map(bstr_to_string)
        .filter(|value| !value.is_empty());
    let name = unsafe { element.CurrentName() }
        .ok()
        .map(bstr_to_string)
        .unwrap_or_default();
    let class_name = unsafe { element.CurrentClassName() }
        .ok()
        .map(bstr_to_string)
        .unwrap_or_default();
    let automation_id = unsafe { element.CurrentAutomationId() }
        .ok()
        .map(bstr_to_string)
        .unwrap_or_default();
    let native_hwnd = unsafe { element.CurrentNativeWindowHandle() }
        .ok()
        .map(|hwnd| hwnd.0 as isize as i64)
        .filter(|hwnd| *hwnd != 0);
    let rect = unsafe { element.CurrentBoundingRectangle() }.ok();
    let has_keyboard_focus = unsafe { element.CurrentHasKeyboardFocus() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let value = value_for_element(element);
    let is_selected = current_pattern::<IUIAutomationSelectionItemPattern>(
        element,
        UIA_SelectionItemPatternId,
    )
    .and_then(|pattern| unsafe { pattern.CurrentIsSelected() }.ok())
    .map(|value| value.as_bool())
    .unwrap_or(false);

    UiaElement {
        automation_id,
        class_name,
        control_type,
        depth,
        element: element.clone(),
        has_keyboard_focus,
        index,
        is_selected,
        localized_control_type,
        name,
        native_hwnd,
        rect,
        value,
    }
}

fn format_uia_element(element: &UiaElement) -> String {
    let control_type = element
        .localized_control_type
        .clone()
        .unwrap_or_else(|| control_type_name(element.control_type));
    let name = escape_tree_text(&element.name);
    let value = element.value.as_deref().unwrap_or("");
    let rect = element.rect.unwrap_or_default();

    let mut parts = vec![
        format!("{} {}", element.index, control_type),
        format!("\"{}\"", name),
        format!("bounds=({}, {}, {}, {})", rect.left, rect.top, rect.right, rect.bottom),
        format!("depth={}", element.depth),
    ];

    if !element.class_name.is_empty() {
        parts.push(format!("class=\"{}\"", escape_tree_text(&element.class_name)));
    }
    if !element.automation_id.is_empty() {
        parts.push(format!("automationId=\"{}\"", escape_tree_text(&element.automation_id)));
    }
    if !value.is_empty() && value != element.name {
        parts.push(format!("value=\"{}\"", escape_tree_text(value)));
    }
    if let Some(hwnd) = element.native_hwnd {
        parts.push(format!("hwnd={hwnd}"));
    }
    if element.has_keyboard_focus {
        parts.push("focused=true".to_string());
    }
    if element.is_selected {
        parts.push("selected=true".to_string());
    }

    parts.join(" ")
}

fn document_text_for_elements(elements: &[UiaElement]) -> String {
    if let Some(text) = elements.iter().find_map(|element| document_text_for_element(&element.element)) {
        return text;
    }

    elements
        .iter()
        .filter_map(|element| {
            element
                .value
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| (!element.name.trim().is_empty()).then_some(element.name.as_str()))
        })
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

fn document_text_for_element(element: &IUIAutomationElement) -> Option<String> {
    let pattern = current_pattern::<IUIAutomationTextPattern>(element, UIA_TextPatternId)?;
    let range = unsafe { pattern.DocumentRange() }.ok()?;
    let text = unsafe { range.GetText(MAX_TEXT_LENGTH) }.ok().map(bstr_to_string)?;
    (!text.trim().is_empty()).then_some(text)
}

fn selected_text_for_element(element: &IUIAutomationElement) -> Option<String> {
    let pattern = current_pattern::<IUIAutomationTextPattern>(element, UIA_TextPatternId)?;
    let selection = unsafe { pattern.GetSelection() }.ok()?;
    let len = unsafe { selection.Length() }.ok()?;
    let mut parts = Vec::new();
    for index in 0..len {
        let range = unsafe { selection.GetElement(index) }.ok()?;
        let text = unsafe { range.GetText(MAX_TEXT_LENGTH) }.ok().map(bstr_to_string)?;
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }
    (!parts.is_empty()).then_some(parts.join("\n"))
}

fn selected_text_from_hwnd(hwnd: i64) -> Option<String> {
    let hwnd = hwnd as isize as SysHwnd;
    let text_len = unsafe { SysSendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) };
    if text_len <= 0 {
        return None;
    }

    let mut start = 0_u32;
    let mut end = 0_u32;
    unsafe {
        SysSendMessageW(
            hwnd,
            EM_GETSEL,
            (&mut start as *mut u32) as usize,
            (&mut end as *mut u32) as isize,
        );
    }
    if end <= start {
        return None;
    }

    let mut buffer = vec![0_u16; text_len as usize + 1];
    let copied = unsafe {
        SysSendMessageW(
            hwnd,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
        )
    };
    if copied <= 0 {
        return None;
    }

    let text = &buffer[..copied as usize];
    let start = (start as usize).min(text.len());
    let end = (end as usize).min(text.len());
    (end > start).then(|| String::from_utf16_lossy(&text[start..end]))
        .filter(|value| !value.trim().is_empty())
}

fn value_for_element(element: &IUIAutomationElement) -> Option<String> {
    let pattern = current_pattern::<IUIAutomationValuePattern>(element, UIA_ValuePatternId)?;
    unsafe { pattern.CurrentValue() }
        .ok()
        .map(bstr_to_string)
        .filter(|value| !value.is_empty())
}

fn current_pattern<T: Interface>(element: &IUIAutomationElement, pattern_id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID) -> Option<T> {
    unsafe { element.GetCurrentPatternAs::<T>(pattern_id) }.ok()
}

fn control_type_name(control_type: Option<UIA_CONTROLTYPE_ID>) -> String {
    match control_type {
        Some(value) if value == UIA_ButtonControlTypeId => "button",
        Some(value) if value == UIA_CalendarControlTypeId => "calendar",
        Some(value) if value == UIA_CheckBoxControlTypeId => "checkbox",
        Some(value) if value == UIA_ComboBoxControlTypeId => "combobox",
        Some(value) if value == UIA_EditControlTypeId => "edit",
        Some(value) if value == UIA_HyperlinkControlTypeId => "hyperlink",
        Some(value) if value == UIA_ImageControlTypeId => "image",
        Some(value) if value == UIA_ListItemControlTypeId => "listitem",
        Some(value) if value == UIA_ListControlTypeId => "list",
        Some(value) if value == UIA_MenuControlTypeId => "menu",
        Some(value) if value == UIA_MenuBarControlTypeId => "menubar",
        Some(value) if value == UIA_MenuItemControlTypeId => "menuitem",
        Some(value) if value == UIA_ProgressBarControlTypeId => "progressbar",
        Some(value) if value == UIA_RadioButtonControlTypeId => "radiobutton",
        Some(value) if value == UIA_ScrollBarControlTypeId => "scrollbar",
        Some(value) if value == UIA_SliderControlTypeId => "slider",
        Some(value) if value == UIA_SpinnerControlTypeId => "spinner",
        Some(value) if value == UIA_StatusBarControlTypeId => "statusbar",
        Some(value) if value == UIA_TabControlTypeId => "tab",
        Some(value) if value == UIA_TabItemControlTypeId => "tabitem",
        Some(value) if value == UIA_TextControlTypeId => "text",
        Some(value) if value == UIA_ToolBarControlTypeId => "toolbar",
        Some(value) if value == UIA_ToolTipControlTypeId => "tooltip",
        Some(value) if value == UIA_TreeControlTypeId => "tree",
        Some(value) if value == UIA_TreeItemControlTypeId => "treeitem",
        Some(value) if value == UIA_CustomControlTypeId => "custom",
        Some(value) if value == UIA_GroupControlTypeId => "group",
        Some(value) if value == UIA_ThumbControlTypeId => "thumb",
        Some(value) if value == UIA_DataGridControlTypeId => "datagrid",
        Some(value) if value == UIA_DataItemControlTypeId => "dataitem",
        Some(value) if value == UIA_DocumentControlTypeId => "document",
        Some(value) if value == UIA_SplitButtonControlTypeId => "splitbutton",
        Some(value) if value == UIA_WindowControlTypeId => "window",
        Some(value) if value == UIA_PaneControlTypeId => "pane",
        Some(value) if value == UIA_HeaderControlTypeId => "header",
        Some(value) if value == UIA_HeaderItemControlTypeId => "headeritem",
        Some(value) if value == UIA_TableControlTypeId => "table",
        Some(value) if value == UIA_TitleBarControlTypeId => "titlebar",
        Some(value) if value == UIA_SeparatorControlTypeId => "separator",
        Some(value) if value == UIA_SemanticZoomControlTypeId => "semanticzoom",
        Some(value) if value == UIA_AppBarControlTypeId => "appbar",
        Some(value) => return format!("controlType({})", value.0),
        None => "element",
    }
    .to_string()
}

fn valid_rect(rect: &RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

fn focused_hwnd_for_window(window_id: i64) -> Option<i64> {
    let hwnd = window_id as isize as SysHwnd;
    let thread_id = unsafe { SysGetWindowThreadProcessId(hwnd, null_mut()) };
    if thread_id == 0 {
        return None;
    }

    let mut info = SysGuiThreadInfo {
        cbSize: size_of::<SysGuiThreadInfo>() as u32,
        flags: 0,
        hwndActive: null_mut(),
        hwndFocus: null_mut(),
        hwndCapture: null_mut(),
        hwndMenuOwner: null_mut(),
        hwndMoveSize: null_mut(),
        hwndCaret: null_mut(),
        rcCaret: SysRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    };

    if unsafe { SysGetGUIThreadInfo(thread_id, &mut info) } == 0 || info.hwndFocus.is_null() {
        None
    } else {
        Some(info.hwndFocus as isize as i64)
    }
}

fn bstr_to_string(value: BSTR) -> String {
    String::try_from(value).unwrap_or_default()
}

fn to_windows_hwnd(window_id: i64) -> WinHwnd {
    WinHwnd(window_id as isize as *mut c_void)
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

fn escape_tree_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

struct UiaElement {
    automation_id: String,
    class_name: String,
    control_type: Option<UIA_CONTROLTYPE_ID>,
    depth: usize,
    element: IUIAutomationElement,
    has_keyboard_focus: bool,
    index: usize,
    is_selected: bool,
    localized_control_type: Option<String>,
    name: String,
    native_hwnd: Option<i64>,
    rect: Option<RECT>,
    value: Option<String>,
}
