use std::ffi::c_void;
use std::os::raw::c_int;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct _cef_string_utf16_t {
    pub str_: *mut u16,
    pub length: usize,
    pub dtor: Option<unsafe extern "C" fn(*mut u16)>,
}

impl Default for _cef_string_utf16_t {
    fn default() -> Self {
        Self {
            str_: std::ptr::null_mut(),
            length: 0,
            dtor: None,
        }
    }
}

pub type cef_string_t = _cef_string_utf16_t;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct _cef_rect_t {
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
}

#[repr(C)]
pub struct _cef_base_ref_counted_t {
    pub size: usize,
    pub add_ref: Option<unsafe extern "C" fn(*mut _cef_base_ref_counted_t)>,
    pub release: Option<unsafe extern "C" fn(*mut _cef_base_ref_counted_t) -> c_int>,
    pub has_one_ref: Option<unsafe extern "C" fn(*mut _cef_base_ref_counted_t) -> c_int>,
    pub has_at_least_one_ref: Option<unsafe extern "C" fn(*mut _cef_base_ref_counted_t) -> c_int>,
}

#[repr(C)]
pub struct _cef_main_args_t {
    pub instance: isize, // HINSTANCE
}

#[repr(C)]
pub struct _cef_window_info_t {
    pub ex_style: u32,
    pub window_name: _cef_string_utf16_t,
    pub style: u32,
    pub bounds: _cef_rect_t,
    pub parent_window: isize, // HWND
    pub menu: isize,          // HMENU
    pub windowless_rendering_enabled: c_int,
    pub shared_texture_enabled: c_int,
    pub external_begin_frame_enabled: c_int,
    pub window: isize,        // HWND
}

#[repr(C)]
pub struct _cef_settings_t {
    pub size: usize,
    pub no_sandbox: c_int,
    pub browser_subprocess_path: _cef_string_utf16_t,
    pub framework_dir_path: _cef_string_utf16_t,
    pub main_bundle_path: _cef_string_utf16_t,
    pub chrome_runtime: c_int,
    pub multi_threaded_message_loop: c_int,
    pub external_message_pump: c_int,
    pub windowless_rendering_enabled: c_int,
    pub command_line_args_disabled: c_int,
    pub cache_path: _cef_string_utf16_t,
    pub root_cache_path: _cef_string_utf16_t,
    pub user_data_path: _cef_string_utf16_t,
    pub persist_session_cookies: c_int,
    pub user_agent: _cef_string_utf16_t,
    pub user_agent_product: _cef_string_utf16_t,
    pub locale: _cef_string_utf16_t,
    pub log_file: _cef_string_utf16_t,
    pub log_severity: c_int,
    pub javascript_flags: _cef_string_utf16_t,
    pub resources_dir_path: _cef_string_utf16_t,
    pub locales_dir_path: _cef_string_utf16_t,
    pub pack_loading_disabled: c_int,
    pub remote_debugging_port: c_int,
    pub uncaught_exception_stack_size: c_int,
    pub background_color: u32,
    pub accept_language_list: _cef_string_utf16_t,
    pub cookieable_schemes_list: _cef_string_utf16_t,
    pub cookieable_schemes_exclude_defaults: c_int,
    pub application_client_id_for_file_scanning: _cef_string_utf16_t,
}

#[repr(C)]
pub struct _cef_browser_settings_t {
    pub size: usize,
    pub windowless_frame_rate: c_int,
    pub standard_font_family: _cef_string_utf16_t,
    pub fixed_font_family: _cef_string_utf16_t,
    pub serif_font_family: _cef_string_utf16_t,
    pub sans_serif_font_family: _cef_string_utf16_t,
    pub cursive_font_family: _cef_string_utf16_t,
    pub fantasy_font_family: _cef_string_utf16_t,
    pub default_font_size: c_int,
    pub default_fixed_font_size: c_int,
    pub minimum_font_size: c_int,
    pub minimum_logical_font_size: c_int,
    pub default_encoding: _cef_string_utf16_t,
    pub remote_fonts: c_int,
    pub javascript: c_int,
    pub javascript_close_windows: c_int,
    pub javascript_access_clipboard: c_int,
    pub javascript_dom_paste: c_int,
    pub image_loading: c_int,
    pub image_shrink_standalone_to_fit: c_int,
    pub text_area_resize: c_int,
    pub tab_to_links: c_int,
    pub local_storage: c_int,
    pub databases: c_int,
    pub webgl: c_int,
    pub background_color: u32,
}

#[repr(C)]
pub struct _cef_app_t {
    pub base: _cef_base_ref_counted_t,
    pub on_before_command_line_processing: Option<
        unsafe extern "C" fn(
            *mut _cef_app_t,
            *const _cef_string_utf16_t,
            *mut c_void, // _cef_command_line_t
        ),
    >,
    pub on_register_custom_schemes: Option<
        unsafe extern "C" fn(
            *mut _cef_app_t,
            *mut _cef_scheme_registrar_t,
        ),
    >,
    pub get_resource_bundle_handler: Option<
        unsafe extern "C" fn(*mut _cef_app_t) -> *mut c_void,
    >,
    pub get_browser_process_handler: Option<
        unsafe extern "C" fn(*mut _cef_app_t) -> *mut c_void,
    >,
    pub get_render_process_handler: Option<
        unsafe extern "C" fn(*mut _cef_app_t) -> *mut c_void,
    >,
}

#[repr(C)]
pub struct _cef_scheme_registrar_t {
    pub base: _cef_base_ref_counted_t,
    pub add_custom_scheme: Option<
        unsafe extern "C" fn(
            *mut _cef_scheme_registrar_t,
            *const _cef_string_utf16_t,
            c_int,
        ) -> c_int,
    >,
}

pub const CEF_SCHEME_OPTION_STANDARD: c_int = 1 << 0;
pub const CEF_SCHEME_OPTION_LOCAL: c_int = 1 << 1;
pub const CEF_SCHEME_OPTION_DISPLAY_ISOLATED: c_int = 1 << 2;
pub const CEF_SCHEME_OPTION_SECURE: c_int = 1 << 3;
pub const CEF_SCHEME_OPTION_CORS_ENABLED: c_int = 1 << 4;
pub const CEF_SCHEME_OPTION_CSP_BYPASSING: c_int = 1 << 5;
pub const CEF_SCHEME_OPTION_FETCH_ENABLED: c_int = 1 << 6;

#[repr(C)]
pub struct _cef_client_t {
    pub base: _cef_base_ref_counted_t,
    pub get_audio_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_command_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_context_menu_handler: Option<
        unsafe extern "C" fn(*mut _cef_client_t) -> *mut _cef_context_menu_handler_t,
    >,
    pub get_dialog_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_display_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_download_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_drag_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_find_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_focus_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_jsdialog_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_keyboard_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_life_span_handler: Option<
        unsafe extern "C" fn(*mut _cef_client_t) -> *mut _cef_life_span_handler_t,
    >,
    pub get_load_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_render_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub get_request_handler: Option<unsafe extern "C" fn(*mut _cef_client_t) -> *mut c_void>,
    pub on_process_message_received: Option<
        unsafe extern "C" fn(
            *mut _cef_client_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            c_int,
            *mut c_void,
        ) -> c_int,
    >,
}

#[repr(C)]
pub struct _cef_life_span_handler_t {
    pub base: _cef_base_ref_counted_t,
    pub on_before_popup: Option<
        unsafe extern "C" fn(
            *mut _cef_life_span_handler_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            *const _cef_string_utf16_t,
            *const _cef_string_utf16_t,
            c_int,
            c_int,
            *const _cef_window_info_t,
            *mut *mut _cef_client_t,
            *mut _cef_browser_settings_t,
            *mut c_void,
            *mut c_int,
        ) -> c_int,
    >,
    pub on_after_created: Option<
        unsafe extern "C" fn(*mut _cef_life_span_handler_t, *mut _cef_browser_t),
    >,
    pub do_close: Option<
        unsafe extern "C" fn(*mut _cef_life_span_handler_t, *mut _cef_browser_t) -> c_int,
    >,
    pub on_before_close: Option<
        unsafe extern "C" fn(*mut _cef_life_span_handler_t, *mut _cef_browser_t),
    >,
}

#[repr(C)]
pub struct _cef_context_menu_handler_t {
    pub base: _cef_base_ref_counted_t,
    pub on_before_context_menu: Option<
        unsafe extern "C" fn(
            *mut _cef_context_menu_handler_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            *mut c_void, // _cef_context_menu_params_t
            *mut c_void, // _cef_menu_model_t
        ),
    >,
    pub run_context_menu: Option<
        unsafe extern "C" fn(
            *mut _cef_context_menu_handler_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> c_int,
    >,
    pub on_context_menu_command: Option<
        unsafe extern "C" fn(
            *mut _cef_context_menu_handler_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            *mut c_void,
            c_int,
            u32,
        ) -> c_int,
    >,
    pub on_context_menu_dismissed: Option<
        unsafe extern "C" fn(
            *mut _cef_context_menu_handler_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
        ),
    >,
}

#[repr(C)]
pub struct _cef_scheme_handler_factory_t {
    pub base: _cef_base_ref_counted_t,
    pub create: Option<
        unsafe extern "C" fn(
            *mut _cef_scheme_handler_factory_t,
            *mut _cef_browser_t,
            *mut _cef_frame_t,
            *const _cef_string_utf16_t,
            *mut _cef_request_t,
        ) -> *mut _cef_resource_handler_t,
    >,
}

#[repr(C)]
pub struct _cef_resource_handler_t {
    pub base: _cef_base_ref_counted_t,
    pub open: Option<
        unsafe extern "C" fn(
            *mut _cef_resource_handler_t,
            *mut _cef_request_t,
            *mut c_int,
            *mut _cef_callback_t,
        ) -> c_int,
    >,
    pub get_response_headers: Option<
        unsafe extern "C" fn(
            *mut _cef_resource_handler_t,
            *mut _cef_response_t,
            *mut i64,
            *mut _cef_string_utf16_t,
        ),
    >,
    pub read: Option<
        unsafe extern "C" fn(
            *mut _cef_resource_handler_t,
            *mut c_void,
            c_int,
            *mut c_int,
            *mut _cef_resource_read_callback_t,
        ) -> c_int,
    >,
    pub cancel: Option<unsafe extern "C" fn(*mut _cef_resource_handler_t)>,
}

#[repr(C)]
pub struct _cef_callback_t {
    pub base: _cef_base_ref_counted_t,
    pub cont: Option<unsafe extern "C" fn(*mut _cef_callback_t)>,
    pub cancel: Option<unsafe extern "C" fn(*mut _cef_callback_t)>,
}

#[repr(C)]
pub struct _cef_resource_read_callback_t {
    pub base: _cef_base_ref_counted_t,
    pub cont: Option<unsafe extern "C" fn(*mut _cef_resource_read_callback_t, c_int)>,
}

#[repr(C)]
pub struct _cef_browser_t {
    pub base: _cef_base_ref_counted_t,
    pub get_host: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> *mut _cef_browser_host_t>,
    pub can_go_back: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub go_back: Option<unsafe extern "C" fn(*mut _cef_browser_t)>,
    pub can_go_forward: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub go_forward: Option<unsafe extern "C" fn(*mut _cef_browser_t)>,
    pub is_loading: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub reload: Option<unsafe extern "C" fn(*mut _cef_browser_t)>,
    pub reload_ignore_cache: Option<unsafe extern "C" fn(*mut _cef_browser_t)>,
    pub stop_load: Option<unsafe extern "C" fn(*mut _cef_browser_t)>,
    pub get_identifier: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub is_same: Option<unsafe extern "C" fn(*mut _cef_browser_t, *mut _cef_browser_t) -> c_int>,
    pub is_popup: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub has_document: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> c_int>,
    pub get_main_frame: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> *mut _cef_frame_t>,
    pub get_focused_frame: Option<unsafe extern "C" fn(*mut _cef_browser_t) -> *mut _cef_frame_t>,
}

#[repr(C)]
pub struct _cef_browser_host_t {
    pub base: _cef_base_ref_counted_t,
    pub get_browser: Option<unsafe extern "C" fn(*mut _cef_browser_host_t) -> *mut _cef_browser_t>,
    pub close_browser: Option<unsafe extern "C" fn(*mut _cef_browser_host_t, c_int)>,
    pub try_close_browser: Option<unsafe extern "C" fn(*mut _cef_browser_host_t) -> c_int>,
    pub set_focus: Option<unsafe extern "C" fn(*mut _cef_browser_host_t, c_int)>,
    pub get_window_handle: Option<unsafe extern "C" fn(*mut _cef_browser_host_t) -> isize>,
}

#[repr(C)]
pub struct _cef_frame_t {
    pub base: _cef_base_ref_counted_t,
    pub is_valid: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> c_int>,
    pub undo: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub redo: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub cut: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub copy: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub paste: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub delete: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub select_all: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub view_source: Option<unsafe extern "C" fn(*mut _cef_frame_t)>,
    pub get_source: Option<unsafe extern "C" fn(*mut _cef_frame_t, *mut c_void)>,
    pub get_text: Option<unsafe extern "C" fn(*mut _cef_frame_t, *mut c_void)>,
    pub load_request: Option<unsafe extern "C" fn(*mut _cef_frame_t, *mut _cef_request_t)>,
    pub load_url: Option<unsafe extern "C" fn(*mut _cef_frame_t, *const _cef_string_utf16_t)>,
    pub execute_java_script: Option<
        unsafe extern "C" fn(
            *mut _cef_frame_t,
            *const _cef_string_utf16_t,
            *const _cef_string_utf16_t,
            c_int,
        ),
    >,
    pub is_main: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> c_int>,
    pub is_focused: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> c_int>,
    pub get_name: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> _cef_string_utf16_t>,
    pub get_identifier: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> i64>,
    pub get_parent: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> *mut _cef_frame_t>,
    pub get_url: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> _cef_string_utf16_t>,
    pub get_browser: Option<unsafe extern "C" fn(*mut _cef_frame_t) -> *mut _cef_browser_t>,
}

#[repr(C)]
pub struct _cef_request_t {
    pub base: _cef_base_ref_counted_t,
    pub is_read_only: Option<unsafe extern "C" fn(*mut _cef_request_t) -> c_int>,
    pub get_url: Option<unsafe extern "C" fn(*mut _cef_request_t) -> _cef_string_utf16_t>,
    pub set_url: Option<unsafe extern "C" fn(*mut _cef_request_t, *const _cef_string_utf16_t)>,
    pub get_method: Option<unsafe extern "C" fn(*mut _cef_request_t) -> _cef_string_utf16_t>,
    pub set_method: Option<unsafe extern "C" fn(*mut _cef_request_t, *const _cef_string_utf16_t)>,
    pub set_referrer: Option<
        unsafe extern "C" fn(*mut _cef_request_t, *const _cef_string_utf16_t, c_int),
    >,
    pub get_referrer_url: Option<unsafe extern "C" fn(*mut _cef_request_t) -> _cef_string_utf16_t>,
    pub get_referrer_policy: Option<unsafe extern "C" fn(*mut _cef_request_t) -> c_int>,
    pub get_post_data: Option<unsafe extern "C" fn(*mut _cef_request_t) -> *mut _cef_post_data_t>,
    pub set_post_data: Option<unsafe extern "C" fn(*mut _cef_request_t, *mut _cef_post_data_t)>,
    pub get_header_map: Option<unsafe extern "C" fn(*mut _cef_request_t, *mut c_void)>,
    pub set_header_map: Option<unsafe extern "C" fn(*mut _cef_request_t, *mut c_void)>,
}

#[repr(C)]
pub struct _cef_response_t {
    pub base: _cef_base_ref_counted_t,
    pub is_read_only: Option<unsafe extern "C" fn(*mut _cef_response_t) -> c_int>,
    pub get_error: Option<unsafe extern "C" fn(*mut _cef_response_t) -> c_int>,
    pub set_error: Option<unsafe extern "C" fn(*mut _cef_response_t, c_int)>,
    pub get_status: Option<unsafe extern "C" fn(*mut _cef_response_t) -> c_int>,
    pub set_status: Option<unsafe extern "C" fn(*mut _cef_response_t, c_int)>,
    pub get_status_text: Option<unsafe extern "C" fn(*mut _cef_response_t) -> _cef_string_utf16_t>,
    pub set_status_text:
        Option<unsafe extern "C" fn(*mut _cef_response_t, *const _cef_string_utf16_t)>,
    pub get_mime_type: Option<unsafe extern "C" fn(*mut _cef_response_t) -> _cef_string_utf16_t>,
    pub set_mime_type:
        Option<unsafe extern "C" fn(*mut _cef_response_t, *const _cef_string_utf16_t)>,
    pub get_charset: Option<unsafe extern "C" fn(*mut _cef_response_t) -> _cef_string_utf16_t>,
    pub set_charset: Option<unsafe extern "C" fn(*mut _cef_response_t, *const _cef_string_utf16_t)>,
    pub get_header_by_name: Option<
        unsafe extern "C" fn(
            *mut _cef_response_t,
            *const _cef_string_utf16_t,
        ) -> _cef_string_utf16_t,
    >,
    pub set_header_by_name: Option<
        unsafe extern "C" fn(
            *mut _cef_response_t,
            *const _cef_string_utf16_t,
            *const _cef_string_utf16_t,
            c_int,
        ),
    >,
}

#[repr(C)]
pub struct _cef_post_data_t {
    pub base: _cef_base_ref_counted_t,
    pub is_read_only: Option<unsafe extern "C" fn(*mut _cef_post_data_t) -> c_int>,
    pub has_excluded_elements: Option<unsafe extern "C" fn(*mut _cef_post_data_t) -> c_int>,
    pub get_element_count: Option<unsafe extern "C" fn(*mut _cef_post_data_t) -> usize>,
    pub get_elements: Option<
        unsafe extern "C" fn(
            *mut _cef_post_data_t,
            *mut usize,
            *mut *mut _cef_post_data_element_t,
        ),
    >,
    pub remove_element:
        Option<unsafe extern "C" fn(*mut _cef_post_data_t, *mut _cef_post_data_element_t) -> c_int>,
    pub add_element:
        Option<unsafe extern "C" fn(*mut _cef_post_data_t, *mut _cef_post_data_element_t) -> c_int>,
    pub remove_elements: Option<unsafe extern "C" fn(*mut _cef_post_data_t)>,
}

#[repr(C)]
pub struct _cef_post_data_element_t {
    pub base: _cef_base_ref_counted_t,
    pub is_read_only: Option<unsafe extern "C" fn(*mut _cef_post_data_element_t) -> c_int>,
    pub set_to_empty: Option<unsafe extern "C" fn(*mut _cef_post_data_element_t)>,
    pub set_to_file:
        Option<unsafe extern "C" fn(*mut _cef_post_data_element_t, *const _cef_string_utf16_t)>,
    pub set_to_bytes:
        Option<unsafe extern "C" fn(*mut _cef_post_data_element_t, usize, *const c_void)>,
    pub get_type: Option<unsafe extern "C" fn(*mut _cef_post_data_element_t) -> c_int>,
    pub get_file:
        Option<unsafe extern "C" fn(*mut _cef_post_data_element_t) -> _cef_string_utf16_t>,
    pub get_bytes_count: Option<unsafe extern "C" fn(*mut _cef_post_data_element_t) -> usize>,
    pub get_bytes: Option<
        unsafe extern "C" fn(*mut _cef_post_data_element_t, usize, *mut c_void) -> usize,
    >,
}
