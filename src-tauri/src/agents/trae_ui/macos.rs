use std::{
    ffi::{c_char, c_double, c_void, CString},
    path::Path,
    process::Command,
    ptr, thread,
    time::{Duration, Instant},
};

use crate::{
    domain::{AppResult, CommandError},
    services::endpoint_url,
};

use super::{
    cached_custom_model_catalog, cached_model_names, endpoint_path, protocol_label, TraeModelInput,
    TraeUiSnapshot,
};
use crate::agents::{trae::TraeKind, AgentDetection};

type CfTypeRef = *const c_void;
type CfStringRef = *const c_void;
type CfArrayRef = *const c_void;
type AxUiElementRef = *const c_void;
type AxValueRef = *const c_void;
type CgEventRef = *const c_void;

const AX_SUCCESS: i32 = 0;
const UTF8_ENCODING: u32 = 0x0800_0100;
const AX_VALUE_CG_POINT: i32 = 1;
const AX_VALUE_CG_SIZE: i32 = 2;
const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
const CG_EVENT_MOUSE_MOVED: u32 = 5;
const CG_HID_EVENT_TAP: u32 = 0;
const CG_COMMAND_FLAG: u64 = 1 << 20;
const MODEL_SELECTOR_LOAD_TIMEOUT: Duration = Duration::from_secs(15);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CgPoint {
    x: c_double,
    y: c_double,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CgSize {
    width: c_double,
    height: c_double,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: *mut CfTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AxUiElementRef,
        attribute: CfStringRef,
        value: CfTypeRef,
    ) -> i32;
    fn AXUIElementCopyActionNames(element: AxUiElementRef, names: *mut CfArrayRef) -> i32;
    fn AXUIElementPerformAction(element: AxUiElementRef, action: CfStringRef) -> i32;
    fn AXValueGetValue(value: AxValueRef, value_type: i32, output: *mut c_void) -> bool;
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        mouse_type: u32,
        position: CgPoint,
        button: u32,
    ) -> CgEventRef;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> CgEventRef;
    fn CGEventSetFlags(event: CgEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CgEventRef);
    static kCFBooleanTrue: CfTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        value: *const c_char,
        encoding: u32,
    ) -> CfStringRef;
    fn CFStringGetLength(value: CfStringRef) -> isize;
    fn CFStringGetCString(
        value: CfStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFGetTypeID(value: CfTypeRef) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFArrayGetCount(array: CfArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CfArrayRef, index: isize) -> *const c_void;
    fn CFRetain(value: CfTypeRef) -> CfTypeRef;
    fn CFRelease(value: CfTypeRef);
}

struct OwnedCf(CfTypeRef);

impl OwnedCf {
    fn new(value: CfTypeRef) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        // Safety: every OwnedCf is created from a Create/Copy result or an
        // explicitly retained array member, and is released exactly once.
        unsafe { CFRelease(self.0) }
    }
}

struct AxElement(OwnedCf);

impl Clone for AxElement {
    fn clone(&self) -> Self {
        // Safety: retaining a live Core Foundation object produces another
        // balanced ownership reference.
        let retained = unsafe { CFRetain(self.0 .0) };
        Self(OwnedCf(retained))
    }
}

impl AxElement {
    fn application(pid: i32) -> AppResult<Self> {
        // Safety: AXUIElementCreateApplication accepts any process identifier
        // and returns a retained reference or null.
        OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) })
            .map(Self)
            .ok_or_else(ui_unavailable)
    }

    fn raw(&self) -> AxUiElementRef {
        self.0 .0
    }

    fn attribute(&self, name: &str) -> Option<OwnedCf> {
        let name = cf_string(name)?;
        let mut value = ptr::null();
        // Safety: the element and attribute references are valid for the call;
        // successful Copy transfers one retained reference to the caller.
        let result = unsafe { AXUIElementCopyAttributeValue(self.raw(), name.0, &mut value) };
        (result == AX_SUCCESS)
            .then(|| OwnedCf::new(value))
            .flatten()
    }

    fn string(&self, name: &str) -> String {
        self.attribute(name)
            .and_then(|value| cf_string_value(value.0))
            .unwrap_or_default()
    }

    fn role(&self) -> String {
        self.string("AXRole")
    }

    fn children(&self) -> Vec<Self> {
        let Some(array) = self.attribute("AXChildren") else {
            return Vec::new();
        };
        // Safety: array is a live CFArray. Members are retained before the
        // array owner is dropped so each returned element stays valid.
        let count = unsafe { CFArrayGetCount(array.0 as CfArrayRef) };
        (0..count)
            .filter_map(|index| {
                let value = unsafe { CFArrayGetValueAtIndex(array.0 as CfArrayRef, index) };
                (!value.is_null()).then(|| {
                    let retained = unsafe { CFRetain(value) };
                    Self(OwnedCf(retained))
                })
            })
            .collect()
    }

    fn parent(&self) -> Option<Self> {
        self.attribute("AXParent").map(Self)
    }

    fn action_names(&self) -> Vec<String> {
        let mut array = ptr::null();
        // Safety: successful Copy returns a retained CFArray.
        if unsafe { AXUIElementCopyActionNames(self.raw(), &mut array) } != AX_SUCCESS {
            return Vec::new();
        }
        let Some(array) = OwnedCf::new(array) else {
            return Vec::new();
        };
        let count = unsafe { CFArrayGetCount(array.0 as CfArrayRef) };
        (0..count)
            .filter_map(|index| {
                let value = unsafe { CFArrayGetValueAtIndex(array.0 as CfArrayRef, index) };
                cf_string_value(value)
            })
            .collect()
    }

    fn press(&self) -> bool {
        let Some(action) = cf_string("AXPress") else {
            return false;
        };
        // Safety: both references are live for the duration of the call.
        unsafe { AXUIElementPerformAction(self.raw(), action.0) == AX_SUCCESS }
    }

    fn press_self_or_parent(&self) -> bool {
        let mut current = Some(self.clone());
        for _ in 0..5 {
            let Some(element) = current else {
                break;
            };
            if element
                .action_names()
                .iter()
                .any(|action| action == "AXPress")
                && element.press()
            {
                return true;
            }
            current = element.parent();
        }
        false
    }

    fn set_value(&self, value: &str) -> AppResult<()> {
        let attribute = cf_string("AXValue").ok_or_else(ui_unavailable)?;
        let value = cf_string(value).ok_or_else(|| {
            CommandError::new("trae_ui_value_invalid", "Trae 模型字段包含无效字符")
        })?;
        // Safety: the element, attribute and CFString are live and the AX API
        // copies the value synchronously.
        let result = unsafe { AXUIElementSetAttributeValue(self.raw(), attribute.0, value.0) };
        if result == AX_SUCCESS {
            Ok(())
        } else {
            Err(
                CommandError::new("trae_ui_field_unwritable", "Trae 模型表单字段无法写入")
                    .with_recovery("请关闭遮挡 Trae 的系统弹窗后重试。"),
            )
        }
    }

    fn point(&self, attribute: &str, value_type: i32) -> Option<(f64, f64)> {
        let value = self.attribute(attribute)?;
        if value_type == AX_VALUE_CG_POINT {
            let mut point = CgPoint::default();
            // Safety: the copied AXValue and output storage match CGPoint.
            unsafe {
                AXValueGetValue(
                    value.0 as AxValueRef,
                    value_type,
                    (&mut point as *mut CgPoint).cast(),
                )
            }
            .then_some((point.x, point.y))
        } else {
            let mut size = CgSize::default();
            // Safety: the copied AXValue and output storage match CGSize.
            unsafe {
                AXValueGetValue(
                    value.0 as AxValueRef,
                    value_type,
                    (&mut size as *mut CgSize).cast(),
                )
            }
            .then_some((size.width, size.height))
        }
    }

    fn hover(&self) -> AppResult<()> {
        let (x, y) = self
            .point("AXPosition", AX_VALUE_CG_POINT)
            .ok_or_else(ui_unavailable)?;
        let (width, height) = self
            .point("AXSize", AX_VALUE_CG_SIZE)
            .ok_or_else(ui_unavailable)?;
        post_mouse(
            CG_EVENT_MOUSE_MOVED,
            CgPoint {
                x: x + width / 2.0,
                y: y + height / 2.0,
            },
        );
        Ok(())
    }

    fn click_center(&self) -> bool {
        let Some((x, y)) = self.point("AXPosition", AX_VALUE_CG_POINT) else {
            return false;
        };
        let Some((width, height)) = self.point("AXSize", AX_VALUE_CG_SIZE) else {
            return false;
        };
        click(CgPoint {
            x: x + width / 2.0,
            y: y + height / 2.0,
        });
        true
    }

    fn descendant_model_name(
        &self,
        model_names: &std::collections::HashSet<String>,
    ) -> Option<String> {
        let mut stack = self.children();
        let mut visited = 0_usize;
        while let Some(element) = stack.pop() {
            visited += 1;
            if visited > 64 {
                break;
            }
            for attribute in ["AXValue", "AXTitle", "AXDescription"] {
                let value = element.string(attribute);
                if model_names.contains(&value) {
                    return Some(value);
                }
            }
            stack.extend(element.children());
        }
        None
    }

    fn click_model_label(&self, model_names: &std::collections::HashSet<String>) -> bool {
        let mut stack = self.children();
        let mut visited = 0_usize;
        while let Some(element) = stack.pop() {
            visited += 1;
            if visited > 64 {
                break;
            }
            if ["AXValue", "AXTitle", "AXDescription"]
                .into_iter()
                .any(|attribute| model_names.contains(&element.string(attribute)))
                && element.click_center()
            {
                return true;
            }
            stack.extend(element.children());
        }
        false
    }
}

struct UiSession {
    root: AxElement,
}

impl UiSession {
    fn new(detection: &AgentDetection, activate: bool) -> AppResult<Self> {
        if !unsafe { AXIsProcessTrusted() } {
            return Err(CommandError::new(
                "trae_accessibility_permission_required",
                "macOS 尚未向当前 AT-Switch 版本授予辅助功能权限",
            )
            .with_recovery(
                "若设置中已经开启，请先关闭再重新开启 AT-Switch 权限，然后完全退出并重新打开 AT-Switch。",
            ));
        }
        let installation = detection
            .installation
            .as_ref()
            .ok_or_else(|| CommandError::new("agent_not_installed", "未检测到 Trae 应用"))?;
        let pid = running_pid(&installation.path)?;
        if activate {
            let status = Command::new("/usr/bin/open")
                .arg("-a")
                .arg(&installation.path)
                .status()?;
            if !status.success() {
                return Err(ui_unavailable());
            }
            thread::sleep(Duration::from_millis(250));
        }
        let root = AxElement::application(pid)?;
        let attribute = cf_string("AXManualAccessibility").ok_or_else(ui_unavailable)?;
        // Safety: setting Chromium's documented manual accessibility flag uses
        // a process-scoped AX attribute and does not modify application files.
        unsafe {
            AXUIElementSetAttributeValue(root.raw(), attribute.0, kCFBooleanTrue);
        }
        wait_until(Duration::from_secs(3), || {
            (!root.children().is_empty()).then_some(())
        })
        .ok_or_else(ui_unavailable)?;
        Ok(Self { root })
    }

    fn matching<F>(&self, predicate: F) -> Vec<AxElement>
    where
        F: Fn(&AxElement) -> bool,
    {
        let mut found = Vec::new();
        let mut stack = vec![self.root.clone()];
        let mut visited = 0_usize;
        while let Some(element) = stack.pop() {
            visited += 1;
            if visited > 20_000 {
                break;
            }
            if predicate(&element) {
                found.push(element.clone());
            }
            let mut children = element.children();
            children.reverse();
            stack.extend(children);
        }
        found
    }

    fn exact(&self, value: &str) -> Vec<AxElement> {
        self.matching(|element| {
            element.string("AXTitle") == value
                || element.string("AXValue") == value
                || element.string("AXDescription") == value
        })
    }

    fn press_text(&self, alternatives: &[&str]) -> AppResult<()> {
        for label in alternatives {
            for element in self.exact(label).into_iter().rev() {
                if element.press_self_or_parent() || element.click_center() {
                    return Ok(());
                }
            }
        }
        Err(CommandError::new(
            "trae_ui_control_missing",
            format!("Trae 当前界面未找到“{}”", alternatives[0]),
        )
        .with_recovery("请保持 Trae 主窗口打开，不要停留在系统权限或登录弹窗。"))
    }

    fn click_text(&self, alternatives: &[&str]) -> AppResult<()> {
        for label in alternatives {
            for element in self.exact(label).into_iter().rev() {
                if element.click_center() {
                    return Ok(());
                }
            }
        }
        Err(CommandError::new(
            "trae_ui_control_missing",
            format!("Trae 当前界面未找到“{}”", alternatives[0]),
        )
        .with_recovery("请保持 Trae 主窗口打开，不要停留在系统权限或登录弹窗。"))
    }

    fn press_button(&self, alternatives: &[&str]) -> AppResult<()> {
        for label in alternatives {
            let buttons = self.matching(|element| {
                element.role() == "AXButton"
                    && (element.string("AXTitle") == *label
                        || element.string("AXDescription") == *label)
            });
            if let Some(button) = buttons.into_iter().next_back() {
                if button.press() {
                    return Ok(());
                }
            }
        }
        self.press_text(alternatives)
    }

    fn has_exact(&self, value: &str) -> bool {
        !self.exact(value).is_empty()
    }

    fn first_role(&self, role: &str) -> Option<AxElement> {
        self.matching(|element| element.role() == role)
            .into_iter()
            .next()
    }

    fn model_combo(&self, model_names: &std::collections::HashSet<String>) -> Option<AxElement> {
        let combo = self
            .matching(|element| {
                if element.role() != "AXComboBox" {
                    return false;
                }
                let value = element.string("AXValue");
                value == "Auto"
                    || value == "Auto Mode"
                    || model_names.contains(&value)
                    || element.descendant_model_name(model_names).is_some()
            })
            .into_iter()
            .next();
        combo.or_else(|| {
            if self.has_exact("添加模型") || self.has_exact("Add model") {
                return None;
            }
            self.matching(|element| {
                element.role() == "AXComboBox" && !element.string("AXValue").trim().is_empty()
            })
            .into_iter()
            .next()
        })
    }

    fn combo_selection(
        &self,
        combo: &AxElement,
        model_names: &std::collections::HashSet<String>,
    ) -> Option<String> {
        combo.descendant_model_name(model_names).or_else(|| {
            let value = combo.string("AXValue");
            (!value.trim().is_empty()).then_some(value)
        })
    }

    fn close_model_settings(&self) -> bool {
        for label in ["添加模型", "Add model"] {
            for marker in self.exact(label).into_iter().rev() {
                let mut current = marker.parent();
                for _ in 0..4 {
                    let Some(container) = current else {
                        break;
                    };
                    let close = container.children().into_iter().find(|element| {
                        element.role() == "AXButton"
                            && element.string("AXTitle").is_empty()
                            && element.string("AXDescription").is_empty()
                            && element
                                .action_names()
                                .iter()
                                .any(|action| action == "AXPress")
                    });
                    if close.is_some_and(|button| button.press()) {
                        return true;
                    }
                    current = container.parent();
                }
            }
        }
        false
    }

    fn close_code_settings(&self) -> bool {
        if self.has_exact("返回应用") || self.has_exact("Back to app") {
            return self.click_text(&["返回应用", "Back to app"]).is_ok();
        }
        send_key(13, CG_COMMAND_FLAG).is_ok()
    }

    fn open_settings_drawer(&self) -> bool {
        let window = self
            .matching(|element| element.role() == "AXWindow")
            .into_iter()
            .next();
        let Some(window) = window else {
            return false;
        };
        let Some((window_x, window_y)) = window.point("AXPosition", AX_VALUE_CG_POINT) else {
            return false;
        };
        let Some((window_width, window_height)) = window.point("AXSize", AX_VALUE_CG_SIZE) else {
            return false;
        };
        if window_width <= 0.0 || window_height <= 0.0 {
            return false;
        }

        self.matching(|element| {
            if element.role() != "AXButton"
                || !element.string("AXTitle").is_empty()
                || !element.string("AXDescription").is_empty()
            {
                return false;
            }
            let Some((x, y)) = element.point("AXPosition", AX_VALUE_CG_POINT) else {
                return false;
            };
            let Some((width, height)) = element.point("AXSize", AX_VALUE_CG_SIZE) else {
                return false;
            };
            let center_x = (x + width / 2.0 - window_x) / window_width;
            let center_y = (y + height / 2.0 - window_y) / window_height;
            (0.55..=0.80).contains(&center_x)
                && (0.07..=0.20).contains(&center_y)
                && width <= 96.0
                && height <= 96.0
        })
        .into_iter()
        .next()
        .is_some_and(|button| button.press() || button.click_center())
    }

    fn dismiss_transient_overlays(&self) {
        for _ in 0..3 {
            let button = self
                .matching(|element| {
                    element.role() == "AXButton"
                        && (matches!(element.string("AXTitle").as_str(), "Close" | "关闭")
                            || matches!(element.string("AXDescription").as_str(), "Close" | "关闭"))
                })
                .into_iter()
                .next_back();
            if !button.is_some_and(|button| button.press() || button.click_center()) {
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }
    }

    fn field_with_placeholder(&self, fragments: &[&str]) -> Option<AxElement> {
        self.matching(|element| {
            let placeholder = element.string("AXPlaceholderValue");
            fragments
                .iter()
                .any(|fragment| placeholder.contains(fragment))
        })
        .into_iter()
        .next_back()
    }

    fn row_named(&self, display_name: &str) -> Option<AxElement> {
        self.exact(display_name).into_iter().find_map(|element| {
            let mut current = Some(element);
            for _ in 0..5 {
                let candidate = current?;
                if candidate.role() == "AXRow" {
                    return Some(candidate);
                }
                current = candidate.parent();
            }
            None
        })
    }
}

pub(super) fn snapshot(
    kind: TraeKind,
    detection: &AgentDetection,
    activate: bool,
) -> AppResult<TraeUiSnapshot> {
    let mut session = UiSession::new(detection, activate)?;
    let model_names = cached_model_names(detection.config_path.as_deref())?;
    if kind == TraeKind::Work && (session.has_exact("添加模型") || session.has_exact("Add model"))
    {
        if !session.close_model_settings() {
            return Err(ui_unavailable());
        }
        session = UiSession::new(detection, false)?;
    }
    if kind == TraeKind::Code && (session.has_exact("返回应用") || session.has_exact("Back to app"))
    {
        session.close_code_settings();
        session = UiSession::new(detection, false)?;
    }
    let mut selection = wait_until(MODEL_SELECTOR_LOAD_TIMEOUT, || {
        let combo = session.model_combo(&model_names)?;
        session.combo_selection(&combo, &model_names)
    });
    if selection.is_none() {
        session.dismiss_transient_overlays();
        let returned_to_chat = match kind {
            TraeKind::Work if session.has_exact("添加模型") || session.has_exact("Add model") => {
                session.close_model_settings()
            }
            TraeKind::Code => session.close_code_settings(),
            _ => false,
        };
        if returned_to_chat {
            selection = wait_until(MODEL_SELECTOR_LOAD_TIMEOUT, || {
                session = UiSession::new(detection, false).ok()?;
                let combo = session.model_combo(&model_names)?;
                session.combo_selection(&combo, &model_names)
            });
        }
    }
    let selection = selection.ok_or_else(|| {
        CommandError::new(
            "trae_model_selector_missing",
            "Trae 已运行，但当前模型选择器尚未加载完成",
        )
        .with_recovery("请确认 Trae 已登录且主窗口可用后重试。")
    })?;
    let catalog = cached_custom_model_catalog(detection.config_path.as_deref())?;
    Ok(TraeUiSnapshot {
        selection,
        custom_models: catalog.names,
        custom_models_by_id: catalog.names_by_id,
        custom_endpoints_by_name: catalog.endpoints_by_name,
    })
}

pub(super) fn add_model(
    kind: TraeKind,
    detection: &AgentDetection,
    input: &TraeModelInput,
) -> AppResult<()> {
    open_model_settings(kind, detection)?;
    let session = UiSession::new(detection, true)?;
    session.press_button(&["添加模型", "Add model"])?;
    let session = refreshed(
        detection,
        &["自定义模型", "Custom model"],
        Duration::from_secs(3),
    )?;
    session.press_text(&["自定义模型", "Custom model"])?;
    let session = refreshed(
        detection,
        &["API 格式", "API format"],
        Duration::from_secs(3),
    )?;
    let labels = [
        "OpenAI Chat Completions 格式",
        "OpenAI Responses API 格式",
        "Anthropic Messages 格式",
    ];
    if let Some(current) = labels.iter().find(|label| session.has_exact(label)) {
        if *current != protocol_label(input.protocol) {
            session.press_text(&[current])?;
            let session = refreshed(
                detection,
                &[protocol_label(input.protocol)],
                Duration::from_secs(3),
            )?;
            session.press_text(&[protocol_label(input.protocol)])?;
        }
    }
    let session = refreshed(detection, &["模型 ID", "Model ID"], Duration::from_secs(3))?;
    let full_url = session.matching(|element| {
        let value = element.string("AXValue");
        value.contains("完整请求 URL") || value.contains("full request URL")
    });
    let endpoint = if full_url.is_empty() {
        input.base_url.trim_end_matches('/').to_owned()
    } else {
        endpoint_url(&input.base_url, endpoint_path(input.protocol))?.to_string()
    };
    session
        .field_with_placeholder(&["api.openai.com", "anthropic.com"])
        .ok_or_else(ui_unavailable)?
        .set_value(&endpoint)?;
    session
        .field_with_placeholder(&["模型 ID", "Model ID"])
        .ok_or_else(ui_unavailable)?
        .set_value(&input.model_id)?;
    session
        .field_with_placeholder(&["展示名称", "display name", "Display name"])
        .ok_or_else(ui_unavailable)?
        .set_value(&input.display_name)?;
    session
        .field_with_placeholder(&["API Key", "API key"])
        .or_else(|| session.first_role("AXSecureTextField"))
        .ok_or_else(ui_unavailable)?
        .set_value(&input.credential)?;
    session.press_button(&["添加模型", "Add model"])?;

    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(300));
        let session = UiSession::new(detection, false)?;
        if session.row_named(&input.display_name).is_some() {
            return Ok(());
        }
        if session.has_exact("添加模型") || session.has_exact("Add model") {
            let failed = session.matching(|element| {
                let value = element.string("AXValue");
                value.contains("失败") || value.to_ascii_lowercase().contains("failed")
            });
            if !failed.is_empty() {
                return Err(CommandError::new(
                    "trae_connectivity_check_failed",
                    "Trae 自定义模型连通性测试失败",
                )
                .with_recovery("请检查 Provider 地址、协议、模型 ID 和 API Key 后重试。"));
            }
        }
    }
    Err(CommandError::new(
        "trae_connectivity_check_timeout",
        "等待 Trae 自定义模型连通性测试超时",
    )
    .with_recovery("请在 Trae 中查看连通性提示，确认网络后重新切换。"))
}

pub(super) fn select_model(
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<()> {
    let _ = send_key(53, 0);
    let mut model_names = cached_model_names(detection.config_path.as_deref())?;
    for candidate in selection_candidates(display_name) {
        model_names.insert(candidate.to_owned());
    }
    let session = UiSession::new(detection, true)?;
    session.dismiss_transient_overlays();
    if session.has_exact("添加模型") || session.has_exact("Add model") {
        match kind {
            TraeKind::Work if !session.close_model_settings() => return Err(ui_unavailable()),
            TraeKind::Code if !session.close_code_settings() => return Err(ui_unavailable()),
            _ => {}
        }
    }
    let mut ready = wait_until(MODEL_SELECTOR_LOAD_TIMEOUT, || {
        let current = UiSession::new(detection, false).ok()?;
        current
            .model_combo(&model_names)
            .map(|combo| (current, combo))
    });
    if ready.is_none() && kind == TraeKind::Code {
        // TraeCode renders settings in an editor tab. Close that tab
        // after adding or deleting a model before addressing the chat
        // model selector; otherwise unrelated settings comboboxes can be
        // mistaken for the selector.
        let current = UiSession::new(detection, false)?;
        if !current.close_code_settings() {
            return Err(ui_unavailable());
        }
        ready = wait_until(MODEL_SELECTOR_LOAD_TIMEOUT, || {
            let session = UiSession::new(detection, false).ok()?;
            session
                .model_combo(&model_names)
                .map(|combo| (session, combo))
        });
    }
    let (session, combo) = ready.ok_or_else(|| {
        CommandError::new("trae_model_selector_missing", "未找到 Trae 模型选择器")
    })?;
    if session
        .combo_selection(&combo, &model_names)
        .as_deref()
        .is_some_and(|selection| selections_match(selection, display_name))
    {
        return Ok(());
    }
    // Electron can acknowledge AXPress without opening the model menu. Click
    // the visible label first, then verify that the requested menu item exists.
    let candidates = selection_candidates(display_name);
    let session = open_menu_with_retry(
        || combo.click_model_label(&model_names) || combo.click_center(),
        |timeout| refreshed(detection, &candidates, timeout).ok(),
        || {
            combo.click_model_label(&model_names)
                || combo.click_center()
                || combo.press_self_or_parent()
        },
    )
    .or_else(|| {
        // The first physical click can only raise Trae's window. Reacquire
        // the live Electron element before the final attempt so that a stale
        // accessibility reference cannot turn a recoverable focus change into
        // a failed model switch.
        let current = UiSession::new(detection, true).ok()?;
        let combo = current.model_combo(&model_names)?;
        (combo.click_model_label(&model_names)
            || combo.click_center()
            || combo.press_self_or_parent())
        .then(|| refreshed(detection, &candidates, Duration::from_secs(5)).ok())
        .flatten()
    })
    .ok_or_else(|| {
        CommandError::new("trae_model_menu_missing", "Trae 模型菜单未打开")
            .with_recovery("请保持 Trae 主窗口可见并关闭遮挡弹窗，然后重新切换。")
    })?;
    session.press_text(&candidates)?;
    let verified = refreshed(detection, &candidates, Duration::from_secs(3))?;
    if verified
        .model_combo(&model_names)
        .and_then(|combo| verified.combo_selection(&combo, &model_names))
        .is_some_and(|selection| selections_match(&selection, display_name))
    {
        return Ok(());
    }

    // Electron can report AXPress as successful while leaving this popup
    // unchanged. Retry the exact item with a physical center click, then
    // perform the same selector reread before accepting the switch.
    let mut retry = UiSession::new(detection, false)?;
    if !candidates
        .iter()
        .any(|candidate| retry.has_exact(candidate))
    {
        let combo = retry.model_combo(&model_names).ok_or_else(|| {
            CommandError::new("trae_model_selector_missing", "未找到 Trae 模型选择器")
        })?;
        if !combo.click_center() && !combo.press_self_or_parent() {
            return Err(ui_unavailable());
        }
        retry = refreshed(detection, &candidates, Duration::from_secs(3))?;
    }
    retry.click_text(&candidates)?;
    let verified = refreshed(detection, &candidates, Duration::from_secs(3))?;
    if verified
        .model_combo(&model_names)
        .and_then(|combo| verified.combo_selection(&combo, &model_names))
        .is_some_and(|selection| selections_match(&selection, display_name))
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "trae_model_selection_failed",
            "Trae 未确认新的模型选择",
        ))
    }
}

fn selection_candidates(selection: &str) -> Vec<&str> {
    if matches!(selection, "Auto" | "Auto Mode") {
        vec!["Auto", "Auto Mode"]
    } else {
        vec![selection]
    }
}

fn open_menu_with_retry<T>(
    mut primary: impl FnMut() -> bool,
    mut visible: impl FnMut(Duration) -> Option<T>,
    mut fallback: impl FnMut() -> bool,
) -> Option<T> {
    if primary() {
        if let Some(menu) = visible(Duration::from_secs(2)) {
            return Some(menu);
        }
    }
    fallback()
        .then(|| visible(Duration::from_secs(3)))
        .flatten()
}

fn selections_match(actual: &str, expected: &str) -> bool {
    actual == expected
        || (matches!(actual, "Auto" | "Auto Mode") && matches!(expected, "Auto" | "Auto Mode"))
}

pub(super) fn delete_model(
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<()> {
    open_model_settings(kind, detection)?;
    let session = UiSession::new(detection, true)?;
    let Some(row) = session.row_named(display_name) else {
        return Ok(());
    };
    let operation_cell = row
        .children()
        .into_iter()
        .last()
        .ok_or_else(ui_unavailable)?;
    operation_cell.hover()?;
    thread::sleep(Duration::from_millis(250));
    let actions = operation_cell
        .children()
        .into_iter()
        .filter(|element| {
            element
                .action_names()
                .iter()
                .any(|action| action == "AXPress")
        })
        .collect::<Vec<_>>();
    let delete = actions.get(1).ok_or_else(|| {
        CommandError::new(
            "trae_model_delete_control_missing",
            "未找到 Trae 自定义模型删除按钮",
        )
    })?;
    if !delete.press() {
        return Err(ui_unavailable());
    }
    let session = refreshed(detection, &["删除", "Delete"], Duration::from_secs(3))?;
    session.press_button(&["删除", "Delete"])?;
    if wait_until(Duration::from_secs(5), || {
        let session = UiSession::new(detection, false).ok()?;
        session.row_named(display_name).is_none().then_some(())
    })
    .is_some()
    {
        Ok(())
    } else {
        Err(CommandError::new(
            "trae_model_delete_failed",
            "Trae 未确认删除 AT-Switch 管理模型",
        ))
    }
}

fn open_model_settings(kind: TraeKind, detection: &AgentDetection) -> AppResult<()> {
    let session = UiSession::new(detection, true)?;
    session.dismiss_transient_overlays();
    if session.has_exact("添加模型") || session.has_exact("Add model") {
        return Ok(());
    }
    if session.has_exact("模型") || session.has_exact("Models") {
        return enter_model_category(&session, detection);
    }
    match kind {
        TraeKind::Code => {
            // Current TraeCode exposes Settings under the account menu, while
            // older builds still accept the keyboard/drawer route below.
            if let Some(account_button) = session
                .matching(|element| {
                    element.role() == "AXButton"
                        && (element.string("AXTitle").contains("免费")
                            || element
                                .string("AXTitle")
                                .to_ascii_lowercase()
                                .contains("free"))
                })
                .into_iter()
                .next()
            {
                if account_button.press() || account_button.click_center() {
                    if let Some(menu) = wait_until(Duration::from_secs(3), || {
                        let current = UiSession::new(detection, false).ok()?;
                        (current.has_exact("设置")
                            || current.has_exact("Settings")
                            || current.has_exact("模型")
                            || current.has_exact("Models"))
                        .then_some(current)
                    }) {
                        if menu.has_exact("设置") || menu.has_exact("Settings") {
                            menu.press_button(&["设置", "Settings"])?;
                        }
                        if let Some(settings) = wait_until(Duration::from_secs(5), || {
                            let current = UiSession::new(detection, false).ok()?;
                            (current.has_exact("模型") || current.has_exact("Models"))
                                .then_some(current)
                        }) {
                            return enter_model_category(&settings, detection);
                        }
                    }
                }
            }
            send_key(43, CG_COMMAND_FLAG)?;
        }
        TraeKind::Work => {
            let account_button = session
                .matching(|element| {
                    element.role() == "AXButton"
                        && (element.string("AXTitle").contains("免费")
                            || element
                                .string("AXTitle")
                                .to_ascii_lowercase()
                                .contains("free"))
                })
                .into_iter()
                .next()
                .ok_or_else(ui_unavailable)?;
            if !account_button.press() && !account_button.click_center() {
                return Err(ui_unavailable());
            }
            let menu = refreshed(detection, &["设置", "Settings"], Duration::from_secs(3))?;
            menu.press_button(&["设置", "Settings"])?;
            let settings = refreshed(detection, &["模型", "Models"], Duration::from_secs(5))?;
            settings.press_text(&["模型", "Models"])?;
            return refreshed(
                detection,
                &["添加模型", "Add model"],
                MODEL_SELECTOR_LOAD_TIMEOUT,
            )
            .map(|_| ());
        }
    }

    if let Some(settings) = wait_until(Duration::from_secs(1), || {
        let session = UiSession::new(detection, false).ok()?;
        (session.has_exact("模型") || session.has_exact("Models")).then_some(session)
    }) {
        return enter_model_category(&settings, detection);
    } else {
        // TraeCode keeps the settings categories behind an unlabelled drawer.
        // Locate it relative to the app window so window movement and display
        // scaling do not turn this into a fixed-coordinate click.
        let session = UiSession::new(detection, false)?;
        if !session.open_settings_drawer() {
            send_key(3, CG_COMMAND_FLAG)?;
        }
        if let Some(settings) = wait_until(Duration::from_secs(3), || {
            let session = UiSession::new(detection, false).ok()?;
            (session.has_exact("模型") || session.has_exact("Models")).then_some(session)
        }) {
            return enter_model_category(&settings, detection);
        }
        let search = wait_until(Duration::from_secs(5), || {
            let session = UiSession::new(detection, false).ok()?;
            session.field_with_placeholder(&["搜索", "Search"])
        })
        .ok_or_else(ui_unavailable)?;
        search.set_value("模型")?;
        let category = || {
            let session = UiSession::new(detection, false).ok()?;
            ["模型管理", "Model management", "Models", "Model"]
                .iter()
                .any(|label| session.has_exact(label))
                .then_some(session)
        };
        let settings = match wait_until(Duration::from_secs(5), category) {
            Some(settings) => settings,
            None => {
                search.set_value("model")?;
                wait_until(Duration::from_secs(5), category).ok_or_else(ui_unavailable)?
            }
        };
        settings.press_text(&["模型管理", "Model management", "Models", "Model"])?;
    }
    refreshed(
        detection,
        &["添加模型", "Add model"],
        MODEL_SELECTOR_LOAD_TIMEOUT,
    )
    .map(|_| ())
}

fn enter_model_category(session: &UiSession, detection: &AgentDetection) -> AppResult<()> {
    let labels = &["模型", "Models"];
    session.press_text(labels)?;
    if refreshed(
        detection,
        &["添加模型", "Add model"],
        Duration::from_secs(2),
    )
    .is_ok()
    {
        return Ok(());
    }
    let current = UiSession::new(detection, false)?;
    current.click_text(labels)?;
    refreshed(
        detection,
        &["添加模型", "Add model"],
        MODEL_SELECTOR_LOAD_TIMEOUT,
    )
    .map(|_| ())
}

fn refreshed(
    detection: &AgentDetection,
    labels: &[&str],
    timeout: Duration,
) -> AppResult<UiSession> {
    wait_until(timeout, || {
        let session = UiSession::new(detection, false).ok()?;
        labels
            .iter()
            .any(|label| session.has_exact(label))
            .then_some(session)
    })
    .ok_or_else(ui_unavailable)
}

fn wait_until<T>(timeout: Duration, mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn running_pid(app_path: &Path) -> AppResult<i32> {
    let marker = format!("{}/Contents/MacOS/", app_path.to_string_lossy());
    let output = Command::new("/bin/ps")
        .args(["-ax", "-o", "pid=,command="])
        .output()?;
    if !output.status.success() {
        return Err(ui_unavailable());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start();
            let (pid, command) = trimmed.split_once(char::is_whitespace)?;
            command
                .trim_start()
                .contains(&marker)
                .then(|| pid.parse::<i32>().ok())
                .flatten()
        })
        .ok_or_else(|| {
            CommandError::new("trae_not_running", "Trae 当前未运行").with_recovery(
                "请打开并登录 Trae 后重新切换；AT-Switch 不会主动启动未运行的 Agent。",
            )
        })
}

fn send_key(key_code: u16, flags: u64) -> AppResult<()> {
    // Safety: CGEvent constructors return retained objects; events are posted
    // synchronously and released after use.
    unsafe {
        let down = CGEventCreateKeyboardEvent(ptr::null(), key_code, true);
        let up = CGEventCreateKeyboardEvent(ptr::null(), key_code, false);
        let Some(down) = OwnedCf::new(down) else {
            return Err(ui_unavailable());
        };
        let Some(up) = OwnedCf::new(up) else {
            return Err(ui_unavailable());
        };
        if flags != 0 {
            CGEventSetFlags(down.0, flags);
            CGEventSetFlags(up.0, flags);
        }
        CGEventPost(CG_HID_EVENT_TAP, down.0);
        CGEventPost(CG_HID_EVENT_TAP, up.0);
    }
    Ok(())
}

fn post_mouse(event_type: u32, point: CgPoint) {
    // Safety: the event uses a finite screen coordinate and is released after
    // synchronous posting.
    unsafe {
        if let Some(event) =
            OwnedCf::new(CGEventCreateMouseEvent(ptr::null(), event_type, point, 0))
        {
            CGEventPost(CG_HID_EVENT_TAP, event.0);
        }
    }
}

fn click(point: CgPoint) {
    post_mouse(CG_EVENT_LEFT_MOUSE_DOWN, point);
    post_mouse(CG_EVENT_LEFT_MOUSE_UP, point);
}

fn cf_string(value: &str) -> Option<OwnedCf> {
    let value = CString::new(value).ok()?;
    // Safety: value is NUL-terminated UTF-8 and Core Foundation copies it.
    OwnedCf::new(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), UTF8_ENCODING) })
}

fn cf_string_value(value: CfTypeRef) -> Option<String> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return None;
    }
    // Safety: type identity was checked above. Four UTF-8 bytes per UTF-16
    // code unit plus NUL is a sufficient conversion buffer.
    let length = unsafe { CFStringGetLength(value as CfStringRef) };
    let mut buffer = vec![0_u8; length.saturating_mul(4).saturating_add(1) as usize];
    if !unsafe {
        CFStringGetCString(
            value as CfStringRef,
            buffer.as_mut_ptr().cast(),
            buffer.len() as isize,
            UTF8_ENCODING,
        )
    } {
        return None;
    }
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    String::from_utf8(buffer[..end].to_vec()).ok()
}

fn ui_unavailable() -> CommandError {
    CommandError::new("trae_ui_unavailable", "无法读取 Trae 官方模型界面")
        .with_recovery("请保持 Trae 主窗口打开并关闭遮挡弹窗，然后重新切换。")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{open_menu_with_retry, selection_candidates, selections_match};

    #[test]
    fn auto_labels_are_interchangeable_across_trae_versions() {
        assert!(selections_match("Auto", "Auto Mode"));
        assert!(selections_match("Auto Mode", "Auto"));
        assert_eq!(selection_candidates("Auto"), ["Auto", "Auto Mode"]);
        assert_eq!(selection_candidates("custom"), ["custom"]);
        assert!(!selections_match("custom", "Auto"));
    }

    #[test]
    fn acknowledged_click_without_a_visible_menu_uses_the_fallback() {
        let reads = Cell::new(0);
        let fallback_clicks = Cell::new(0);
        let result = open_menu_with_retry(
            || true,
            |_| {
                reads.set(reads.get() + 1);
                (reads.get() == 2).then_some("model menu")
            },
            || {
                fallback_clicks.set(fallback_clicks.get() + 1);
                true
            },
        );
        assert_eq!(result, Some("model menu"));
        assert_eq!(fallback_clicks.get(), 1);

        let result = open_menu_with_retry(
            || true,
            |_| Some("already open"),
            || panic!("a visible menu must not be clicked again"),
        );
        assert_eq!(result, Some("already open"));
    }
}
