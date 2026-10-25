pub mod api;
pub mod scheme;
pub mod types;

use api::{cef_string_from_str, get_cef_api};
use scheme::{set_command_dispatcher, CommandDispatcher, JorenSchemeHandlerFactory};
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::{Arc, Mutex};
use types::*;

pub struct BrowserSurfaceController {
    main_window_hwnd: isize,
    browser_tab_hwnd: isize,
    is_browser_visible: bool,
    bounds: _cef_rect_t,
}

static SURFACE_CONTROLLER: Mutex<Option<BrowserSurfaceController>> = Mutex::new(None);

pub fn init_surface_controller(main_hwnd: isize) {
    let mut guard = SURFACE_CONTROLLER.lock().unwrap();
    *guard = Some(BrowserSurfaceController {
        main_window_hwnd: main_hwnd,
        browser_tab_hwnd: 0,
        is_browser_visible: false,
        bounds: _cef_rect_t::default(),
    });
}

pub fn set_browser_child_hwnd(child_hwnd: isize) {
    let mut guard = SURFACE_CONTROLLER.lock().unwrap();
    if let Some(ctrl) = guard.as_mut() {
        ctrl.browser_tab_hwnd = child_hwnd;
    }
}

pub fn update_browser_surface(bounds: Option<_cef_rect_t>, show: bool) {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW,
        };

        let mut guard = SURFACE_CONTROLLER.lock().unwrap();
        if let Some(ctrl) = guard.as_mut() {
            ctrl.is_browser_visible = show;
            if let Some(b) = bounds {
                ctrl.bounds = b;
            }

            if ctrl.browser_tab_hwnd != 0 {
                if show && ctrl.bounds.width > 0 && ctrl.bounds.height > 0 {
                    SetWindowPos(
                        ctrl.browser_tab_hwnd as *mut c_void,
                        std::ptr::null_mut(),
                        ctrl.bounds.x,
                        ctrl.bounds.y,
                        ctrl.bounds.width,
                        ctrl.bounds.height,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    ShowWindow(ctrl.browser_tab_hwnd as *mut c_void, SW_SHOW);
                } else {
                    ShowWindow(ctrl.browser_tab_hwnd as *mut c_void, SW_HIDE);
                }
            }
        }
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
    _version_info: *const c_void,
) -> i32 {
    // -------------------------------------------------------------
    // MANDATORY SECURITY GATE: Fail-Closed if sandbox_info is NULL
    // -------------------------------------------------------------
    if sandbox_info.is_null() {
        eprintln!("[FATAL SECURITY ERROR] CEF Windows sandbox is REQUIRED but sandbox_info was NULL!");
        eprintln!("[SECURITY GATE] Failing closed: Application will NOT start unsandboxed.");
        return 1;
    }

    let api = match get_cef_api() {
        Some(a) => a,
        None => {
            eprintln!("[FATAL ERROR] Could not load CEF API from libcef.dll.");
            return 1;
        }
    };

    let main_args = _cef_main_args_t {
        instance: h_instance,
    };

    // Subprocess execution: handles renderer, gpu, utility, etc.
    let exit_code = (api.execute_process)(&main_args, std::ptr::null_mut(), sandbox_info);
    if exit_code >= 0 {
        return exit_code;
    }

    // -------------------------------------------------------------
    // Main Browser Process Initialization
    // -------------------------------------------------------------
    let mut settings: _cef_settings_t = std::mem::zeroed();
    settings.size = std::mem::size_of::<_cef_settings_t>();
    settings.no_sandbox = 0; // 0 = Sandbox ENABLED
    settings.multi_threaded_message_loop = 0; // Use standard message loop
    settings.chrome_runtime = 0;

    let init_res = (api.initialize)(
        &main_args,
        &settings,
        std::ptr::null_mut(),
        sandbox_info,
    );

    if init_res == 0 {
        eprintln!("[FATAL ERROR] cef_initialize failed.");
        return 1;
    }

    // Register custom scheme: joren://
    let scheme_name = cef_string_from_str("joren");
    let domain_name = cef_string_from_str("app");
    let factory = JorenSchemeHandlerFactory::new();
    (api.register_scheme_handler_factory)(&scheme_name, &domain_name, factory);

    // Initialize Win32 Host Window
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
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
                WM_SIZE => {
                    // Update layout
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
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }

        let class_name = windows_sys::core::w!("JorenMainWindow");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance as *mut c_void,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name,
            hIconSm: std::ptr::null_mut(),
        };

        RegisterClassExW(&wnd_class);

        let main_hwnd = CreateWindowExW(
            0,
            class_name,
            windows_sys::core::w!("Joren"),
            WS_OVERLAPPEDWINDOW,
            100,
            100,
            1400,
            900,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance as *mut c_void,
            std::ptr::null(),
        );

        init_surface_controller(main_hwnd as isize);
        ShowWindow(main_hwnd, SW_SHOW);

        // Create main UI browser window attached to host HWND
        let mut win_info: _cef_window_info_t = std::mem::zeroed();
        win_info.parent_window = main_hwnd as isize;
        win_info.style = 0x50000000; // WS_CHILD | WS_VISIBLE
        win_info.bounds = _cef_rect_t {
            x: 0,
            y: 0,
            width: 1400,
            height: 900,
        };

        let mut b_settings: _cef_browser_settings_t = std::mem::zeroed();
        b_settings.size = std::mem::size_of::<_cef_browser_settings_t>();
        b_settings.javascript = 1;
        b_settings.local_storage = 1;
        b_settings.databases = 1;

        let app_url = cef_string_from_str("joren://app/index.html");

        (api.browser_host_create_browser)(
            &win_info,
            std::ptr::null_mut(),
            &app_url,
            &b_settings,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // Enter standard CEF message loop
    (api.run_message_loop)();

    // Clean shutdown
    (api.shutdown)();

    0
}
