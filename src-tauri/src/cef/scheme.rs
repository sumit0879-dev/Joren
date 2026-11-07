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
                process_request: None,
                get_response_headers: Some(Self::get_response_headers),
                skip: None,
                read: Some(Self::read),
                read_response: None,
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
        crate::cef::log_startup(
            "HANDLER_OPEN",
            &format!(
                "open: bytes={}, mime={}, base.size={}",
                this.response_data.len(),
                this.response_mime,
                this.base.base.size
            ),
        );
        // Handle immediately. CEF requires that |callback| is executed only
        // when handle_request is set to false.
        if !handle_request.is_null() {
            *handle_request = 1;
        }
        let _ = callback;
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

                // Same-origin only. Do not grant wildcard access to the trusted app scheme.
                let h_origin_name = cef_string_from_str("Access-Control-Allow-Origin");
                let h_origin_val = cef_string_from_str("joren://app");
                set_header(response, &h_origin_name, &h_origin_val, 1);
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
        if this.read_offset == 0 || this.read_offset >= this.response_data.len() {
            crate::cef::log_startup(
                "HANDLER_READ",
                &format!(
                    "read: to_read={}, offset={}/{}",
                    bytes_to_read, this.read_offset, this.response_data.len()
                ),
            );
        }
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

        let parsed = match parse_joren_url(&url_str) {
            Some(parsed) => parsed,
            None => {
                crate::cef::log_startup("SECURITY", &format!("Rejecting non-app URL: {}", url_str));
                return std::ptr::null_mut();
            }
        };

        if method.eq_ignore_ascii_case("OPTIONS") {
            return JorenResourceHandler::new(Vec::new(), "text/plain".to_string(), 204);
        }

        if parsed.host != "app" {
            crate::cef::log_startup("SECURITY", &format!("Rejecting untrusted joren host: {}", parsed.host));
            return JorenResourceHandler::new(b"Forbidden".to_vec(), "text/plain".to_string(), 403);
        }

        if parsed.path == "/api/invoke" {
            if !method.eq_ignore_ascii_case("POST") {
                crate::cef::log_startup("SECURITY", &format!("Rejecting {} /api/invoke", method));
                return JorenResourceHandler::new(b"Method Not Allowed".to_vec(), "text/plain".to_string(), 405);
            }
            if let Some(get_header) = req.get_header_by_name {
                let origin_name = cef_string_from_str("Origin");
                let origin_ptr = get_header(request, &origin_name);
                let origin = cef_string_userfree_to_string_and_free(origin_ptr);
                if !origin.is_empty() && origin != "joren://app" && origin != "null" {
                    crate::cef::log_startup("SECURITY", &format!("Rejecting IPC from origin {}", origin));
                    return JorenResourceHandler::new(b"Forbidden".to_vec(), "text/plain".to_string(), 403);
                }
            }
            let mut post_bytes = Vec::new();
            if let Some(get_post_data) = req.get_post_data {
                let post_data = get_post_data(request);
                if !post_data.is_null() {
                    let pd = post_data.as_ref().unwrap();
                    let count = if let Some(get_count) = pd.get_element_count {
                        get_count(post_data)
                    } else {
                        0
                    };
                    if count > 0 && pd.get_elements.is_some() {
                        let mut elements = vec![std::ptr::null_mut(); count];
                        let mut actual_count = count;
                        (pd.get_elements.unwrap())(post_data, &mut actual_count, elements.as_mut_ptr());
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
                    crate::cef::log_startup("IPC_POST", &format!("Read {} POST bytes from {} elements", post_bytes.len(), count));
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
            } else {
                crate::cef::log_startup("IPC_ERR", &format!("Failed to parse JSON from {} post bytes: {}", post_bytes.len(), String::from_utf8_lossy(&post_bytes)));
            }

            let resp_bytes = serde_json::to_vec(&response_body).unwrap_or_default();
            return JorenResourceHandler::new(resp_bytes, "application/json".to_string(), status);
        }

        let sub_path = match safe_relative_path(&parsed.path) {
            Ok(path) => path,
            Err(_) => {
                crate::cef::log_startup("SECURITY", &format!("Blocked path traversal attempt: {}", url_str));
                return JorenResourceHandler::new(b"Forbidden".to_vec(), "text/plain".to_string(), 403);
            }
        };

        // Determine if request expects a static file by checking if last segment has an extension
        let last_segment = sub_path
            .rsplit('/')
            .next()
            .unwrap_or(sub_path.as_str())
            .to_string();
        let has_extension = last_segment.contains('.') && !last_segment.starts_with('.');

        // Locate bundled app folder
        let mut app_dir = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        app_dir.pop();
        let mut file_path = app_dir.join("app").join(&sub_path);

        if !file_path.exists() {
            // Check if flattened in app/last_segment (in case assets directory was flattened during copy)
            let flattened_in_app = app_dir.join("app").join(&last_segment);
            if flattened_in_app.exists() {
                file_path = flattened_in_app;
            } else {
                // Check dev path
                let dev_path = PathBuf::from("build").join(&sub_path);
                if dev_path.exists() {
                    file_path = dev_path;
                } else {
                    let dev_flattened = PathBuf::from("build").join(&last_segment);
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

        let mime = mime_for_path(&file_path.to_string_lossy());

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JorenUrl {
    pub host: String,
    pub path: String,
}

pub fn parse_joren_url(url: &str) -> Option<JorenUrl> {
    let rest = url.strip_prefix("joren://")?;
    let rest = rest.split('#').next().unwrap_or(rest);
    let rest = rest.split('?').next().unwrap_or(rest);
    if rest.is_empty() {
        return None;
    }
    let (host, path) = if let Some((host, path)) = rest.split_once('/') {
        (host, format!("/{path}"))
    } else {
        (rest, "/".to_string())
    };
    if host.is_empty() || host.contains(':') || host.contains('@') {
        return None;
    }
    Some(JorenUrl {
        host: host.to_string(),
        path,
    })
}

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
                if let Some(c) = h1 {
                    bytes.push(c);
                }
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn safe_relative_path(path: &str) -> Result<String, &'static str> {
    let decoded = percent_decode(path);
    let normalized = decoded.replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    if trimmed.contains("..") || trimmed.contains(':') {
        return Err("rejected path");
    }
    if trimmed.is_empty() {
        Ok("index.html".to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

pub fn mime_for_path(path: &str) -> &'static str {
    let path_lower = path.to_lowercase();
    if path_lower.ends_with(".html") || path_lower.ends_with(".htm") {
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
    } else if path_lower.ends_with(".avif") {
        "image/avif"
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_app_url_and_strips_query() {
        let parsed = parse_joren_url("joren://app/assets/index-abc.js?v=1#x").unwrap();
        assert_eq!(parsed.host, "app");
        assert_eq!(parsed.path, "/assets/index-abc.js");
    }

    #[test]
    fn rejects_traversal_and_foreign_scheme() {
        assert!(safe_relative_path("/../windows/system32").is_err());
        assert!(safe_relative_path("/C:/secret").is_err());
        assert!(parse_joren_url("https://example.com/").is_none());
        assert_eq!(safe_relative_path("/").unwrap(), "index.html");
        assert_eq!(safe_relative_path("/chat").unwrap(), "chat");
    }

    #[test]
    fn mime_matches_production_assets() {
        assert_eq!(mime_for_path("app/index.html"), "text/html");
        assert_eq!(mime_for_path("app/assets/index-abc.js"), "text/javascript");
        assert_eq!(mime_for_path("app/assets/index-abc.css"), "text/css");
        assert_eq!(mime_for_path("app/assets/icon.png"), "image/png");
        assert_eq!(mime_for_path("app/assets/font.woff2"), "font/woff2");
    }
}
