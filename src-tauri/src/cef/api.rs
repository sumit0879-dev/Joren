use super::types::*;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::OnceLock;

pub type FnCefExecuteProcess = unsafe extern "C" fn(
    *const cef_main_args_t,
    *mut cef_app_t,
    *mut c_void,
) -> c_int;

pub type FnCefInitialize = unsafe extern "C" fn(
    *const cef_main_args_t,
    *const cef_settings_t,
    *mut cef_app_t,
    *mut c_void,
) -> c_int;

pub type FnCefShutdown = unsafe extern "C" fn();
pub type FnCefDoMessageLoopWork = unsafe extern "C" fn();
pub type FnCefRunMessageLoop = unsafe extern "C" fn();
pub type FnCefQuitMessageLoop = unsafe extern "C" fn();

pub type FnCefBrowserHostCreateBrowser = unsafe extern "C" fn(
    *const cef_window_info_t,
    *mut cef_client_t,
    *const cef_string_t,
    *const cef_browser_settings_t,
    *mut cef_dictionary_value_t,
    *mut cef_request_context_t,
) -> c_int;

pub type FnCefRegisterSchemeHandlerFactory = unsafe extern "C" fn(
    *const cef_string_t,
    *const cef_string_t,
    *mut cef_scheme_handler_factory_t,
) -> c_int;

pub type FnCefStringUtf16Set = unsafe extern "C" fn(
    *const u16,
    usize,
    *mut cef_string_utf16_t,
    c_int,
) -> c_int;

pub type FnCefStringUtf16Clear = unsafe extern "C" fn(*mut cef_string_utf16_t);
pub type FnCefStringUserfreeUtf16Free = unsafe extern "C" fn(cef_string_userfree_utf16_t);
pub type FnCefApiVersion = unsafe extern "C" fn() -> c_int;
pub type FnCefApiHash = unsafe extern "C" fn(std::os::raw::c_int, std::os::raw::c_int) -> *const std::os::raw::c_char;

pub struct CefApi {
    pub execute_process: FnCefExecuteProcess,
    pub initialize: FnCefInitialize,
    pub shutdown: FnCefShutdown,
    pub do_message_loop_work: FnCefDoMessageLoopWork,
    pub run_message_loop: FnCefRunMessageLoop,
    pub quit_message_loop: FnCefQuitMessageLoop,
    pub browser_host_create_browser: FnCefBrowserHostCreateBrowser,
    pub register_scheme_handler_factory: FnCefRegisterSchemeHandlerFactory,
    pub string_utf16_set: FnCefStringUtf16Set,
    pub string_utf16_clear: FnCefStringUtf16Clear,
    pub string_userfree_utf16_free: Option<FnCefStringUserfreeUtf16Free>,
    pub api_version: Option<FnCefApiVersion>,
    pub api_hash: Option<FnCefApiHash>,
}

static CEF_API: OnceLock<Option<CefApi>> = OnceLock::new();

#[cfg(target_os = "windows")]
pub fn get_cef_api() -> Option<&'static CefApi> {
    CEF_API.get_or_init(|| {
        unsafe {
            use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};

            // Attempt to load libcef.dll from current process module or executable folder
            let mut handle = GetModuleHandleW(windows_sys::core::w!("libcef.dll"));
            if handle.is_null() {
                handle = LoadLibraryW(windows_sys::core::w!("libcef.dll"));
            }
            if handle.is_null() {
                if let Ok(mut exe_dir) = std::env::current_exe() {
                    exe_dir.pop();
                    let cef_path = exe_dir.join("libcef.dll");
                    let mut wide: Vec<u16> = cef_path.to_string_lossy().encode_utf16().collect();
                    wide.push(0);
                    handle = LoadLibraryW(wide.as_ptr());
                }
            }

            if handle.is_null() {
                eprintln!("[CEF API] libcef.dll could not be loaded into current process.");
                return None;
            }

            macro_rules! load_sym {
                ($name:ident, $sym:literal, $t:ty) => {
                    let ptr = GetProcAddress(handle, concat!($sym, "\0").as_ptr());
                    if ptr.is_none() {
                        eprintln!("[CEF API] Missing required symbol: {}", $sym);
                        return None;
                    }
                    let $name: $t = std::mem::transmute(ptr);
                };
            }

            macro_rules! load_opt_sym {
                ($name:ident, $sym:literal, $t:ty) => {
                    let ptr = GetProcAddress(handle, concat!($sym, "\0").as_ptr());
                    let $name: Option<$t> = if let Some(p) = ptr {
                        Some(std::mem::transmute(p))
                    } else {
                        None
                    };
                };
            }

            load_sym!(execute_process, "cef_execute_process", FnCefExecuteProcess);
            load_sym!(initialize, "cef_initialize", FnCefInitialize);
            load_sym!(shutdown, "cef_shutdown", FnCefShutdown);
            load_sym!(do_message_loop_work, "cef_do_message_loop_work", FnCefDoMessageLoopWork);
            load_sym!(run_message_loop, "cef_run_message_loop", FnCefRunMessageLoop);
            load_sym!(quit_message_loop, "cef_quit_message_loop", FnCefQuitMessageLoop);
            load_sym!(browser_host_create_browser, "cef_browser_host_create_browser", FnCefBrowserHostCreateBrowser);
            load_sym!(register_scheme_handler_factory, "cef_register_scheme_handler_factory", FnCefRegisterSchemeHandlerFactory);
            load_sym!(string_utf16_set, "cef_string_utf16_set", FnCefStringUtf16Set);
            load_sym!(string_utf16_clear, "cef_string_utf16_clear", FnCefStringUtf16Clear);
            load_opt_sym!(string_userfree_utf16_free, "cef_string_userfree_utf16_free", FnCefStringUserfreeUtf16Free);
            load_opt_sym!(api_version, "cef_api_version", FnCefApiVersion);
            load_opt_sym!(api_hash, "cef_api_hash", FnCefApiHash);

            Some(CefApi {
                execute_process,
                initialize,
                shutdown,
                do_message_loop_work,
                run_message_loop,
                quit_message_loop,
                browser_host_create_browser,
                register_scheme_handler_factory,
                string_utf16_set,
                string_utf16_clear,
                string_userfree_utf16_free,
                api_version,
                api_hash,
            })
        }
    }).as_ref()
}

#[cfg(not(target_os = "windows"))]
pub fn get_cef_api() -> Option<&'static CefApi> {
    None
}

pub fn cef_string_from_str(s: &str) -> cef_string_t {
    let mut cef_str = cef_string_t {
        str_: std::ptr::null_mut(),
        length: 0,
        dtor: None,
    };
    let wide: Vec<u16> = s.encode_utf16().collect();
    if let Some(api) = get_cef_api() {
        unsafe {
            (api.string_utf16_set)(wide.as_ptr(), wide.len(), &mut cef_str, 1);
        }
    }
    cef_str
}

pub fn cef_string_to_string(cef_str: &cef_string_t) -> String {
    if cef_str.str_.is_null() || cef_str.length == 0 {
        return String::new();
    }
    unsafe {
        let slice = std::slice::from_raw_parts(cef_str.str_, cef_str.length);
        String::from_utf16_lossy(slice)
    }
}

pub unsafe fn cef_string_userfree_to_string_and_free(userfree: cef_string_userfree_t) -> String {
    if userfree.is_null() {
        return String::new();
    }
    let res = cef_string_to_string(&*userfree);
    if let Some(api) = get_cef_api() {
        if let Some(free_fn) = api.string_userfree_utf16_free {
            free_fn(userfree);
        }
    }
    res
}
