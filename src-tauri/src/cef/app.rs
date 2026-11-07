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
            // Standard (1) | Display Isolated (4) | Secure (8) | CORS (16) | Fetch (64) = 93
            let options = 1 | 4 | 8 | 16 | 64;
            let res = add_custom_scheme(registrar, &scheme_name, options);
            crate::cef::log_startup(
                "SCHEME",
                &format!("Custom scheme 'joren' registered in on_register_custom_schemes (result={})", res),
            );
        }
    }
}
