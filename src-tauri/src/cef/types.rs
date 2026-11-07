//! Version-matched CEF 156.0.15 FFI Bindings
//! Pinned to CEF 156.0.15+gcf671c0+chromium-156.0.8078.80

pub use cef_dll_sys::*;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct _cef_version_info_t {
    pub size: usize,
    pub cef_version_major: std::os::raw::c_int,
    pub cef_version_minor: std::os::raw::c_int,
    pub cef_version_patch: std::os::raw::c_int,
    pub cef_commit_number: std::os::raw::c_int,
    pub chrome_version_major: std::os::raw::c_int,
    pub chrome_version_minor: std::os::raw::c_int,
    pub chrome_version_build: std::os::raw::c_int,
    pub chrome_version_patch: std::os::raw::c_int,
}
pub type cef_version_info_t = _cef_version_info_t;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cef_156_struct_sizes() {
        assert_eq!(std::mem::size_of::<_cef_window_info_t>(), 112);
        assert_eq!(std::mem::size_of::<_cef_settings_t>(), 496);
        assert_eq!(std::mem::size_of::<_cef_browser_settings_t>(), 264);
        assert_eq!(std::mem::size_of::<_cef_client_t>(), 192);
        assert_eq!(std::mem::size_of::<_cef_life_span_handler_t>(), 88);
        assert_eq!(std::mem::size_of::<_cef_load_handler_t>(), 72);
        assert_eq!(std::mem::size_of::<_cef_scheme_handler_factory_t>(), 48);
        assert_eq!(std::mem::size_of::<_cef_resource_handler_t>(), 96);
        assert_eq!(CEF_API_VERSION_LAST, 15600);
    }
}
