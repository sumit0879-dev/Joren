use super::api::{cef_string_from_str, cef_string_userfree_to_string_and_free};
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

#[repr(C)]
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
                process_request: Some(Self::process_request),
                get_response_headers: Some(Self::get_response_headers),
                skip: None,
                read: Some(Self::read),
                read_response: Some(Self::read_response),
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
        self_: *mut _cef_resource_handler_t,
        _request: *mut _cef_request_t,
        handle_request: *mut c_int,
        callback: *mut _cef_callback_t,
    ) -> c_int {
        let this = (self_ as *mut Self).as_mut().unwrap();
        crate::cef::log_startup("HANDLER_OPEN", &format!("open: size={}, mime={}", this.response_data.len(), this.response_mime));
        if !handle_request.is_null() {
            *handle_request = 1;
        }
        if !callback.is_null() {
            let cb = &*callback;
            if let Some(cont) = cb.cont {
                cont(callback);
            }
        }
        1
    }

    unsafe extern "C" fn process_request(
        self_: *mut _cef_resource_handler_t,
        _request: *mut _cef_request_t,
        callback: *mut _cef_callback_t,
    ) -> c_int {
        let this = (self_ as *mut Self).as_mut().unwrap();
        crate::cef::log_startup("HANDLER_PROC", &format!("process_request: size={}, mime={}", this.response_data.len(), this.response_mime));
        if !callback.is_null() {
            let cb = &*callback;
            if let Some(cont) = cb.cont {
                cont(callback);
            }
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
        crate::cef::log_startup("HANDLER_HDR", &format!("get_response_headers: status={}, mime={}, len={}", this.response_status, this.response_mime, this.response_data.len()));
        if let Some(resp) = response.as_mut() {
            if let Some(set_status) = resp.set_status {
                set_status(response, this.response_status);
            }
            if let Some(set_mime) = resp.set_mime_type {
                let pure_mime = this.response_mime.split(';').next().unwrap_or(&this.response_mime).trim();
                let mime_cef = cef_string_from_str(pure_mime);
                set_mime(response, &mime_cef);
            }
            if let Some(set_header) = resp.set_header_by_name {
                let content_type = if this.response_mime.contains("charset") {
                    this.response_mime.clone()
                } else if this.response_mime.starts_with("text/") || this.response_mime == "application/json" {
                    format!("{}; charset=utf-8", this.response_mime)
                } else {
                    this.response_mime.clone()
                };
                let h_ct_name = cef_string_from_str("Content-Type");
                let h_ct_val = cef_string_from_str(&content_type);
                set_header(response, &h_ct_name, &h_ct_val, 1);

                let h_origin_name = cef_string_from_str("Access-Control-Allow-Origin");
                let h_origin_val = cef_string_from_str("*");
                set_header(response, &h_origin_name, &h_origin_val, 1);

                let h_methods_name = cef_string_from_str("Access-Control-Allow-Methods");
                let h_methods_val = cef_string_from_str("GET, POST, OPTIONS, HEAD");
                set_header(response, &h_methods_name, &h_methods_val, 1);

                let h_headers_name = cef_string_from_str("Access-Control-Allow-Headers");
                let h_headers_val = cef_string_from_str("Content-Type, Authorization, *");
                set_header(response, &h_headers_name, &h_headers_val, 1);
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
        crate::cef::log_startup("HANDLER_READ", &format!("read: to_read={}, offset={}/{}", bytes_to_read, this.read_offset, this.response_data.len()));
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

    unsafe extern "C" fn read_response(
        self_: *mut _cef_resource_handler_t,
        data_out: *mut c_void,
        bytes_to_read: c_int,
        bytes_read: *mut c_int,
        _callback: *mut _cef_callback_t,
    ) -> c_int {
        let this = (self_ as *mut Self).as_mut().unwrap();
        crate::cef::log_startup("HANDLER_READ_RESP", &format!("read_response: to_read={}, offset={}/{}", bytes_to_read, this.read_offset, this.response_data.len()));
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

    unsafe extern "C" fn cancel(self_: *mut _cef_resource_handler_t) {
        crate::cef::log_startup("HANDLER_CANCEL", "cancel called");
    }
}

#[repr(C)]
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
        let url_str = if let Some(get_url) = req.get_url {
            let url_userfree = get_url(request);
            cef_string_userfree_to_string_and_free(url_userfree)
        } else {
            return std::ptr::null_mut();
        };

        let method = if let Some(get_method) = req.get_method {
            let m_ptr = get_method(request);
            cef_string_userfree_to_string_and_free(m_ptr)
        } else {
            "GET".to_string()
        };

        crate::cef::log_startup("SCHEME_REQ", &format!("{} {}", method, url_str));

        if method.eq_ignore_ascii_case("OPTIONS") {
            return JorenResourceHandler::new(Vec::new(), "text/plain".to_string(), 200);
        }

        // Strictly enforce trusted app scheme: joren://
        if !url_str.starts_with("joren://") {
            crate::cef::log_startup("SECURITY", &format!("Rejecting request with non-joren scheme: {}", url_str));
            return std::ptr::null_mut();
        }

        // Check if IPC endpoint: /api/invoke
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
            let mut response_body = serde_json::json!({ "error": "Invalid JSON request payload" });
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
                        response_body = serde_json::json!({ "error": "IPC dispatcher not registered" });
                        status = 500;
                    }
                } else {
                    response_body = serde_json::json!({ "error": "Missing 'cmd' parameter in request" });
                }
            }

            let resp_bytes = serde_json::to_vec(&response_body).unwrap_or_default();
            return JorenResourceHandler::new(resp_bytes, "application/json".to_string(), status);
        }

        // Static bundled assets: joren://app/... or joren:///...
        let path_and_query = if url_str.starts_with("joren://app") {
            &url_str["joren://app".len()..]
        } else if url_str.starts_with("joren://") {
            &url_str["joren://".len()..]
        } else {
            "/index.html"
        };

        let clean_path = path_and_query.split('?').next().unwrap_or("/").split('#').next().unwrap_or("/");

        // Simple percent decoding
        fn percent_decode(input: &str) -> String {
            let mut bytes = Vec::new();
            let mut iter = input.bytes();
            while let Some(b) = iter.next() {
                if b == b'%' {
                    let h1 = iter.next();
                    let h2 = iter.next();
                    if let (Some(c1), Some(c2)) = (h1, h2) {
                        let hex = [c1, c2];
                        if let Ok(val) = u8::from_str_radix(std::str::from_utf8(&hex).unwrap_or(""), 16) {
                            bytes.push(val);
                            continue;
                        }
                        bytes.push(b'%');
                        bytes.push(c1);
                        bytes.push(c2);
                    } else {
                        bytes.push(b'%');
                        if let Some(c) = h1 { bytes.push(c); }
                    }
                } else {
                    bytes.push(b);
                }
            }
            String::from_utf8_lossy(&bytes).to_string()
        }

        let decoded = percent_decode(clean_path);
        let normalized = decoded.replace('\\', "/");
        let trimmed = normalized.trim_start_matches('/');

        // Reject path traversal
        if trimmed.contains("..") || trimmed.contains(':') {
            crate::cef::log_startup("SECURITY", &format!("Blocked path traversal attempt: {}", url_str));
            return JorenResourceHandler::new(b"Forbidden".to_vec(), "text/plain".to_string(), 403);
        }

        let sub_path = if trimmed.is_empty() {
            "index.html"
        } else {
            trimmed
        };

        // Determine if request expects a static file by checking if last segment has an extension
        let last_segment = sub_path.rsplit('/').next().unwrap_or(sub_path);
        let has_extension = last_segment.contains('.') && !last_segment.starts_with('.');

        // Locate bundled app folder
        let mut app_dir = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        app_dir.pop();
        let mut file_path = app_dir.join("app").join(sub_path);

        if !file_path.exists() {
            // Check if flattened in app/last_segment (in case assets directory was flattened during copy)
            let flattened_in_app = app_dir.join("app").join(last_segment);
            if flattened_in_app.exists() {
                file_path = flattened_in_app;
            } else {
                // Check dev path
                let dev_path = PathBuf::from("build").join(sub_path);
                if dev_path.exists() {
                    file_path = dev_path;
                } else {
                    let dev_flattened = PathBuf::from("build").join(last_segment);
                    if dev_flattened.exists() {
                        file_path = dev_flattened;
                    } else if !has_extension {
                        // SPA client-side route fallback (only for extensionless route paths, e.g. /chat)
                        let index_path = app_dir.join("app").join("index.html");
                        if index_path.exists() {
                            file_path = index_path;
                        } else {
                            let dev_index = PathBuf::from("build").join("index.html");
                            if dev_index.exists() {
                                file_path = dev_index;
                            }
                        }
                    }
                }
            }
        }

        let path_lower = file_path.to_string_lossy().to_lowercase();
        let mime = if path_lower.ends_with(".html") || path_lower.ends_with(".htm") {
            "text/html"
        } else if path_lower.ends_with(".js") || path_lower.ends_with(".mjs") {
            "text/javascript"
        } else if path_lower.ends_with(".css") {
            "text/css"
        } else if path_lower.ends_with(".json") {
            "application/json"
        } else if path_lower.ends_with(".svg") {
            "image/svg+xml"
        } else if path_lower.ends_with(".png") {
            "image/png"
        } else if path_lower.ends_with(".jpg") || path_lower.ends_with(".jpeg") {
            "image/jpeg"
        } else if path_lower.ends_with(".webp") {
            "image/webp"
        } else if path_lower.ends_with(".gif") {
            "image/gif"
        } else if path_lower.ends_with(".ico") {
            "image/x-icon"
        } else if path_lower.ends_with(".woff2") {
            "font/woff2"
        } else if path_lower.ends_with(".woff") {
            "font/woff"
        } else if path_lower.ends_with(".ttf") {
            "font/ttf"
        } else if path_lower.ends_with(".wasm") {
            "application/wasm"
        } else {
            "application/octet-stream"
        };

        match std::fs::read(&file_path) {
            Ok(bytes) => {
                crate::cef::log_startup("RESOURCE_200", &format!("200 OK: /{} ({} bytes, mime={})", sub_path, bytes.len(), mime));
                JorenResourceHandler::new(bytes, mime.to_string(), 200)
            }
            Err(e) => {
                crate::cef::log_startup("RESOURCE_404", &format!("404 Not Found: /{} (err={})", sub_path, e));
                JorenResourceHandler::new(b"404 Not Found".to_vec(), "text/plain".to_string(), 404)
            }
        }
    }
}
