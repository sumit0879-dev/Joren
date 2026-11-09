//! Joren CEF Browser Engine Adapter
//! Decouples high-level AI tools, risk engine, and business logic from the native CEF runtime.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserEngineTabInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    pub is_active: bool,
    pub is_internal: bool,
}

pub trait BrowserEngine: Send + Sync {
    fn create_tab(&self, tab_id: Option<&str>, url: Option<&str>) -> Result<serde_json::Value, String>;
    fn navigate(&self, tab_id: &str, url: &str) -> Result<String, String>;
    fn close_tab(&self, tab_id: &str) -> Result<serde_json::Value, String>;
    fn switch_tab(&self, tab_id: &str) -> Result<serde_json::Value, String>;
    fn get_state(&self) -> Result<serde_json::Value, String>;
    fn observe(&self, tab_id: &str) -> Result<serde_json::Value, String>;
    fn click(&self, tab_id: &str, element_id: &str) -> Result<serde_json::Value, String>;
    fn type_text(&self, tab_id: &str, element_id: &str, text: &str, clear_first: bool) -> Result<serde_json::Value, String>;
    fn select_option(&self, tab_id: &str, element_id: &str, value: &str) -> Result<serde_json::Value, String>;
    fn focus_element(&self, tab_id: &str, element_id: &str) -> Result<serde_json::Value, String>;
    fn scroll(&self, tab_id: &str, x: i32, y: i32) -> Result<serde_json::Value, String>;
    fn press_key(&self, tab_id: &str, key: &str) -> Result<serde_json::Value, String>;
    fn screenshot(&self, tab_id: &str) -> Result<serde_json::Value, String>;
    fn wait_for_condition(&self, tab_id: &str, condition_js: &str, timeout_ms: u64) -> Result<serde_json::Value, String>;
    fn zoom_set(&self, tab_id: &str, level: f64) -> Result<serde_json::Value, String>;
    fn find_in_page(&self, tab_id: &str, query: &str, forward: bool) -> Result<serde_json::Value, String>;
    fn print_tab(&self, tab_id: &str) -> Result<serde_json::Value, String>;
    fn save_page_html(&self, tab_id: &str, destination_path: &str) -> Result<serde_json::Value, String>;
}

pub struct CefBrowserEngine;

impl CefBrowserEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CefBrowserEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserEngine for CefBrowserEngine {
    fn create_tab(&self, tab_id: Option<&str>, url: Option<&str>) -> Result<serde_json::Value, String> {
        let mut map = serde_json::Map::new();
        if let Some(id) = tab_id {
            map.insert("tabId".to_string(), serde_json::json!(id));
        }
        if let Some(u) = url {
            map.insert("url".to_string(), serde_json::json!(u));
        }
        crate::cef::tabs::create_tab(&serde_json::Value::Object(map))
    }

    fn navigate(&self, tab_id: &str, url: &str) -> Result<String, String> {
        let res = crate::cef::tabs::navigate_tab(&serde_json::json!({
            "tabId": tab_id,
            "url": url,
        }))?;
        Ok(res.as_str().unwrap_or("").to_string())
    }

    fn close_tab(&self, tab_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::close_tab(&serde_json::json!({ "tabId": tab_id }))
    }

    fn switch_tab(&self, tab_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::switch_tab(&serde_json::json!({ "tabId": tab_id }))
    }

    fn get_state(&self) -> Result<serde_json::Value, String> {
        crate::cef::tabs::get_state()
    }

    fn observe(&self, tab_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::observe_tab(&serde_json::json!({ "tabId": tab_id }))
    }

    fn click(&self, tab_id: &str, element_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::click_element(&serde_json::json!({
            "tabId": tab_id,
            "elementId": element_id,
        }))
    }

    fn type_text(&self, tab_id: &str, element_id: &str, text: &str, clear_first: bool) -> Result<serde_json::Value, String> {
        crate::cef::tabs::type_element(&serde_json::json!({
            "tabId": tab_id,
            "elementId": element_id,
            "text": text,
            "clearFirst": clear_first,
        }))
    }

    fn select_option(&self, tab_id: &str, element_id: &str, value: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::select_option(&serde_json::json!({
            "tabId": tab_id,
            "elementId": element_id,
            "value": value,
        }))
    }

    fn focus_element(&self, tab_id: &str, element_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::focus_element(&serde_json::json!({
            "tabId": tab_id,
            "elementId": element_id,
        }))
    }

    fn scroll(&self, tab_id: &str, x: i32, y: i32) -> Result<serde_json::Value, String> {
        crate::cef::tabs::scroll(&serde_json::json!({
            "tabId": tab_id,
            "x": x,
            "y": y,
        }))
    }

    fn press_key(&self, tab_id: &str, key: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::press_key(&serde_json::json!({
            "tabId": tab_id,
            "key": key,
        }))
    }

    fn screenshot(&self, tab_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::screenshot_tab(&serde_json::json!({ "tabId": tab_id }))
    }

    fn wait_for_condition(&self, tab_id: &str, condition_js: &str, timeout_ms: u64) -> Result<serde_json::Value, String> {
        crate::cef::tabs::wait_for_condition(&serde_json::json!({
            "tabId": tab_id,
            "condition": condition_js,
            "timeout_ms": timeout_ms,
        }))
    }

    fn zoom_set(&self, tab_id: &str, level: f64) -> Result<serde_json::Value, String> {
        crate::cef::tabs::zoom_set(&serde_json::json!({
            "tabId": tab_id,
            "level": level,
        }))
    }

    fn find_in_page(&self, tab_id: &str, query: &str, forward: bool) -> Result<serde_json::Value, String> {
        crate::cef::tabs::find_in_page(&serde_json::json!({
            "tabId": tab_id,
            "query": query,
            "forward": forward,
        }))
    }

    fn print_tab(&self, tab_id: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::print_tab(&serde_json::json!({ "tabId": tab_id }))
    }

    fn save_page_html(&self, tab_id: &str, destination_path: &str) -> Result<serde_json::Value, String> {
        crate::cef::tabs::save_page_html(&serde_json::json!({
            "tabId": tab_id,
            "path": destination_path,
        }))
    }
}

