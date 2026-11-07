use super::api::cef_string_from_str;
use super::types::*;
use std::os::raw::c_int;

#[repr(C)]
pub struct JorenCefApp {
    pub base: _cef_app_t,
    ref_count: std::sync::atomic::AtomicI32,
}

impl JorenCefApp {
    pub fn new() -> *mut _cef_app_t {
        let mut base: _cef_app_t = unsafe { std::mem::zeroed() };
        base.base.size = std::mem::size_of::<_cef_app_t>();
        base.base.add_ref = Some(Self::add_ref);
        base.base.release = Some(Self::release);
        base.base.has_one_ref = Some(Self::has_one_ref);
        base.base.has_at_least_one_ref = Some(Self::has_at_least_one_ref);
        base.on_before_command_line_processing = Some(Self::on_before_command_line_processing);
        base.on_register_custom_schemes = Some(Self::on_register_custom_schemes);

        let app = Box::new(Self {
            base,
            ref_count: std::sync::atomic::AtomicI32::new(1),
        });
        Box::into_raw(app) as *mut _cef_app_t
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

    unsafe extern "C" fn on_before_command_line_processing(
        _self: *mut _cef_app_t,
        _process_type: *const cef_string_t,
        command_line: *mut _cef_command_line_t,
    ) {
        if command_line.is_null() {
            return;
        }
        let cl = &*command_line;
        if let Some(append_switch) = cl.append_switch {
            // Disable GPU acceleration when running in CI or headless environments to avoid DX11 driver failures
            if std::env::var("CI").is_ok() || std::env::args().any(|a| a == "--smoke-test") {
                let s_gpu = cef_string_from_str("disable-gpu");
                let s_shm = cef_string_from_str("disable-dev-shm-usage");
                append_switch(command_line, &s_gpu);
                append_switch(command_line, &s_shm);
            }
        }
    }

    unsafe extern "C" fn on_register_custom_schemes(
        _self: *mut _cef_app_t,
        registrar: *mut _cef_scheme_registrar_t,
    ) {
        if registrar.is_null() {
            return;
        }
        let reg = &*registrar;
        if let Some(add_custom_scheme) = reg.add_custom_scheme {
            let scheme_name = cef_string_from_str("joren");
            // Standard (1) | Secure (8) | CORS (16) | Fetch (64) = 89
            let options = 1 | 8 | 16 | 64;
            let res = add_custom_scheme(registrar, &scheme_name, options);
            crate::cef::log_startup(
                "SCHEME",
                &format!("Custom scheme 'joren' registered in on_register_custom_schemes (result={})", res),
            );
        }
    }
}
