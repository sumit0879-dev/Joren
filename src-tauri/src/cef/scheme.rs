use super::api::{cef_string_from_str, cef_string_to_string};
use super::types::*;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type CommandDispatcher = Arc<dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;

static DISPATCHER: Mutex<Option<CommandDispatcher>> = Mutex::new(None);

pub fn set_command_dispatcher(d: CommandDispatcher) {
    let mut guard = DISPATCHER.lock().unwrap();
    *guard = Some(d);
}

pub struct JorenResourceHandler {
    pub base: _cef_resource_handler_t,
    pub response_data: Vec<u8>,
    pub response_mime: String,
    pub response_status: c_int,
    pub read_offset: usize,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenResourceHandler {
    pub fn new(data: Vec<u8>, mime: String, status: c_int) -> *mut _cef_resource_handler_t {
        let handler = Box::new(Self {
            base: _cef_resource_handler_t {
                base: _cef_base_ref_counted_t {
                    size: std::mem::size_of::<_cef_resource_handler_t>(),
                    add_ref: Some(Self::add_ref),
                    release: Some(Self::release),
                    has_one_ref: Some(Self::has_one_ref),
                    has_at_least_one_ref: Some(Self::has_at_least_one_ref),
                },
                open: Some(Self::open),
                get_response_headers: Some(Self::get_response_headers),
                read: Some(Self::read),
                cancel: Some(Self::cancel),
            },
            response_data: data,
            response_mime: mime,
            response_status: status,
            read_offset: 0,
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(handler) as *mut _cef_resource_handler_t
    }

    unsafe extern "C" fn add_ref(base: *mut _cef_base_ref_counted_t) {
        let this = (base as *mut Self).as_ref().unwrap();
        this.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    unsafe extern "C" fn release(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release);
        if prev == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            let _ = Box::from_raw(base as *mut Self);
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

    unsafe extern "C" fn open(
        _self: *mut _cef_resource_handler_t,
        _request: *mut _cef_request_t,
        handle_request: *mut c_int,
        _callback: *mut _cef_callback_t,
    ) -> c_int {
        if !handle_request.is_null() {
            *handle_request = 1;
        }
        1
    }

    unsafe extern "C" fn get_response_headers(
        self_: *mut _cef_resource_handler_t,
        response: *mut _cef_response_t,
        response_length: *mut i64,
        _redirect_url: *mut _cef_string_utf16_t,
    ) {
        let this = (self_ as *mut Self).as_mut().unwrap();
        if let Some(resp) = response.as_mut() {
            if let Some(set_status) = resp.set_status {
                set_status(response, this.response_status);
            }
            if let Some(set_mime) = resp.set_mime_type {
                let mime_cef = cef_string_from_str(&this.response_mime);
                set_mime(response, &mime_cef);
            }
        }
        if !response_length.is_null() {
            *response_length = this.response_data.len() as i64;
        }
    }

    unsafe extern "C" fn read(
        self_: *mut _cef_resource_handler_t,
        data_out: *mut c_void,
        bytes_to_read: c_int,
        bytes_read: *mut c_int,
        _callback: *mut _cef_resource_read_callback_t,
    ) -> c_int {
        let this = (self_ as *mut Self).as_mut().unwrap();
        if this.read_offset >= this.response_data.len() {
            if !bytes_read.is_null() {
                *bytes_read = 0;
            }
            return 0;
        }

        let available = this.response_data.len() - this.read_offset;
        let to_copy = std::cmp::min(bytes_to_read as usize, available);
        std::ptr::copy_nonoverlapping(
            this.response_data[this.read_offset..].as_ptr(),
            data_out as *mut u8,
            to_copy,
        );
        this.read_offset += to_copy;
        if !bytes_read.is_null() {
            *bytes_read = to_copy as c_int;
        }
        1
    }

    unsafe extern "C" fn cancel(_self: *mut _cef_resource_handler_t) {}
}

pub struct JorenSchemeHandlerFactory {
    pub base: _cef_scheme_handler_factory_t,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenSchemeHandlerFactory {
    pub fn new() -> *mut _cef_scheme_handler_factory_t {
        let factory = Box::new(Self {
            base: _cef_scheme_handler_factory_t {
                base: _cef_base_ref_counted_t {
                    size: std::mem::size_of::<_cef_scheme_handler_factory_t>(),
                    add_ref: Some(Self::add_ref),
                    release: Some(Self::release),
                    has_one_ref: Some(Self::has_one_ref),
                    has_at_least_one_ref: Some(Self::has_at_least_one_ref),
                },
                create: Some(Self::create),
            },
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(factory) as *mut _cef_scheme_handler_factory_t
    }

    unsafe extern "C" fn add_ref(base: *mut _cef_base_ref_counted_t) {
        let this = (base as *mut Self).as_ref().unwrap();
        this.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    unsafe extern "C" fn release(base: *mut _cef_base_ref_counted_t) -> c_int {
        let this = (base as *mut Self).as_ref().unwrap();
        let prev = this.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Release);
        if prev == 1 {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            let _ = Box::from_raw(base as *mut Self);
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

    unsafe extern "C" fn create(
        _self: *mut _cef_scheme_handler_factory_t,
        _browser: *mut _cef_browser_t,
        _frame: *mut _cef_frame_t,
        _scheme_name: *const _cef_string_utf16_t,
        request: *mut _cef_request_t,
    ) -> *mut _cef_resource_handler_t {
        if request.is_null() {
            return std::ptr::null_mut();
        }

        let req = request.as_ref().unwrap();
        let url_cef = if let Some(get_url) = req.get_url {
            get_url(request)
        } else {
            return std::ptr::null_mut();
        };

        let url_str = cef_string_to_string(&url_cef);

        // Check if IPC endpoint: joren://app/api/invoke
        if url_str.contains("/api/invoke") {
            let mut post_bytes = Vec::new();
            if let Some(get_post_data) = req.get_post_data {
                let post_data = get_post_data(request);
                if !post_data.is_null() {
                    let pd = post_data.as_ref().unwrap();
                    if let Some(get_elements) = pd.get_elements {
                        let mut count = 0usize;
                        get_elements(post_data, &mut count, std::ptr::null_mut());
                        if count > 0 {
                            let mut elements = vec![std::ptr::null_mut(); count];
                            get_elements(post_data, &mut count, elements.as_mut_ptr());
                            for el in elements {
                                if !el.is_null() {
                                    let elem = el.as_ref().unwrap();
                                    if let Some(get_bytes_count) = elem.get_bytes_count {
                                        let bcount = get_bytes_count(el);
                                        if bcount > 0 && elem.get_bytes.is_some() {
                                            let mut buf = vec![0u8; bcount];
                                            let read = (elem.get_bytes.unwrap())(
                                                el,
                                                bcount,
                                                buf.as_mut_ptr() as *mut c_void,
                                            );
                                            buf.truncate(read);
                                            post_bytes.extend_from_slice(&buf);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Parse JSON { cmd, args }
            let mut response_body = serde_json::json!({ "error": "Invalid request" });
            let mut status = 400;

            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&post_bytes) {
                if let Some(cmd) = val.get("cmd").and_then(|c| c.as_str()) {
                    let default_args = serde_json::json!({});
                    let args = val.get("args").unwrap_or(&default_args);

                    let guard = DISPATCHER.lock().unwrap();
                    if let Some(dispatcher) = guard.as_ref() {
                        match dispatcher(cmd, args) {
                            Ok(res) => {
                                response_body = res;
                                status = 200;
                            }
                            Err(err) => {
                                response_body = serde_json::json!({ "error": err });
                                status = 400;
                            }
                        }
                    } else {
                        response_body = serde_json::json!({ "error": "Dispatcher not registered" });
                        status = 500;
                    }
                }
            }

            let resp_bytes = serde_json::to_vec(&response_body).unwrap_or_default();
            return JorenResourceHandler::new(resp_bytes, "application/json".to_string(), status);
        }

        // Static bundled assets: joren://app/...
        let path_part = if let Some(idx) = url_str.find("joren://app") {
            let sub = &url_str[idx + "joren://app".len()..];
            let clean = sub.split('?').next().unwrap_or("/").split('#').next().unwrap_or("/");
            if clean.is_empty() || clean == "/" {
                "/index.html"
            } else {
                clean
            }
        } else {
            "/index.html"
        };

        // Locate bundled app folder
        let mut app_dir = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        app_dir.pop();
        let mut file_path = app_dir.join("app").join(path_part.trim_start_matches('/'));

        if !file_path.exists() {
            // Check dev path
            let dev_path = PathBuf::from("build").join(path_part.trim_start_matches('/'));
            if dev_path.exists() {
                file_path = dev_path;
            } else {
                // SPA fallback to index.html
                let index_path = app_dir.join("app").join("index.html");
                if index_path.exists() {
                    file_path = index_path;
                }
            }
        }

        let mime = if path_part.ends_with(".html") {
            "text/html; charset=utf-8"
        } else if path_part.ends_with(".js") {
            "application/javascript; charset=utf-8"
        } else if path_part.ends_with(".css") {
            "text/css; charset=utf-8"
        } else if path_part.ends_with(".png") {
            "image/png"
        } else if path_part.ends_with(".svg") {
            "image/svg+xml"
        } else if path_part.ends_with(".json") {
            "application/json"
        } else {
            "application/octet-stream"
        };

        match std::fs::read(&file_path) {
            Ok(bytes) => JorenResourceHandler::new(bytes, mime.to_string(), 200),
            Err(_) => JorenResourceHandler::new(b"Not Found".to_vec(), "text/plain".to_string(), 404),
        }
    }
}
