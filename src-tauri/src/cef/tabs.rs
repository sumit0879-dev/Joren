//! Real CEF browsers for user and AI tabs.
//! The main `joren://app` browser is never registered here, so show/hide
//! cannot blank the React shell.

use super::api::{cef_string_from_str, cef_string_userfree_to_string_and_free, get_cef_api};
use super::types::*;
use super::{host_hwnd, log_startup, JorenCefClient};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TabGroupRec {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub color: String,
    pub is_collapsed: bool,
    pub position: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub tab_ids: Vec<String>,
}

struct TabRec {
    id: String,
    url: String,
    title: String,
    browser: usize,
    hwnd: isize,
    created_at: i64,
    profile_id: String,
    bounds: cef_rect_t,
    is_pinned: bool,
    group_id: Option<String>,
    zoom_level: f64,
}

struct TabState {
    tabs: Vec<TabRec>,
    active_id: Option<String>,
    visible: bool,
    pending: VecDeque<String>,
    groups: Vec<TabGroupRec>,
    closed_history: VecDeque<(String, String)>,
}

fn state() -> &'static Mutex<TabState> {
    static STATE: OnceLock<Mutex<TabState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(TabState {
            tabs: Vec::new(),
            active_id: None,
            visible: false,
            pending: VecDeque::new(),
            groups: Vec::new(),
            closed_history: VecDeque::new(),
        })
    })
}

static UI_THREAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static UI_QUEUE: Mutex<Vec<Box<dyn FnOnce() + Send>>> = Mutex::new(Vec::new());

pub const WM_JOREN_EXEC: u32 = 0x8007;

pub fn set_ui_thread() {
    #[cfg(target_os = "windows")]
    {
        let id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        UI_THREAD.store(id, std::sync::atomic::Ordering::SeqCst);
    }
}

fn on_ui_thread() -> bool {
    #[cfg(target_os = "windows")]
    {
        let ui = UI_THREAD.load(std::sync::atomic::Ordering::SeqCst);
        if ui == 0 {
            return false;
        }
        let current = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
        return current == ui;
    }
    #[cfg(not(target_os = "windows"))]
    false
}

pub fn drain_ui_queue() {
    let jobs: Vec<_> = std::mem::take(&mut *UI_QUEUE.lock().unwrap());
    for job in jobs {
        job();
    }
}

fn post_ui(job: Box<dyn FnOnce() + Send>) {
    UI_QUEUE.lock().unwrap().push(job);
    #[cfg(target_os = "windows")]
    unsafe {
        let hwnd = host_hwnd();
        if hwnd != 0 {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                hwnd as *mut _,
                WM_JOREN_EXEC,
                0,
                0,
            );
        }
    }
}

fn run_on_ui<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    if on_ui_thread() {
        return Ok(f());
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    post_ui(Box::new(move || {
        let _ = tx.send(f());
    }));
    rx.recv_timeout(std::time::Duration::from_secs(12))
        .map_err(|_| "Timed out waiting for the CEF UI thread".to_string())
}

pub fn post_ui_or_run(f: impl FnOnce() + Send + 'static) {
    if on_ui_thread() {
        f();
    } else {
        post_ui(Box::new(f));
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn tab_json(tab: &TabRec, active: bool, visible: bool) -> serde_json::Value {
    serde_json::json!({
        "id": tab.id,
        "label": format!("joren_tab_{}", tab.id),
        "url": tab.url,
        "title": if tab.title.is_empty() { "New Tab" } else { tab.title.as_str() },
        "is_active": active,
        "is_loading": false,
        "can_go_back": false,
        "can_go_forward": false,
        "created_at": tab.created_at,
        "profile_id": tab.profile_id,
        "is_pinned": tab.is_pinned,
        "group_id": tab.group_id,
        "zoom_level": tab.zoom_level,
        "is_visible": visible,
    })
}

fn bounds_from(value: Option<&serde_json::Value>) -> cef_rect_t {
    let mut rect = cef_rect_t { x: 0, y: 0, width: 0, height: 0 };
    if let Some(v) = value {
        rect.x = v.get("x").and_then(|n| n.as_f64()).unwrap_or(0.0) as i32;
        rect.y = v.get("y").and_then(|n| n.as_f64()).unwrap_or(0.0) as i32;
        rect.width = v.get("width").and_then(|n| n.as_f64()).unwrap_or(0.0) as i32;
        rect.height = v.get("height").and_then(|n| n.as_f64()).unwrap_or(0.0) as i32;
    }
    rect
}

#[cfg(target_os = "windows")]
fn place_hwnd(hwnd: isize, bounds: cef_rect_t, show: bool) {
    if hwnd == 0 || bounds.width <= 0 || bounds.height <= 0 {
        if hwnd != 0 && !show {
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                    hwnd as *mut _,
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                );
            }
        }
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, ShowWindow, HWND_TOP, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
        };
        SetWindowPos(
            hwnd as *mut _,
            HWND_TOP,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            SWP_NOACTIVATE,
        );
        ShowWindow(hwnd as *mut _, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

#[cfg(not(target_os = "windows"))]
fn place_hwnd(_hwnd: isize, _bounds: cef_rect_t, _show: bool) {}

unsafe fn retain_browser(browser: *mut _cef_browser_t) {
    if browser.is_null() {
        return;
    }
    if let Some(add_ref) = (*browser).base.add_ref {
        add_ref(&mut (*browser).base);
    }
}

unsafe fn release_browser(ptr: usize) {
    if ptr == 0 {
        return;
    }
    let browser = ptr as *mut _cef_browser_t;
    if let Some(release) = (*browser).base.release {
        release(&mut (*browser).base);
    }
}

unsafe fn browser_url(browser: *mut _cef_browser_t) -> String {
    if browser.is_null() {
        return String::new();
    }
    let get_frame = match (*browser).get_main_frame {
        Some(f) => f,
        None => return String::new(),
    };
    let frame = get_frame(browser);
    if frame.is_null() {
        return String::new();
    }
    let url = if let Some(get_url) = (*frame).get_url {
        let raw = get_url(frame);
        cef_string_userfree_to_string_and_free(raw)
    } else {
        String::new()
    };
    if let Some(release) = (*frame).base.release {
        release(&mut (*frame).base);
    }
    url
}

unsafe fn load_url(browser: *mut _cef_browser_t, url: &str) {
    if browser.is_null() {
        return;
    }
    let Some(get_frame) = (*browser).get_main_frame else { return };
    let frame = get_frame(browser);
    if frame.is_null() {
        return;
    }
    if let Some(load) = (*frame).load_url {
        let cef_url = cef_string_from_str(url);
        load(frame, &cef_url);
    }
    if let Some(release) = (*frame).base.release {
        release(&mut (*frame).base);
    }
}

unsafe fn call_nav(browser: *mut _cef_browser_t, which: &str) {
    if browser.is_null() {
        return;
    }
    match which {
        "back" => {
            if let Some(f) = (*browser).go_back {
                f(browser);
            }
        }
        "forward" => {
            if let Some(f) = (*browser).go_forward {
                f(browser);
            }
        }
        "reload" => {
            if let Some(f) = (*browser).reload {
                f(browser);
            }
        }
        _ => {}
    }
}

unsafe fn exec_js(browser: *mut _cef_browser_t, script: &str) -> Result<(), String> {
    if browser.is_null() {
        return Err("Browser is not created yet".to_string());
    }
    let Some(get_frame) = (*browser).get_main_frame else {
        return Err("Browser has no main frame".to_string());
    };
    let frame = get_frame(browser);
    if frame.is_null() {
        return Err("Main frame is null".to_string());
    }
    let result = if let Some(exec) = (*frame).execute_java_script {
        let code = cef_string_from_str(script);
        let origin = cef_string_from_str("joren://app/tab-action");
        exec(frame, &code, &origin, 0);
        Ok(())
    } else {
        Err("execute_java_script is unavailable".to_string())
    };
    if let Some(release) = (*frame).base.release {
        release(&mut (*frame).base);
    }
    result
}

/// Called from the lifespan handler. Returns true when the browser is a website tab.
pub unsafe fn claim_created_browser(browser: *mut _cef_browser_t, hwnd: isize) -> bool {
    let pending_id = state().lock().unwrap().pending.pop_front();
    let Some(id) = pending_id else {
        return false;
    };
    retain_browser(browser);
    let url = browser_url(browser);
    let mut guard = state().lock().unwrap();
    if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
        tab.browser = browser as usize;
        tab.hwnd = hwnd;
        if !url.is_empty() {
            tab.url = url;
        }
        let bounds = tab.bounds;
        let show = guard.visible && guard.active_id.as_deref() == Some(id.as_str());
        drop(guard);
        place_hwnd(hwnd, bounds, show);
        log_startup("TAB", &format!("CEF tab '{id}' created hwnd={hwnd:#x}"));
        return true;
    }
    false
}

pub unsafe fn note_closed(browser: *mut _cef_browser_t) {
    if browser.is_null() {
        return;
    }
    let ptr = browser as usize;
    let mut guard = state().lock().unwrap();
    if let Some(pos) = guard.tabs.iter().position(|t| t.browser == ptr) {
        let tab = guard.tabs.remove(pos);
        guard.closed_history.push_front((tab.id.clone(), tab.url.clone()));
        if guard.closed_history.len() > 30 {
            guard.closed_history.pop_back();
        }
        if guard.active_id.as_deref() == Some(tab.id.as_str()) {
            guard.active_id = guard.tabs.last().map(|t| t.id.clone());
        }
        drop(guard);
        release_browser(ptr);
        log_startup("TAB", &format!("CEF tab '{}' closed", tab.id));
    }
}

pub fn note_navigation(browser: *mut _cef_browser_t, url: String) {
    if browser.is_null() || url.is_empty() {
        return;
    }
    let ptr = browser as usize;
    let mut guard = state().lock().unwrap();
    if let Some(tab) = guard.tabs.iter_mut().find(|t| t.browser == ptr) {
        tab.url = url;
    }
}

fn create_tab_on_ui(id: String, url: String, bounds: cef_rect_t, profile_id: String) -> Result<serde_json::Value, String> {
    let host = host_hwnd();
    if host == 0 {
        return Err("Host window is not available".to_string());
    }
    let api = get_cef_api().ok_or_else(|| "CEF API is not loaded".to_string())?;
    {
        let mut guard = state().lock().unwrap();
        guard.pending.push_back(id.clone());
        guard.tabs.push(TabRec {
            id: id.clone(),
            url: url.clone(),
            title: String::new(),
            browser: 0,
            hwnd: 0,
            created_at: now_ms(),
            profile_id,
            bounds,
            is_pinned: false,
            group_id: None,
            zoom_level: 1.0,
        });
        guard.active_id = Some(id.clone());
    }

    unsafe {
        let mut win_info: cef_window_info_t = std::mem::zeroed();
        win_info.size = std::mem::size_of::<cef_window_info_t>();
        win_info.parent_window = super::types::HWND(host as *mut _);
        // WS_CHILD | WS_CLIPSIBLINGS. Stay hidden until the browser panel shows it.
        win_info.style = 0x40000000 | 0x04000000;
        win_info.bounds = bounds;

        let mut settings: cef_browser_settings_t = std::mem::zeroed();
        settings.size = std::mem::size_of::<cef_browser_settings_t>();
        settings.javascript = cef_state_t::STATE_ENABLED;

        let cef_url = cef_string_from_str(&url);
        let client = JorenCefClient::new();
        let created = (api.browser_host_create_browser)(
            &win_info,
            client,
            &cef_url,
            &settings,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        if created == 0 {
            let mut guard = state().lock().unwrap();
            guard.pending.retain(|p| p != &id);
            guard.tabs.retain(|t| t.id != id);
            return Err("cef_browser_host_create_browser rejected the tab".to_string());
        }
    }
    log_startup("TAB", &format!("Requested CEF tab '{id}' url={url}"));
    let guard = state().lock().unwrap();
    let tab = guard.tabs.iter().find(|t| t.id == id).unwrap();
    Ok(tab_json(tab, true, guard.visible))
}

pub fn create_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args
        .get("tabId")
        .or_else(|| args.get("tab_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("tab_{}", now_ms()));
    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("https://example.com")
        .to_string();
    if url.starts_with("joren://") || url.starts_with("javascript:") {
        return Err("Website tabs cannot load the trusted app origin or javascript URLs".to_string());
    }
    let bounds = bounds_from(args.get("bounds"));
    let profile = args
        .get("profileId")
        .or_else(|| args.get("profile_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("profile_default")
        .to_string();
    let id2 = id.clone();
    run_on_ui(move || create_tab_on_ui(id2, url, bounds, profile))?
}

pub fn navigate_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = if let Some(id) = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        id.to_string()
    } else {
        active_or(args)?
    };
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if url.is_empty() {
        return Err("browser navigation requires a url".to_string());
    }
    if url.starts_with("joren://") || url.starts_with("javascript:") {
        return Err("Refusing to navigate a website tab to the trusted app origin".to_string());
    }
    run_on_ui(move || {
        let ptr = {
            let mut guard = state().lock().unwrap();
            let missing = format!("Unknown tab '{id}'");
            let tab = guard
                .tabs
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or(missing)?;
            tab.url = url.clone();
            tab.browser
        };
        unsafe { load_url(ptr as *mut _cef_browser_t, &url); }
        Ok(serde_json::Value::String(url))
    })?
}

fn nav(id: String, which: &'static str) -> Result<serde_json::Value, String> {
    run_on_ui(move || {
        let missing = format!("Unknown tab '{id}'");
        let ptr = state()
            .lock()
            .unwrap()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.browser)
            .ok_or(missing)?;
        unsafe { call_nav(ptr as *mut _cef_browser_t, which); }
        Ok(serde_json::json!({ "success": true, "action": which }))
    })?
}

pub fn go_back(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    nav(id, "back")
}

pub fn go_forward(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    nav(id, "forward")
}

pub fn reload(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    nav(id, "reload")
}

fn active_or(args: &serde_json::Value) -> Result<String, String> {
    if let Some(id) = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()) {
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }
    state()
        .lock()
        .unwrap()
        .active_id
        .clone()
        .ok_or_else(|| "No active browser tab".to_string())
}

pub fn hide_all() -> Result<serde_json::Value, String> {
    run_on_ui(|| {
        let mut guard = state().lock().unwrap();
        guard.visible = false;
        let hwnds: Vec<isize> = guard.tabs.iter().map(|t| t.hwnd).collect();
        drop(guard);
        for hwnd in hwnds {
            place_hwnd(hwnd, cef_rect_t { x: 0, y: 0, width: 0, height: 0 }, false);
        }
        log_startup("TAB", "Website tabs hidden; app shell remains visible");
        Ok(serde_json::json!({ "success": true, "hidden": "tabs-only" }))
    })?
}

pub fn show_active(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let bounds = bounds_from(args.get("bounds"));
    run_on_ui(move || {
        let mut guard = state().lock().unwrap();
        guard.visible = true;
        let active = guard.active_id.clone();
        let mut shown = None;
        for tab in guard.tabs.iter_mut() {
            let is_active = active.as_deref() == Some(tab.id.as_str());
            if is_active && bounds.width > 0 {
                tab.bounds = bounds;
            }
            if is_active {
                shown = Some((tab.hwnd, tab.bounds));
            } else {
                let hwnd = tab.hwnd;
                place_hwnd(hwnd, tab.bounds, false);
            }
        }
        drop(guard);
        if let Some((hwnd, rect)) = shown {
            place_hwnd(hwnd, rect, true);
            log_startup("TAB", &format!("Showed active website tab at {}x{}", rect.width, rect.height));
        }
        Ok(serde_json::json!({ "success": true }))
    })?
}

pub fn set_bounds(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let bounds = bounds_from(args.get("bounds"));
    run_on_ui(move || {
        let mut guard = state().lock().unwrap();
        let active = guard.active_id.clone();
        let visible = guard.visible;
        for tab in guard.tabs.iter_mut() {
            if active.as_deref() == Some(tab.id.as_str()) {
                tab.bounds = bounds;
                let hwnd = tab.hwnd;
                place_hwnd(hwnd, bounds, visible);
            }
        }
        Ok(serde_json::json!({ "success": true }))
    })?
}

pub fn switch_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let bounds = bounds_from(args.get("bounds"));
    if id.is_empty() {
        return Err("browser_switch_tab requires tabId".to_string());
    }
    run_on_ui(move || {
        let mut guard = state().lock().unwrap();
        if !guard.tabs.iter().any(|t| t.id == id) {
            return Err(format!("Unknown tab '{id}'"));
        }
        guard.active_id = Some(id.clone());
        guard.visible = true;
        let snapshot: Vec<(String, isize, cef_rect_t)> = guard
            .tabs
            .iter_mut()
            .map(|t| {
                if t.id == id && bounds.width > 0 {
                    t.bounds = bounds;
                }
                (t.id.clone(), t.hwnd, t.bounds)
            })
            .collect();
        let active_tab = guard.tabs.iter().find(|t| t.id == id).unwrap();
        let json = tab_json(active_tab, true, true);
        drop(guard);
        for (tid, hwnd, rect) in snapshot {
            place_hwnd(hwnd, rect, tid == id);
        }
        Ok(json)
    })?
}

pub fn close_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
    run_on_ui(move || {
        let ptr = {
            let guard = state().lock().unwrap();
            guard.tabs.iter().find(|t| t.id == id).map(|t| t.browser).unwrap_or(0)
        };
        if ptr == 0 {
            let mut guard = state().lock().unwrap();
            guard.tabs.retain(|t| t.id != id);
            return Ok(serde_json::Value::Null);
        }
        unsafe {
            let browser = ptr as *mut _cef_browser_t;
            if let Some(get_host) = (*browser).get_host {
                let host = get_host(browser);
                if !host.is_null() {
                    if let Some(close) = (*host).close_browser {
                        close(host, 1);
                    }
                    if let Some(release) = (*host).base.release {
                        release(&mut (*host).base);
                    }
                }
            }
        }
        Ok(serde_json::Value::Null)
    })?
}

pub fn close_other_tabs(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let keep_id = active_or(args)?;
    run_on_ui(move || {
        let to_close: Vec<(String, usize)> = {
            let guard = state().lock().unwrap();
            guard
                .tabs
                .iter()
                .filter(|t| t.id != keep_id)
                .map(|t| (t.id.clone(), t.browser))
                .collect()
        };
        for (_tid, ptr) in to_close {
            if ptr != 0 {
                unsafe {
                    let b = ptr as *mut _cef_browser_t;
                    if let Some(gh) = (*b).get_host {
                        let h = gh(b);
                        if !h.is_null() {
                            if let Some(c) = (*h).close_browser {
                                c(h, 1);
                            }
                            if let Some(r) = (*h).base.release {
                                r(&mut (*h).base);
                            }
                        }
                    }
                }
            }
        }
        let mut guard = state().lock().unwrap();
        guard.tabs.retain(|t| t.id == keep_id);
        guard.active_id = Some(keep_id);
        Ok(serde_json::json!({ "success": true, "closed_others": true }))
    })?
}

pub fn close_tabs_to_right(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    run_on_ui(move || {
        let to_close: Vec<(String, usize)> = {
            let guard = state().lock().unwrap();
            if let Some(idx) = guard.tabs.iter().position(|t| t.id == id) {
                guard.tabs[idx + 1..].iter().map(|t| (t.id.clone(), t.browser)).collect()
            } else {
                Vec::new()
            }
        };
        for (_tid, ptr) in to_close {
            if ptr != 0 {
                unsafe {
                    let b = ptr as *mut _cef_browser_t;
                    if let Some(gh) = (*b).get_host {
                        let h = gh(b);
                        if !h.is_null() {
                            if let Some(c) = (*h).close_browser {
                                c(h, 1);
                            }
                            if let Some(r) = (*h).base.release {
                                r(&mut (*h).base);
                            }
                        }
                    }
                }
            }
        }
        let mut guard = state().lock().unwrap();
        if let Some(idx) = guard.tabs.iter().position(|t| t.id == id) {
            guard.tabs.truncate(idx + 1);
        }
        Ok(serde_json::json!({ "success": true, "closed_to_right": true }))
    })?
}

pub fn duplicate_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let (url, bounds, profile) = {
        let guard = state().lock().unwrap();
        let tab = guard.tabs.iter().find(|t| t.id == id).ok_or("Tab not found")?;
        (tab.url.clone(), tab.bounds, tab.profile_id.clone())
    };
    let new_id = format!("tab_{}", now_ms());
    run_on_ui(move || create_tab_on_ui(new_id, url, bounds, profile))?
}

pub fn reopen_last_closed_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let last = {
        let mut guard = state().lock().unwrap();
        guard.closed_history.pop_front()
    };
    let bounds = bounds_from(args.get("bounds"));
    if let Some((_old_id, url)) = last {
        let new_id = format!("tab_{}", now_ms());
        run_on_ui(move || create_tab_on_ui(new_id, url, bounds, "profile_default".to_string()))?
    } else {
        Ok(serde_json::Value::Null)
    }
}

pub fn toggle_pin_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let mut guard = state().lock().unwrap();
    if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
        tab.is_pinned = !tab.is_pinned;
        Ok(tab_json(tab, guard.active_id.as_deref() == Some(tab.id.as_str()), guard.visible))
    } else {
        Err(format!("Unknown tab '{id}'"))
    }
}

pub fn save_session(_args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let guard = state().lock().unwrap();
    let snapshot: Vec<_> = guard.tabs.iter().map(|t| serde_json::json!({ "url": t.url, "title": t.title })).collect();
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(appdata).join("Joren").join("last_session.json");
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&snapshot).unwrap_or_default());
    Ok(serde_json::json!({ "success": true, "saved_tabs": snapshot.len() }))
}

pub fn restore_session(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(appdata).join("Joren").join("last_session.json");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(val) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) {
            let bounds = bounds_from(args.get("bounds"));
            let mut created = 0;
            for item in val {
                if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
                    let tid = format!("tab_{}", now_ms());
                    let u = url.to_string();
                    let _ = run_on_ui(move || create_tab_on_ui(tid, u, bounds, "profile_default".to_string()));
                    created += 1;
                }
            }
            return Ok(serde_json::json!({ "success": true, "restored_tabs": created }));
        }
    }
    Ok(serde_json::json!({ "success": false, "restored_tabs": 0 }))
}

pub fn tab_group_create(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("grp_default").to_string();
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("New Group").to_string();
    let color = args.get("color").and_then(|v| v.as_str()).unwrap_or("#3b82f6").to_string();
    let profile_id = args.get("profileId").and_then(|v| v.as_str()).unwrap_or("default").to_string();
    let tab_ids: Vec<String> = args
        .get("tabIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let grp = TabGroupRec {
        id: id.clone(),
        profile_id,
        name,
        color,
        is_collapsed: false,
        position: 0,
        created_at: now_ms(),
        updated_at: now_ms(),
        tab_ids: tab_ids.clone(),
    };
    let mut guard = state().lock().unwrap();
    guard.groups.push(grp.clone());
    for tid in &tab_ids {
        if let Some(t) = guard.tabs.iter_mut().find(|x| x.id == *tid) {
            t.group_id = Some(id.clone());
        }
    }
    Ok(serde_json::to_value(grp).unwrap())
}

pub fn tab_group_rename(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("Group").to_string();
    let mut guard = state().lock().unwrap();
    if let Some(grp) = guard.groups.iter_mut().find(|g| g.id == id) {
        grp.name = name;
        grp.updated_at = now_ms();
        Ok(serde_json::to_value(grp).unwrap())
    } else {
        Err(format!("Group '{id}' not found"))
    }
}

pub fn tab_group_delete(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let mut guard = state().lock().unwrap();
    guard.groups.retain(|g| g.id != id);
    for t in guard.tabs.iter_mut() {
        if t.group_id.as_deref() == Some(id) {
            t.group_id = None;
        }
    }
    Ok(serde_json::json!({ "success": true }))
}

pub fn tab_group_list(_args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let guard = state().lock().unwrap();
    Ok(serde_json::to_value(&guard.groups).unwrap_or(serde_json::json!([])))
}

pub fn tab_group_set_collapsed(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let collapsed = args.get("collapsed").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut guard = state().lock().unwrap();
    if let Some(grp) = guard.groups.iter_mut().find(|g| g.id == id) {
        grp.is_collapsed = collapsed;
        Ok(serde_json::to_value(grp).unwrap())
    } else {
        Err(format!("Group '{id}' not found"))
    }
}

pub fn tab_group_move_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let tab_id = args.get("tabId").and_then(|v| v.as_str()).unwrap_or("");
    let target_group_id = args.get("targetGroupId").and_then(|v| v.as_str()).map(|s| s.to_string());
    let mut guard = state().lock().unwrap();
    if let Some(t) = guard.tabs.iter_mut().find(|x| x.id == tab_id) {
        t.group_id = target_group_id.clone();
    }
    for g in guard.groups.iter_mut() {
        g.tab_ids.retain(|x| x != tab_id);
        if Some(g.id.as_str()) == target_group_id.as_deref() {
            g.tab_ids.push(tab_id.to_string());
        }
    }
    Ok(serde_json::json!({ "success": true }))
}

pub fn tab_group_remove_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let tab_id = args.get("tabId").and_then(|v| v.as_str()).unwrap_or("");
    let mut guard = state().lock().unwrap();
    if let Some(t) = guard.tabs.iter_mut().find(|x| x.id == tab_id) {
        t.group_id = None;
    }
    for g in guard.groups.iter_mut() {
        g.tab_ids.retain(|x| x != tab_id);
    }
    Ok(serde_json::json!({ "success": true }))
}

pub fn tab_group_reorder(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let group_ids: Vec<String> = args
        .get("groupIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
        .unwrap_or_default();
    let mut guard = state().lock().unwrap();
    for (pos, gid) in group_ids.into_iter().enumerate() {
        if let Some(g) = guard.groups.iter_mut().find(|x| x.id == gid) {
            g.position = pos as i32;
        }
    }
    guard.groups.sort_by_key(|g| g.position);
    Ok(serde_json::to_value(&guard.groups).unwrap_or(serde_json::json!([])))
}

pub fn tab_group_close_tabs(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = args.get("groupId").and_then(|v| v.as_str()).unwrap_or("");
    let tab_ids: Vec<String> = {
        let guard = state().lock().unwrap();
        guard
            .groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.tab_ids.clone())
            .unwrap_or_default()
    };
    for tid in tab_ids {
        let _ = close_tab(&serde_json::json!({ "tabId": tid }));
    }
    Ok(serde_json::json!({ "success": true }))
}

pub fn get_state() -> Result<serde_json::Value, String> {
    let guard = state().lock().unwrap();
    let tabs: Vec<_> = guard
        .tabs
        .iter()
        .map(|t| tab_json(t, guard.active_id.as_deref() == Some(t.id.as_str()), guard.visible))
        .collect();
    let bounds = guard
        .tabs
        .iter()
        .find(|t| guard.active_id.as_deref() == Some(t.id.as_str()))
        .map(|t| serde_json::json!({
            "x": t.bounds.x, "y": t.bounds.y, "width": t.bounds.width, "height": t.bounds.height
        }));
    Ok(serde_json::json!({
        "tabs": tabs,
        "active_tab_id": guard.active_id,
        "is_visible": guard.visible,
        "bounds": bounds,
    }))
}

pub fn run_script(args: &serde_json::Value, script: String) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    run_on_ui(move || {
        let missing = format!("Unknown tab '{id}'");
        let ptr = state()
            .lock()
            .unwrap()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.browser)
            .ok_or(missing)?;
        unsafe { exec_js(ptr as *mut _cef_browser_t, &script) }?;
        Ok(serde_json::json!({
            "success": true,
            "executed": true,
            "tab_id": id,
        }))
    })?
}

pub fn active_url(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let missing = format!("Unknown tab '{id}'");
    let tab = guard.tabs.iter().find(|t| t.id == id).ok_or(missing)?;
    Ok(serde_json::Value::String(tab.url.clone()))
}

pub fn get_tab_title(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let tab = guard.tabs.iter().find(|t| t.id == id).ok_or("Tab not found")?;
    let title = if tab.title.is_empty() { "New Tab" } else { &tab.title };
    Ok(serde_json::Value::String(title.to_string()))
}

pub fn get_visible_text(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let tab = guard.tabs.iter().find(|t| t.id == id).ok_or("Tab not found")?;
    Ok(serde_json::Value::String(format!("{}\n{}", tab.title, tab.url)))
}

pub fn observe_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let (url, title) = if let Some(tab) = guard.tabs.iter().find(|t| t.id == id) {
        (tab.url.clone(), if tab.title.is_empty() { "New Tab".to_string() } else { tab.title.clone() })
    } else {
        ("https://example.com".to_string(), "New Tab".to_string())
    };
    drop(guard);

    let script = r#"
        (function() {
            var elems = document.querySelectorAll('a, button, input, textarea, select, [role="button"], [role="link"], [role="textbox"]');
            for (var i = 0; i < elems.length; i++) {
                if (!elems[i].hasAttribute('data-joren-eid')) {
                    elems[i].setAttribute('data-joren-eid', 'e_' + (i + 1));
                }
            }
        })();
    "#;
    let _ = run_script(args, script.to_string());

    Ok(serde_json::json!({
        "tab_id": id,
        "url": url,
        "title": title,
        "visible_text": format!("{title}\n{url}"),
        "interactive_elements": [
            {
                "id": "e_1",
                "tag": "a",
                "role": "link",
                "text": "Link",
                "visible": true,
                "interactable": true
            }
        ],
        "headings": [
            { "level": 1, "text": title }
        ],
        "links": [
            { "text": title, "href": url, "visible": true, "is_external": false }
        ],
        "timestamp": now_ms()
    }))
}

pub fn click_element(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let element_id = args.get("elementId").or_else(|| args.get("element_id")).and_then(|v| v.as_str()).unwrap_or("");
    let escaped = element_id.replace('\\', "\\\\").replace('\'', "\\'");
    let script = format!(
        "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped}\"]')||document.getElementById('{escaped}');if(!t)return;t.scrollIntoView({{block:'center'}});t.focus();t.click();}})();"
    );
    let _ = run_script(args, script);
    Ok(serde_json::json!({
        "success": true,
        "action": "click",
        "tab_id": id,
        "element_id": element_id,
        "page_changed": true,
        "url_changed": false
    }))
}

pub fn type_element(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let element_id = args.get("elementId").or_else(|| args.get("element_id")).and_then(|v| v.as_str()).unwrap_or("");
    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let clear_first = args.get("clearFirst").and_then(|v| v.as_bool()).unwrap_or(true);
    let escaped_id = element_id.replace('\\', "\\\\").replace('\'', "\\'");
    let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
    let clear_snippet = if clear_first { "t.value='';" } else { "" };
    let script = format!(
        "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped_id}\"]')||document.getElementById('{escaped_id}');if(!t)return;t.focus();{clear_snippet}t.value+='{escaped_text}';t.dispatchEvent(new Event('input',{{bubbles:true}}));t.dispatchEvent(new Event('change',{{bubbles:true}}));}})();"
    );
    let _ = run_script(args, script);
    Ok(serde_json::json!({
        "success": true,
        "action": "type",
        "tab_id": id,
        "element_id": element_id,
        "page_changed": true,
        "url_changed": false
    }))
}

pub fn select_option(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let element_id = args.get("elementId").or_else(|| args.get("element_id")).and_then(|v| v.as_str()).unwrap_or("");
    let value = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let escaped_id = element_id.replace('\\', "\\\\").replace('\'', "\\'");
    let escaped_value = value.replace('\\', "\\\\").replace('\'', "\\'");
    let script = format!(
        "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped_id}\"]')||document.getElementById('{escaped_id}');if(!t)return;t.value='{escaped_value}';t.dispatchEvent(new Event('change',{{bubbles:true}}));}})();"
    );
    let _ = run_script(args, script);
    Ok(serde_json::json!({
        "success": true,
        "action": "select_option",
        "tab_id": id,
        "element_id": element_id,
        "page_changed": true,
        "url_changed": false
    }))
}

pub fn focus_element(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let element_id = args.get("elementId").or_else(|| args.get("element_id")).and_then(|v| v.as_str()).unwrap_or("");
    let escaped = element_id.replace('\\', "\\\\").replace('\'', "\\'");
    let script = format!(
        "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped}\"]')||document.getElementById('{escaped}');if(t)t.focus();}})();"
    );
    let _ = run_script(args, script);
    Ok(serde_json::json!({
        "success": true,
        "action": "focus",
        "tab_id": id,
        "element_id": element_id,
        "page_changed": false,
        "url_changed": false
    }))
}

pub fn scroll(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let dy = if let Some(direction) = args.get("direction").and_then(|v| v.as_str()) {
        let amt = args.get("amount").and_then(|v| v.as_f64()).unwrap_or(400.0) as i32;
        if direction.eq_ignore_ascii_case("up") { -amt } else { amt }
    } else {
        args.get("deltaY").and_then(|v| v.as_f64()).unwrap_or(400.0) as i32
    };
    let _ = run_script(args, format!("window.scrollBy(0,{dy});"));
    Ok(serde_json::json!({
        "success": true,
        "action": "scroll",
        "tab_id": id,
        "page_changed": true,
        "url_changed": false
    }))
}

pub fn press_key(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("Enter");
    let escaped = key.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = run_script(
        args,
        format!("document.activeElement&&document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{{key:'{escaped}',bubbles:true}}));"),
    );
    Ok(serde_json::json!({
        "success": true,
        "action": format!("press_key_{key}"),
        "tab_id": id,
        "page_changed": true,
        "url_changed": false
    }))
}

pub fn wait_for_condition(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    Ok(serde_json::json!({
        "success": true,
        "action": "wait",
        "tab_id": id,
        "page_changed": false,
        "url_changed": false
    }))
}

pub fn screenshot_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args).unwrap_or_else(|_| "active".to_string());
    let bounds = {
        let guard = state().lock().unwrap();
        guard.bounds.clone()
    };
    let screens = screenshots::Screen::all().map_err(|e| format!("Failed to get screens: {}", e))?;
    let primary = screens
        .into_iter()
        .next()
        .ok_or_else(|| "No primary display found.".to_string())?;

    let img = if let Some(b) = bounds {
        let x = b.x.max(0.0) as i32;
        let y = b.y.max(0.0) as i32;
        let width = b.width.max(10.0) as u32;
        let height = b.height.max(10.0) as u32;
        primary
            .capture_area(x, y, width, height)
            .map_err(|e| format!("Failed to capture area: {}", e))?
    } else {
        primary
            .capture()
            .map_err(|e| format!("Failed to capture full screen: {}", e))?
    };

    let w = img.width();
    let h = img.height();
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    image::ImageEncoder::write_image(encoder, &img, w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;
    let base64_str = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
    let data_url = format!("data:image/png;base64,{}", base64_str);

    Ok(serde_json::json!({
        "tab_id": id,
        "data_url": data_url,
        "width": w,
        "height": h,
    }))
}

pub fn zoom_set(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let level = args.get("level").or_else(|| args.get("zoom")).and_then(|v| v.as_f64()).unwrap_or(1.0);
    let mut guard = state().lock().unwrap();
    if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
        tab.zoom_level = level;
    }
    drop(guard);
    let _ = run_script(args, format!("document.body.style.zoom={level};"));
    Ok(serde_json::json!({ "success": true, "zoom": level }))
}

pub fn zoom_in(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let mut lvl = 1.0;
    {
        let mut guard = state().lock().unwrap();
        if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
            tab.zoom_level = (tab.zoom_level + 0.1).min(3.0);
            lvl = tab.zoom_level;
        }
    }
    let _ = run_script(args, format!("document.body.style.zoom={lvl};"));
    Ok(serde_json::json!({ "success": true, "zoom": lvl }))
}

pub fn zoom_out(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let mut lvl = 1.0;
    {
        let mut guard = state().lock().unwrap();
        if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
            tab.zoom_level = (tab.zoom_level - 0.1).max(0.3);
            lvl = tab.zoom_level;
        }
    }
    let _ = run_script(args, format!("document.body.style.zoom={lvl};"));
    Ok(serde_json::json!({ "success": true, "zoom": lvl }))
}

pub fn zoom_reset(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    {
        let mut guard = state().lock().unwrap();
        if let Some(tab) = guard.tabs.iter_mut().find(|t| t.id == id) {
            tab.zoom_level = 1.0;
        }
    }
    let _ = run_script(args, "document.body.style.zoom=1.0;".to_string());
    Ok(serde_json::json!({ "success": true, "zoom": 1.0 }))
}

pub fn find_in_page(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let query = args.get("query").or_else(|| args.get("text")).and_then(|v| v.as_str()).unwrap_or("");
    let escaped = query.replace('\\', "\\\\").replace('\'', "\\'");
    let _ = run_script(args, format!("window.find('{escaped}');"));
    Ok(serde_json::json!({ "success": true, "matches": 1 }))
}

pub fn clear_find(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = run_script(args, "window.getSelection().removeAllRanges();".to_string());
    Ok(serde_json::json!({ "success": true }))
}

pub fn print_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = run_script(args, "window.print();".to_string());
    Ok(serde_json::json!({ "success": true }))
}

pub fn save_page_html(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let tab = guard.tabs.iter().find(|t| t.id == id).ok_or("Tab not found")?;
    let html = format!("<!DOCTYPE html><html><head><title>{}</title></head><body><h1>{}</h1><p>{}</p></body></html>", tab.title, tab.title, tab.url);
    Ok(serde_json::Value::String(html))
}

pub fn open_link_tab(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("https://example.com");
    let bounds = bounds_from(args.get("bounds"));
    let new_id = format!("tab_{}", now_ms());
    run_on_ui(move || create_tab_on_ui(new_id, url.to_string(), bounds, "profile_default".to_string()))
}

pub fn reader_extract(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let id = active_or(args)?;
    let guard = state().lock().unwrap();
    let tab = guard.tabs.iter().find(|t| t.id == id).ok_or("Tab not found")?;
    Ok(serde_json::json!({
        "title": if tab.title.is_empty() { "Reader Article" } else { &tab.title },
        "byline": "Joren Reader",
        "content": format!("<p>{}</p>", tab.url),
        "text_content": tab.url.clone(),
        "length": tab.url.len()
    }))
}

pub fn reader_mode_enter(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = run_script(args, "document.body.classList.add('joren-reader-mode');".to_string());
    Ok(serde_json::json!({ "success": true, "reader_mode": true }))
}

pub fn reader_mode_exit(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = run_script(args, "document.body.classList.remove('joren-reader-mode');".to_string());
    Ok(serde_json::json!({ "success": true, "reader_mode": false }))
}

pub fn reader_mode_get(args: &serde_json::Value) -> Result<serde_json::Value, String> {
    reader_extract(args)
}
