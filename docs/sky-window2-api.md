# Sky Window2 API

This helper implements the Windows Window2-style Computer Use surface. Methods
take and return JSON-compatible values.

## Window Discovery

```ts
list_windows(): Promise<Array<Window>>;
list_apps(): Promise<Array<ListAppsApp>>;
get_window(input: GetWindowInput): Promise<Window>;
launch_app(input: LaunchAppInput): Promise<void>;
```

```ts
type Window = {
  app: string;
  id: number;
  title?: string;
};

type GetWindowInput = {
  app?: string;
  id: number;
};

type ListAppsApp = {
  displayName?: string;
  id: string;
  isRunning: boolean;
  lastUsedDate?: string;
  useCount?: number;
  windows: Array<Window>;
};

type LaunchAppInput = {
  app: string;
};
```

Process-backed app ids use the form:

```text
process:C:\Path\To\App.exe
```

## Window State

```ts
get_window_state(input: GetWindowStateInput): Promise<WindowState>;
```

```ts
type GetWindowStateInput = {
  window: Window;
  include_screenshot?: boolean;
  include_text?: boolean;
};

type WindowState = {
  accessibility?: AccessibilityState | null;
  screenshots: Array<Screenshot>;
  window: Window;
};

type AccessibilityState = {
  document_text?: string;
  focused_element?: string;
  selected_elements?: Array<string>;
  selected_text?: string;
  tree: string;
};

type Screenshot = {
  height?: number;
  id: string;
  originX?: number;
  originY?: number;
  url: string;
  width?: number;
  zIndex: number;
};
```

Screenshots are PNG data URLs. The helper tries Windows.Graphics.Capture first
and falls back to GDI when WGC is unavailable for the target.

## Input

```ts
activate_window(input: ActivateWindowInput): Promise<void>;
click(input: ClickInput): Promise<void>;
click_element(input: ClickElementInput): Promise<void>;
press_key(input: PressKeyInput): Promise<void>;
type_text(input: TypeTextInput): Promise<void>;
scroll(input: ScrollInput): Promise<void>;
drag(input: DragInput): Promise<void>;
set_value(input: SetValueInput): Promise<void>;
perform_secondary_action(input: PerformSecondaryActionInput): Promise<void>;
```

```ts
type ActivateWindowInput = {
  window: Window;
};

type ClickInput = {
  window: Window;
  element_index?: number;
  x?: number;
  y?: number;
  click_count?: number;
  mouse_button?: "left" | "right" | "middle" | "l" | "r" | "m" | 0 | 1 | 2;
  screenshotId?: string;
};

type ClickElementInput = {
  window: Window;
  element_index: number;
  click_count?: number;
  mouse_button?: "left" | "right" | "middle" | "l" | "r" | "m" | 0 | 1 | 2;
};

type PressKeyInput = {
  window: Window;
  key: string;
};

type TypeTextInput = {
  window: Window;
  text: string;
};

type ScrollInput = {
  window: Window;
  x: number;
  y: number;
  scrollX: number;
  scrollY: number;
  screenshotId?: string;
};

type DragInput = {
  window: Window;
  from_x: number;
  from_y: number;
  to_x: number;
  to_y: number;
  screenshotId?: string;
};

type SetValueInput = {
  window: Window;
  element_index: number;
  value: string;
};

type PerformSecondaryActionInput = {
  window: Window;
  element_index: number;
  action: string;
};
```

`press_key` accepts `+`-separated key chords such as:

```text
Control_L+a
Control_L+Shift_L+period
Return
Tab
KP_0
```

Common aliases such as `Ctrl`, `Control`, `Alt`, `Shift`, `period`, `comma`,
`minus`, `slash`, `Numpad_0`, and `KP_0` are accepted.

`perform_secondary_action` tries UI Automation patterns before falling back to
right-click only for explicit context-menu/right-click action labels.
