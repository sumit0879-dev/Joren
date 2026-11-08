pub mod api;
pub mod app;
pub mod scheme;
pub mod tabs;
pub mod types;

use api::{cef_string_from_str, cef_string_to_string, cef_string_userfree_to_string_and_free, get_cef_api};
use app::JorenCefApp;
use scheme::{set_command_dispatcher, JorenSchemeHandlerFactory};
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};
use types::*;

pub struct BrowserSurfaceController {
    main_window_hwnd: isize,
    main_view_hwnd: isize,
    browser_tab_hwnd: isize,
    is_browser_visible: bool,
    bounds: cef_rect_t,
}

static SURFACE_CONTROLLER: Mutex<Option<BrowserSurfaceController>> = Mutex::new(None);
static MAIN_BROWSER: Mutex<Option<usize>> = Mutex::new(None);

pub fn log_startup(stage: &str, details: &str) {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let log_dir = std::path::PathBuf::from(appdata).join("Joren").join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("startup.log");
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{}] [pid={}] [{}] {}\n", now, std::process::id(), stage, details);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_file) {
        use std::io::Write;
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
    eprintln!("[{}] {}", stage, details);
}

#[cfg(target_os = "windows")]
pub fn is_process_elevated() -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut ret_len: u32 = 0;
        let success = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        if success != 0 {
            Some(elevation.TokenIsElevated != 0)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn is_process_elevated() -> Option<bool> {
    None
}

pub fn get_process_role() -> String {
    for arg in std::env::args() {
        if let Some(rest) = arg.strip_prefix("--type=") {
            return rest.to_string();
        }
    }
    "browser".to_string()
}

#[cfg(target_os = "windows")]
pub fn show_fatal_error_dialog(title: &str, message: &str) {
    eprintln!("[FATAL_ERROR_DIALOG] [{}] {}", title, message);
    if std::env::var("CI").is_ok() || std::env::args().any(|a| a == "--smoke-test") {
        return;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let wide_msg: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_msg.as_ptr(),
            wide_title.as_ptr(),
            MB_ICONERROR | MB_OK,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn show_fatal_error_dialog(_title: &str, _message: &str) {}

pub fn init_surface_controller(main_hwnd: isize) {
    let mut guard = SURFACE_CONTROLLER.lock().unwrap();
    *guard = Some(BrowserSurfaceController {
        main_window_hwnd: main_hwnd,
        main_view_hwnd: 0,
        browser_tab_hwnd: 0,
        is_browser_visible: false,
        bounds: cef_rect_t {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
    });
}

pub fn host_hwnd() -> isize {
    SURFACE_CONTROLLER
        .lock()
        .unwrap()
        .as_ref()
        .map(|ctrl| ctrl.main_window_hwnd)
        .unwrap_or(0)
}

pub fn set_main_view_hwnd(hwnd: isize) {
    let mut guard = SURFACE_CONTROLLER.lock().unwrap();
    if let Some(ctrl) = guard.as_mut() {
        ctrl.main_view_hwnd = hwnd;
    }
}

pub fn set_main_browser(browser: *mut _cef_browser_t) {
    let mut guard = MAIN_BROWSER.lock().unwrap();
    *guard = Some(browser as usize);
}

pub fn clear_main_browser() {
    let mut guard = MAIN_BROWSER.lock().unwrap();
    *guard = None;
}

static SMOKE_TEST_PASSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn close_main_window() {
    #[cfg(target_os = "windows")]
    {
        let guard = SURFACE_CONTROLLER.lock().unwrap();
        if let Some(ctrl) = guard.as_ref() {
            if ctrl.main_window_hwnd != 0 {
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                        ctrl.main_window_hwnd as *mut _,
                        windows_sys::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                        0,
                        0,
                    );
                }
            }
        }
    }
}

pub fn notify_smoke_test_success() {
    log_startup("SMOKE_TEST", "notify_smoke_test_success called: React UI & frontend IPC roundtrip verified!");
    SMOKE_TEST_PASSED.store(true, std::sync::atomic::Ordering::SeqCst);
    if std::env::args().any(|a| a == "--smoke-test") {
        close_main_window();
    }
}

pub fn is_smoke_test_passed() -> bool {
    SMOKE_TEST_PASSED.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn dispatch_event_to_browser(event_name: &str, payload_json: &str) {
    let event_name = event_name.to_string();
    let payload_json = payload_json.to_string();
    tabs::post_ui_or_run(move || unsafe {
        deliver_event_on_ui(&event_name, &payload_json);
    });
}

unsafe fn deliver_event_on_ui(event_name: &str, payload_json: &str) {
    let guard = MAIN_BROWSER.lock().unwrap();
    if let Some(browser_ptr) = *guard {
        unsafe {
            let browser = browser_ptr as *mut _cef_browser_t;
            if !browser.is_null() {
                let b = &*browser;
                if let Some(get_frame) = b.get_main_frame {
                    let frame = get_frame(browser);
                    if !frame.is_null() {
                        let f = &*frame;
                        if let Some(exec_js) = f.execute_java_script {
                            let script = format!(
                                "window.dispatchEvent(new CustomEvent('joren-event', {{ detail: {{ event: {}, payload: {} }} }}));",
                                serde_json::to_string(event_name).unwrap_or_else(|_| "\"\"".to_string()),
                                payload_json
                            );
                            let cef_code = cef_string_from_str(&script);
                            let cef_url = cef_string_from_str("joren://app/event");
                            exec_js(frame, &cef_code, &cef_url, 0);
                        }
                    }
                }
            }
        }
    }
}

pub fn update_browser_surface_size(width: i32, height: i32) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
        };

        let guard = SURFACE_CONTROLLER.lock().unwrap();
        if let Some(ctrl) = guard.as_ref() {
            if ctrl.main_view_hwnd != 0 && width > 0 && height > 0 {
                SetWindowPos(
                    ctrl.main_view_hwnd as *mut c_void,
                    std::ptr::null_mut(),
                    0,
                    0,
                    width,
                    height,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }
}

pub fn update_browser_surface(_bounds: Option<cef_rect_t>, show: bool) {
    // Website tabs are positioned by cef::tabs. This must not hide the React shell.
    log_startup(
        "TAB",
        &format!("Legacy surface request show={show} does not affect the main UI browser"),
    );
}

#[repr(C)]
pub struct JorenLifeSpanHandler {
    pub base: _cef_life_span_handler_t,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenLifeSpanHandler {
    pub fn new() -> *mut _cef_life_span_handler_t {
        let mut base: _cef_life_span_handler_t = unsafe { std::mem::zeroed() };
        base.base.size = std::mem::size_of::<_cef_life_span_handler_t>();
        base.base.add_ref = Some(Self::add_ref);
        base.base.release = Some(Self::release);
        base.base.has_one_ref = Some(Self::has_one_ref);
        base.base.has_at_least_one_ref = Some(Self::has_at_least_one_ref);
        base.on_after_created = Some(Self::on_after_created);
        base.do_close = Some(Self::do_close);
        base.on_before_close = Some(Self::on_before_close);

        let handler = Box::new(Self {
            base,
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(handler) as *mut _cef_life_span_handler_t
    }

    unsafe extern "C" fn add_ref(base: *mut _cef_base_ref_counted_t) {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log_startup("LIFESPAN_REF", &format!("add_ref: {} -> {}", prev, prev + 1));
    }

    unsafe extern "C" fn release(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release);
        log_startup("LIFESPAN_REF", &format!("release: {} -> {}", prev, prev - 1));
        if prev == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            let _ = Box::from_raw(base as *mut Self);
            log_startup("LIFESPAN_REF", "JorenLifeSpanHandler DEALLOCATED");
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn has_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) == 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn has_at_least_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) >= 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn on_after_created(
        _self: *mut _cef_life_span_handler_t,
        browser: *mut _cef_browser_t,
    ) {
        if !browser.is_null() {
            let mut child_hwnd = 0isize;
            if let Some(get_host) = (*browser).get_host {
                let host = get_host(browser);
                if !host.is_null() {
                    if let Some(get_win) = (*host).get_window_handle {
                        let handle = get_win(host);
                        child_hwnd = handle.0 as isize;
                    }
                    if let Some(release) = (*host).base.release {
                        release(&mut (*host).base);
                    }
                }
            }
            if tabs::claim_created_browser(browser, child_hwnd) {
                log_startup("BROWSER", &format!("Website tab browser created. Child HWND: {child_hwnd:#x}"));
                return;
            }
            set_main_browser(browser);
            set_main_view_hwnd(child_hwnd);
            log_startup("BROWSER", &format!("App UI browser created. Child HWND: {child_hwnd:#x}"));
        }
    }

    unsafe extern "C" fn do_close(
        _self: *mut _cef_life_span_handler_t,
        _browser: *mut _cef_browser_t,
    ) -> c_int {
        0
    }

    unsafe extern "C" fn on_before_close(
        _self: *mut _cef_life_span_handler_t,
        browser: *mut _cef_browser_t,
    ) {
        let is_tab = tabs::note_closed(browser);
        if !is_tab {
            log_startup("BROWSER", "Main application browser window is closing.");
            clear_main_browser();
        } else {
            log_startup("BROWSER", "Website tab browser closed.");
        }
    }
}

#[repr(C)]
pub struct JorenLoadHandler {
    pub base: _cef_load_handler_t,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenLoadHandler {
    pub fn new() -> *mut _cef_load_handler_t {
        let mut base: _cef_load_handler_t = unsafe { std::mem::zeroed() };
        base.base.size = std::mem::size_of::<_cef_load_handler_t>();
        base.base.add_ref = Some(Self::add_ref);
        base.base.release = Some(Self::release);
        base.base.has_one_ref = Some(Self::has_one_ref);
        base.base.has_at_least_one_ref = Some(Self::has_at_least_one_ref);
        base.on_load_end = Some(Self::on_load_end);
        base.on_load_error = Some(Self::on_load_error);

        let handler = Box::new(Self {
            base,
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(handler) as *mut _cef_load_handler_t
    }

    unsafe extern "C" fn add_ref(base: *mut _cef_base_ref_counted_t) {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log_startup("LOAD_REF", &format!("add_ref: {} -> {}", prev, prev + 1));
    }

    unsafe extern "C" fn release(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release);
        log_startup("LOAD_REF", &format!("release: {} -> {}", prev, prev - 1));
        if prev == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            let _ = Box::from_raw(base as *mut Self);
            log_startup("LOAD_REF", "JorenLoadHandler DEALLOCATED");
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn has_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) == 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn has_at_least_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) >= 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn on_load_end(
        _self: *mut _cef_load_handler_t,
        _browser: *mut _cef_browser_t,
        frame: *mut _cef_frame_t,
        http_status_code: c_int,
    ) {
        if !frame.is_null() {
            let f = &*frame;
            if let Some(is_main) = f.is_main {
                if is_main(frame) != 0 {
                    let page_url = if let Some(get_url) = f.get_url {
                        let raw = get_url(frame);
                        cef_string_userfree_to_string_and_free(raw)
                    } else {
                        String::new()
                    };
                    tabs::note_navigation(_browser, page_url.clone());
                    log_startup("LOAD", &format!("Main frame loaded (HTTP status: {http_status_code}) url={page_url}"));
                    if std::env::args().any(|a| a == "--smoke-test") {
                        if let Some(exec_js) = f.execute_java_script {
                            log_startup("SMOKE_TEST", "Injecting smoke-test JavaScript probe into main frame...");
                            let script = cef_string_from_str(r#"
                                (async function() {
                                    console.log("[SMOKE_TEST] Main frame URL:", window.location.href);
                                    console.log("[SMOKE_TEST] Document readyState:", document.readyState);
                                    console.log("[SMOKE_TEST] HTML preview:", (document.documentElement ? document.documentElement.outerHTML.substring(0, 300) : "null"));
                                    const scriptElements = Array.from(document.querySelectorAll('script'));
                                    const scripts = scriptElements.map(s => ({ src: s.src, type: s.type }));
                                    console.log("[SMOKE_TEST] Script tags in DOM:", JSON.stringify(scripts));

                                    for (const s of scriptElements) {
                                        if (s.src) {
                                            try {
                                                const r = await fetch(s.src);
                                                console.log("[SMOKE_TEST] fetch " + s.src + " -> status: " + r.status + ", type: " + r.headers.get("content-type"));
                                            } catch (err) {
                                                console.error("[SMOKE_TEST] fetch " + s.src + " FAILED:", err);
                                            }
                                        }
                                    }

                                    let attempts = 0;
                                    const maxAttempts = 220;
                                    const check = setInterval(async () => {
                                        attempts++;
                                        if (attempts % 10 === 0) {
                                            console.log("[SMOKE_TEST] waiting for window.__JOREN_APP_READY__ (attempt " + attempts + "), script_loaded=" + !!window.__JOREN_SCRIPT_LOADED__);
                                        }
                                        if (window.__JOREN_APP_READY__) {
                                            clearInterval(check);
                                            console.log("[SMOKE_TEST] window.__JOREN_APP_READY__ confirmed true in page!");
                                            let eventSeen = false;
                                            window.addEventListener('joren-event', (e) => {
                                                if (e.detail && e.detail.event === 'smoke-test-event') eventSeen = true;
                                            });
                                            try {
                                                // Test policy command resolution
                                                const polRes = await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'policy_list_pending_approvals', args: {} })
                                                });
                                                console.log("[SMOKE_TEST] policy_list_pending_approvals status:", polRes.status);

                                                // Test native CEF browser tab lifecycle, DOM observation & screenshot
                                                const tabRes = await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({
                                                        cmd: 'browser_create_tab',
                                                        args: { url: 'data:text/html,<html><head><title>SmokeTestPage</title></head><body><h1>SmokeHeading</h1><button id=\"smoke-btn\">ClickMe</button><input id=\"smoke-input\" type=\"text\" value=\"init\"/><a href=\"https://example.com\">SmokeLink</a></body></html>' }
                                                    })
                                                });
                                                const tabData = await tabRes.json();
                                                console.log("[SMOKE_TEST] browser_create_tab response:", JSON.stringify(tabData));
                                                const testTabId = tabData.id || (tabData.tab && tabData.tab.id) || 'tab-smoke';

                                                await new Promise((r) => setTimeout(r, 600));

                                                const obsRes = await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'browser_observe_tab', args: { tabId: testTabId } })
                                                });
                                                const obsData = await obsRes.json();
                                                console.log("[SMOKE_TEST] browser_observe_tab status:", obsRes.status, "title:", obsData.title, "interactive_elements:", obsData.interactive_elements ? obsData.interactive_elements.length : 0);

                                                const shotRes = await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'browser_screenshot_tab', args: { tabId: testTabId } })
                                                });
                                                console.log("[SMOKE_TEST] browser_screenshot_tab status:", shotRes.status);

                                                await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'browser_close_tab', args: { tabId: testTabId } })
                                                });
                                                console.log("[SMOKE_TEST] browser_close_tab complete");

                                                const res = await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'smoke_test_verify', args: { react_ready: true } })
                                                });
                                                const body = await res.json();
                                                console.log("[SMOKE_TEST] IPC pingback status:", res.status, JSON.stringify(body));
                                                for (let i = 0; i < 20 && !eventSeen; i++) {
                                                    await new Promise((r) => setTimeout(r, 100));
                                                }
                                                console.log("[SMOKE_TEST] event received:", eventSeen);
                                                await fetch('/api/invoke', {
                                                    method: 'POST',
                                                    headers: { 'Content-Type': 'application/json' },
                                                    body: JSON.stringify({ cmd: 'smoke_test_ack', args: { react_ready: true, event_received: eventSeen } })
                                                });
                                            } catch (e) {
                                                console.error("[SMOKE_TEST] IPC probe failed:", e);
                                            }
                                        } else if (attempts >= maxAttempts) {
                                            clearInterval(check);
                                            console.error("[SMOKE_TEST] Timeout waiting for window.__JOREN_APP_READY__, script_loaded=" + !!window.__JOREN_SCRIPT_LOADED__);
                                        }
                                    }, 100);
                                })();
                            "#);
                            let url = cef_string_from_str("joren://app/smoke-test.js");
                            exec_js(frame, &script, &url, 1);
                        }
                    }
                }
            }
        }
    }

    unsafe extern "C" fn on_load_error(
        _self: *mut _cef_load_handler_t,
        _browser: *mut _cef_browser_t,
        _frame: *mut _cef_frame_t,
        error_code: cef_errorcode_t,
        error_text: *const cef_string_t,
        failed_url: *const cef_string_t,
    ) {
        let err_str = if !error_text.is_null() {
            cef_string_to_string(&*error_text)
        } else {
            String::new()
        };
        let url_str = if !failed_url.is_null() {
            cef_string_to_string(&*failed_url)
        } else {
            String::new()
        };
        log_startup("LOAD_ERROR", &format!("Load error code {:?}: {} URL: {}", error_code, err_str, url_str));
    }
}

#[repr(C)]
pub struct JorenCefClient {
    pub base: _cef_client_t,
    pub life_span: *mut _cef_life_span_handler_t,
    pub load: *mut _cef_load_handler_t,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenCefClient {
    pub fn new() -> *mut _cef_client_t {
        let life_span = JorenLifeSpanHandler::new();
        let load = JorenLoadHandler::new();
        let mut base: _cef_client_t = unsafe { std::mem::zeroed() };
        base.base.size = std::mem::size_of::<_cef_client_t>();
        base.base.add_ref = Some(Self::add_ref);
        base.base.release = Some(Self::release);
        base.base.has_one_ref = Some(Self::has_one_ref);
        base.base.has_at_least_one_ref = Some(Self::has_at_least_one_ref);
        base.get_life_span_handler = Some(Self::get_life_span_handler);
        base.get_load_handler = Some(Self::get_load_handler);

        let client = Box::new(Self {
            base,
            life_span,
            load,
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(client) as *mut _cef_client_t
    }

    unsafe extern "C" fn add_ref(base: *mut _cef_base_ref_counted_t) {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log_startup("CLIENT_REF", &format!("add_ref: {} -> {}", prev, prev + 1));
    }

    unsafe extern "C" fn release(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release);
        log_startup("CLIENT_REF", &format!("release: {} -> {}", prev, prev - 1));
        if prev == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            let client = Box::from_raw(base as *mut Self);
            if !client.life_span.is_null() {
                if let Some(rel) = (*client.life_span).base.release {
                    rel(&mut (*client.life_span).base);
                }
            }
            if !client.load.is_null() {
                if let Some(rel) = (*client.load).base.release {
                    rel(&mut (*client.load).base);
                }
            }
            log_startup("CLIENT_REF", "JorenCefClient DEALLOCATED");
            1
        } else {
            0
        }
    }

    unsafe extern "C" fn has_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) == 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn has_at_least_one_ref(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        if this.ref_count.load(std::sync::atomic::Ordering::Relaxed) >= 1 { 1 } else { 0 }
    }

    unsafe extern "C" fn get_life_span_handler(self_: *mut _cef_client_t) -> *mut _cef_life_span_handler_t {
        let this = (self_ as *mut Self).as_ref().unwrap();
        if !this.life_span.is_null() {
            if let Some(add_ref) = (*this.life_span).base.add_ref {
                add_ref(&mut (*this.life_span).base);
            }
        }
        this.life_span
    }

    unsafe extern "C" fn get_load_handler(self_: *mut _cef_client_t) -> *mut _cef_load_handler_t {
        let this = (self_ as *mut Self).as_ref().unwrap();
        if !this.load.is_null() {
            if let Some(add_ref) = (*this.load).base.add_ref {
                add_ref(&mut (*this.load).base);
            }
        }
        this.load
    }
}

/// Official CEF Entry Point exported by joren.dll
/// Called by bootstrap.exe (renamed to joren.exe) with Windows Sandbox context
#[no_mangle]
pub unsafe extern "system" fn RunWinMain(
    h_instance: isize,
    _lp_cmd_line: *const u16,
    _n_cmd_show: i32,
    sandbox_info: *mut c_void,
    version_info: *const c_void,
) -> i32 {
    run_cef_main(h_instance, sandbox_info, version_info)
}

/// Official CEF Console Entry Point exported by joren.dll
#[no_mangle]
pub unsafe extern "system" fn RunConsoleMain(
    _argc: i32,
    _argv: *const *const u8,
    sandbox_info: *mut c_void,
    version_info: *const c_void,
) -> i32 {
    run_cef_main(0, sandbox_info, version_info)
}

unsafe fn run_cef_main(
    h_instance: isize,
    sandbox_info: *mut c_void,
    version_info: *const c_void,
) -> i32 {
    let role = get_process_role();
    let elevated_str = match is_process_elevated() {
        Some(true) => "elevated=true",
        Some(false) => "elevated=false",
        None => "elevated=unknown",
    };
    log_startup(
        "BOOTSTRAP",
        &format!("RunWinMain entered: role={}, {}", role, elevated_str),
    );

    // -------------------------------------------------------------
    // MANDATORY SECURITY GATE: Fail-Closed if sandbox_info is NULL
    // -------------------------------------------------------------
    if sandbox_info.is_null() {
        let err_msg = "CEF Windows sandbox is REQUIRED but sandbox_info was NULL.\nApplication will NOT start unsandboxed.\nPlease launch via joren.exe.";
        log_startup("SECURITY", err_msg);
        show_fatal_error_dialog("Joren Security Error", err_msg);
        return 1;
    }

    // Inspect version_info
    if !version_info.is_null() {
        let v = &*(version_info as *const cef_version_info_t);
        log_startup("VERSION", &format!(
            "CEF: {}.{}.{} (commit {}) | Chrome: {}.{}.{}.{}",
            v.cef_version_major, v.cef_version_minor, v.cef_version_patch, v.cef_commit_number,
            v.chrome_version_major, v.chrome_version_minor, v.chrome_version_build, v.chrome_version_patch
        ));
    }

    let api = match get_cef_api() {
        Some(a) => a,
        None => {
            let msg = "Could not load required symbols from libcef.dll.\nPlease verify libcef.dll exists in application directory.";
            log_startup("LOAD_ERROR", msg);
            show_fatal_error_dialog("Joren Runtime Error", msg);
            return 1;
        }
    };

    // Configure CEF API version via cef_api_hash to ensure wrapper CToCpp checks pass
    if let Some(hash_fn) = api.api_hash {
        let _ = hash_fn(CEF_API_VERSION_LAST, 0);
        if let Some(ver_fn) = api.api_version {
            let ver = ver_fn();
            log_startup("VERSION", &format!("Configured CEF API version: {}", ver));
        }
    }

    let main_args = cef_main_args_t {
        instance: HINSTANCE(h_instance as *mut _),
    };

    let app_sub = JorenCefApp::new();

    // Subprocess execution: handles renderer, gpu, utility, etc.
    let exit_code = (api.execute_process)(&main_args, app_sub, sandbox_info);
    if exit_code >= 0 {
        return exit_code;
    }

    // -------------------------------------------------------------
    // Main Browser Process Initialization
    // -------------------------------------------------------------
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
        let class_name = windows_sys::core::w!("JorenMainWindow");
        let existing_hwnd = unsafe { FindWindowW(class_name, windows_sys::core::w!("Joren")) };
        if !existing_hwnd.is_null() {
            log_startup("SINGLE_INSTANCE", &format!("Existing Joren window found (hwnd={:?}). Activating window and exiting new process cleanly.", existing_hwnd));
            unsafe {
                ShowWindow(existing_hwnd, SW_RESTORE);
                SetForegroundWindow(existing_hwnd);
            }
            return 0;
        }
    }

    let mut settings: cef_settings_t = std::mem::zeroed();
    settings.size = std::mem::size_of::<cef_settings_t>();
    settings.no_sandbox = 0; // 0 = Sandbox ENABLED (Mandatory)
    settings.multi_threaded_message_loop = 0; // Standard message pump
    settings.background_color = 0xFF121417;

    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let joren_data_dir = std::path::PathBuf::from(&appdata).join("Joren");
    let cef_cache_path = joren_data_dir.join("cache");
    let cef_log_path = joren_data_dir.join("logs").join("cef.log");
    let _ = std::fs::create_dir_all(&cef_cache_path);
    if let Some(parent) = cef_log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    settings.root_cache_path = cef_string_from_str(&cef_cache_path.to_string_lossy());
    settings.cache_path = cef_string_from_str(&cef_cache_path.to_string_lossy());
    settings.log_file = cef_string_from_str(&cef_log_path.to_string_lossy());
    settings.log_severity = cef_log_severity_t::LOGSEVERITY_INFO;

    let app_main = JorenCefApp::new();
    let init_res = (api.initialize)(
        &main_args,
        &settings,
        app_main,
        sandbox_info,
    );

    if init_res == 0 {
        let exit_code = if let Some(get_code) = api.get_exit_code {
            get_code()
        } else {
            0
        };
        log_startup("INIT", &format!("cef_initialize returned 0. Process exit code: {}", exit_code));

        if exit_code == RESULT_CODE_NORMAL_EXIT {
            log_startup("INIT", "Normal singleton early exit (code 0). Terminating second process cleanly.");
            return RESULT_CODE_NORMAL_EXIT;
        }

        if exit_code == RESULT_CODE_NORMAL_EXIT_AUTO_DE_ELEVATED {
            log_startup("INIT", "Chromium auto-de-elevation handoff (code 38). Elevated parent process exiting cleanly while unelevated child continues.");
            return RESULT_CODE_NORMAL_EXIT_AUTO_DE_ELEVATED;
        }

        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
            let class_name = windows_sys::core::w!("JorenMainWindow");
            let existing_hwnd = FindWindowW(class_name, windows_sys::core::w!("Joren"));
            if !existing_hwnd.is_null() {
                log_startup("INIT", &format!("Found active Joren window {:?}; bringing to foreground.", existing_hwnd));
                ShowWindow(existing_hwnd, SW_RESTORE);
                SetForegroundWindow(existing_hwnd);
                return 0;
            }
        }

        let msg = format!("CEF initialization (cef_initialize) failed with exit code {}.\n\nLogs: {}\nCEF: 156.0.15", exit_code, cef_log_path.display());
        log_startup("INIT_ERROR", &msg);
        show_fatal_error_dialog("Joren Runtime Error", &msg);
        return exit_code;
    }
    log_startup("INIT", "cef_initialize succeeded.");

    // Register custom scheme handler factory: joren://app and joren:// (all domains)
    let scheme_name = cef_string_from_str("joren");
    let domain_name = cef_string_from_str("app");
    let factory = JorenSchemeHandlerFactory::new();
    let reg_res = (api.register_scheme_handler_factory)(&scheme_name, &domain_name, factory);
    if reg_res == 0 {
        let msg = "cef_register_scheme_handler_factory failed for joren://app";
        log_startup("SCHEME_ERROR", msg);
        show_fatal_error_dialog("Joren Scheme Error", msg);
        return 1;
    }
    log_startup("SCHEME", "Registered scheme handler factory for joren://app (result=1)");

    let empty_domain = cef_string_from_str("");
    let factory_global = JorenSchemeHandlerFactory::new();
    let reg_res_global = (api.register_scheme_handler_factory)(&scheme_name, &empty_domain, factory_global);
    log_startup("SCHEME", &format!("Registered global scheme handler factory for joren:// (result={})", reg_res_global));

    // Initialize backend core (database, settings, providers, tools, voice)
    let app_handle = tauri::AppHandle::default();
    let core = match crate::core::JorenCore::initialize(&app_handle) {
        Ok(c) => Arc::new(c),
        Err(err) => {
            let msg = format!("Failed to initialize backend services:\n{}", err);
            log_startup("CORE_ERROR", &msg);
            show_fatal_error_dialog("Joren Backend Error", &msg);
            return 1;
        }
    };
    log_startup("CORE", "Backend core services initialized.");

    // Wire command dispatcher to scheme handler
    let core_dispatch = core.clone();
    scheme::set_command_dispatcher(Arc::new(move |cmd, args| {
        core_dispatch.dispatch(cmd, args)
    }));
    log_startup("IPC", "IPC command dispatcher registered.");

    // Wire native event dispatcher
    app_handle.set_event_dispatcher(|event, payload| {
        dispatch_event_to_browser(event, payload);
    });

    // Verify core dispatch responsiveness before continuing
    match core.dispatch("ping", &serde_json::json!({})) {
        Ok(val) => {
            log_startup("CORE_PING", &format!("Internal core ping succeeded: {}", val));
        }
        Err(e) => {
            let msg = format!("Backend core failed to respond to ping:\n{}", e);
            log_startup("CORE_PING", &msg);
            show_fatal_error_dialog("Joren Core Error", &msg);
            return 1;
        }
    }

    // Check for --smoke-test CLI flag: Spawn watchdog to verify full browser + React UI load + IPC roundtrip
    let is_smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    if is_smoke_test {
        log_startup("SMOKE_TEST", "Spawning smoke-test watchdog thread (25s timeout)...");
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let timeout = std::time::Duration::from_secs(25);
            while start.elapsed() < timeout {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if is_smoke_test_passed() {
                    log_startup("SMOKE_TEST", "Verified React UI mounted & frontend-to-Rust IPC roundtrip succeeded!");
                    log_startup("SMOKE_TEST", "All smoke test checks PASSED cleanly. Exiting message loop.");
                    close_main_window();
                    return;
                }
            }
            log_startup("SMOKE_TEST", "FATAL: Watchdog timeout (25s) waiting for React UI mounting and smoke_test_verify IPC!");
            std::process::exit(1);
        });
    }

    // Initialize Win32 Host Window
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{GetLastError, HWND};
        use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassExW, ShowWindow, CS_HREDRAW, CS_VREDRAW,
            SW_SHOW, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
        };

        unsafe extern "system" fn wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                DestroyWindow, PostQuitMessage, WM_CLOSE, WM_DESTROY, WM_SIZE,
            };

            match msg {
                tabs::WM_JOREN_EXEC => {
                    tabs::drain_ui_queue();
                    0
                }
                WM_SIZE => {
                    let width = (lparam & 0xFFFF) as i32;
                    let height = ((lparam >> 16) & 0xFFFF) as i32;
                    update_browser_surface_size(width, height);
                    0
                }
                WM_CLOSE => {
                    // Terminate child local LLM server if active
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/IM", "llama-server.exe"])
                        .output();
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    if let Some(api) = get_cef_api() {
                        (api.quit_message_loop)();
                    }
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }

        let h_module = if h_instance != 0 {
            h_instance as *mut c_void
        } else {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()) as *mut c_void
        };

        let class_name = windows_sys::core::w!("JorenMainWindow");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_module,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: CreateSolidBrush(0x00171412),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name,
            hIconSm: std::ptr::null_mut(),
        };

        let atom = RegisterClassExW(&wnd_class);
        if atom == 0 {
            let err = GetLastError();
            if err != 1410 {
                let msg = format!("RegisterClassExW failed (GetLastError={err})");
                log_startup("WINDOW", &msg);
                show_fatal_error_dialog("Joren Window Error", &msg);
                return 1;
            }
        }

        let main_hwnd = CreateWindowExW(
            0,
            class_name,
            windows_sys::core::w!("Joren"),
            WS_OVERLAPPEDWINDOW | 0x02000000, // WS_CLIPCHILDREN
            100,
            100,
            1400,
            900,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_module,
            std::ptr::null(),
        );

        if main_hwnd.is_null() {
            let err = GetLastError();
            let msg = format!("CreateWindowExW failed (GetLastError={err})");
            log_startup("WINDOW", &msg);
            show_fatal_error_dialog("Joren Window Error", &msg);
            return 1;
        }
        init_surface_controller(main_hwnd as isize);
        ShowWindow(main_hwnd, SW_SHOW);
        log_startup("WINDOW", &format!("Host window created hwnd={:?}", main_hwnd));

        let mut win_info: cef_window_info_t = std::mem::zeroed();
        win_info.size = std::mem::size_of::<cef_window_info_t>();
        win_info.parent_window = HWND(main_hwnd as *mut _);
        // WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS
        win_info.style = 0x54000000;
        win_info.bounds = cef_rect_t {
            x: 0,
            y: 0,
            width: 1400,
            height: 900,
        };

        let mut b_settings: cef_browser_settings_t = std::mem::zeroed();
        b_settings.size = std::mem::size_of::<cef_browser_settings_t>();
        b_settings.javascript = cef_state_t::STATE_ENABLED;
        b_settings.local_storage = cef_state_t::STATE_ENABLED;
        b_settings.webgl = cef_state_t::STATE_ENABLED;

        let app_url = cef_string_from_str("joren://app/index.html");
        let client = JorenCefClient::new();

        let created = (api.browser_host_create_browser)(
            &win_info,
            client,
            &app_url,
            &b_settings,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        if created == 0 {
            log_startup("BROWSER_ERROR", "cef_browser_host_create_browser returned 0.");
            show_fatal_error_dialog("Joren Browser Error", "Failed to create main browser window.");
            return 1;
        }
        log_startup("BROWSER", "Browser creation initiated.");
    }

    tabs::set_ui_thread();
    // Enter standard CEF message loop
    (api.run_message_loop)();

    // Clean shutdown
    (api.shutdown)();
    log_startup("SHUTDOWN", "Application exited cleanly.");

    0
}
