# Joren Full CEF Runtime & Browser Functionality Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the false CEF initialization error dialog, establish reliable Windows single-instance lifecycle handling, wire all 114 unhandled frontend commands (94 browser and 20 non-browser), and extend CEF native tab rendering, DOM observation, and automated smoke testing to deliver a fully functional installed desktop application.

**Architecture:** 
1. Use CEF's official `cef_get_exit_code()` API and `_cef_browser_process_handler_t` (`on_already_running_app_relaunch`) alongside Win32 window detection (`FindWindowW` / `SetForegroundWindow`) to ensure duplicate launches activate the existing Joren window and exit cleanly with code 0 instead of displaying a false runtime error dialog.
2. Bridge all 114 missing frontend commands in `src-tauri/src/core.rs` by reusing the tested Rust domain modules (`policy`, `tools`, `memory`, `llm`, `chat`, `voice`, `browser_storage`, `browser_download`, `browser_profile`, `browser_privacy`, `browser_control`, `browser_risk`, `browser_agent`, `browser_orchestrator`, and `browser_recovery`).
3. Complete native multi-tab CEF surface orchestration in `src-tauri/src/cef/tabs.rs` to provide real tab creation, DOM observation with structured element IDs, element interaction, tab groups, session persistence, zooming, and screenshot capture.
4. Expand CI and packaging smoke tests to validate real browser tab lifecycle and single-instance behavior.

**Tech Stack:** Rust 1.77+, CEF 156.0.15 (`cef-dll-sys` 156.3.0), Win32 API (`windows-sys` 0.59), React 18, TypeScript, Vite, NSIS.

**Spec:** User master recovery prompt for PR #2 (`feat/full-cef-runtime-migration`).

## Global Constraints
- Active branch must remain `feat/full-cef-runtime-migration`.
- PR #2 must remain open and unmerged.
- Never push directly to `main`.
- Mandatory Chromium sandbox (`no_sandbox = 0`).
- No WebView2 dependency.
- Preserved user data directories and databases.

## Review Focus
1. Does launching a second instance of Joren focus the running window without showing `cef_initialize failed`?
2. Does `policy_list_pending_approvals` return HTTP 200 rather than HTTP 400 (`Unknown or unsupported command`)?
3. Does clicking tabs in the Browser view create, switch, and navigate real CEF child browsers?
4. Does `browser_observe_tab` return structured DOM elements rather than falling through to unsupported command?
5. Does the Windows Build and CI pass green with comprehensive smoke test coverage?

---

### Task 1: CEF Initialization, Process Lifecycle, and Single-Instance Architecture

**Files:**
- Modify: `src-tauri/src/cef/api.rs`
- Modify: `src-tauri/src/cef/app.rs`
- Modify: `src-tauri/src/cef/mod.rs`
- Modify: `src-tauri/src/cef/types.rs`

- [ ] **Step 1.1: Dynamically bind `cef_get_exit_code` in `src-tauri/src/cef/api.rs`**
  - Add `pub type FnCefGetExitCode = unsafe extern "C" fn() -> c_int;`
  - Add `pub get_exit_code: Option<FnCefGetExitCode>` to `CefApi`.
  - In `get_cef_api()`, load symbol `"cef_get_exit_code"` dynamically with fallback if not present.

- [ ] **Step 1.2: Implement `_cef_browser_process_handler_t` in `src-tauri/src/cef/app.rs`**
  - Create `#[repr(C)] pub struct JorenBrowserProcessHandler` implementing reference counting.
  - Implement `on_already_running_app_relaunch`:
    - Reads command line, activates the existing host window (`ShowWindow(hwnd, SW_RESTORE)`, `SetForegroundWindow(hwnd)`).
    - Returns `1` to signal the relaunch was handled.
  - Populate `base.get_browser_process_handler = Some(Self::get_browser_process_handler)` in `JorenCefApp`.

- [ ] **Step 1.3: Single-instance detection & `cef_initialize` exit code evaluation in `src-tauri/src/cef/mod.rs`**
  - Before `cef_initialize` in the browser process (when not a `--type=` subprocess):
    - Check for an existing top-level Joren window via Win32 `FindWindowW(class_name, windows_sys::core::w!("Joren"))`.
    - If found and responsive, activate it via `ShowWindow(hwnd, SW_RESTORE)` and `SetForegroundWindow(hwnd)`.
  - When `cef_initialize()` returns 0:
    - Query `api.get_exit_code()`.
    - If exit code is 0 (or if an existing window was activated): log `[INIT] Normal early exit / single-instance relaunch (exit code 0); exiting cleanly.` and return 0 immediately without showing any error dialog.
    - If exit code is non-zero and no existing instance exists: log `[INIT_ERROR] CEF initialization failed with exit code {code}` and display actionable diagnostic dialog including version, exit code, and log directory.

- [ ] **Step 1.4: Cache path validation**
  - Verify that `cef_cache_path` and `cef_log_path` are canonical absolute paths.
  - Set `settings.root_cache_path` and `settings.cache_path` consistently.

---

### Task 2: Native Dispatcher Wiring for Non-Browser Commands

**Files:**
- Modify: `src-tauri/src/core.rs`

- [ ] **Step 2.1: Wire Policy Engine Commands**
  - Add dispatch handlers for:
    - `"policy_list_pending_approvals"` -> `tokio_rt.block_on(crate::policy_list_pending_approvals(app.state()))`
    - `"policy_evaluate_action"` -> parse `request` & `context`, invoke `tokio_rt.block_on(crate::policy_evaluate_action(request, context, app.state()))`
    - `"policy_resolve_approval"` -> parse `approval_id` & `decision`, invoke `tokio_rt.block_on(crate::policy_resolve_approval(approval_id, decision, app.state()))`
    - `"policy_get_audit_log"` -> parse `limit`, invoke `tokio_rt.block_on(crate::policy_get_audit_log(limit, app.state()))`

- [ ] **Step 2.2: Wire Universal Tools Runtime Commands**
  - Add dispatch handlers for:
    - `"tools_list_definitions"` -> `crate::tools_list_definitions(app.state())`
    - `"tools_get_definition"` -> parse `name`, `crate::tools_get_definition(name, app.state())`
    - `"tools_execute"` -> parse `request`, `tokio_rt.block_on(crate::tools_execute(request, app.state()))`
    - `"tools_cancel_execution"` -> parse `execution_id`, `tokio_rt.block_on(crate::tools_cancel_execution(execution_id, app.state()))`

- [ ] **Step 2.3: Wire Memory Commands**
  - Add dispatch handlers for:
    - `"get_memories_cmd"` -> `tokio_rt.block_on(crate::memory::get_memories_cmd(app.clone()))`
    - `"save_to_memory_cmd"` -> parse `text` and `source`, `tokio_rt.block_on(crate::memory::save_to_memory_cmd(app.clone(), text, source))`
    - `"search_memory_cmd"` -> parse `query` and `limit`, `tokio_rt.block_on(crate::memory::search_memory_cmd(app.clone(), query, limit))`
    - `"delete_memory_cmd"` -> parse `id`, `tokio_rt.block_on(crate::memory::delete_memory_cmd(app.clone(), id))`

- [ ] **Step 2.4: Wire LLM, Chat & Voice Commands**
  - Add dispatch handlers for:
    - `"load_local_llm"` -> `tokio_rt.block_on(crate::llm::load_local_llm(model_path, context_size, gpu_layers))`
    - `"local_chat"` -> `tokio_rt.block_on(crate::llm::local_chat(messages, max_tokens, temperature))`
    - `"stop_local_llm"` -> `tokio_rt.block_on(crate::llm::stop_local_llm())`
    - `"chat_command"` -> `tokio_rt.block_on(crate::chat::chat_command(app.clone(), message, session_id, attachments, provider, model))`
    - `"realtime_voice_get_status"` -> `tokio_rt.block_on(crate::realtime_voice_get_status(app.state()))`
    - `"voice_get_device_status"` -> `tokio_rt.block_on(crate::voice_get_device_status(app.state()))`
    - `"voice_session_start"` -> `tokio_rt.block_on(crate::voice_session_start(conversation_id, app.state()))`
    - `"voice_session_submit_transcript"` -> `tokio_rt.block_on(crate::voice_session_submit_transcript(...))`

---

### Task 3: Browser Storage, Profile, Privacy, Downloads & Agent Dispatcher Integration

**Files:**
- Modify: `src-tauri/src/core.rs`

- [ ] **Step 3.1: Wire Browser Storage (History & Bookmarks)**
  - `"browser_history_add"` -> `crate::browser_storage::browser_history_add(app.clone(), url, title, tab_id, app.state())`
  - `"browser_history_get_recent"` -> `crate::browser_storage::browser_history_get_recent(limit, app.state())`
  - `"browser_history_search"` -> `crate::browser_storage::browser_history_search(query, limit, app.state())`
  - `"browser_history_delete"` -> `crate::browser_storage::browser_history_delete(app.clone(), id, app.state())`
  - `"browser_history_clear"` -> `crate::browser_storage::browser_history_clear(app.clone(), app.state())`
  - `"browser_bookmark_add"` -> `crate::browser_storage::browser_bookmark_add(app.clone(), url, title, folder_id, favicon, app.state())`
  - `"browser_bookmark_update"` -> `crate::browser_storage::browser_bookmark_update(app.clone(), id, url, title, folder_id, app.state())`
  - `"browser_bookmark_delete"` -> `crate::browser_storage::browser_bookmark_delete(app.clone(), id, app.state())`
  - `"browser_bookmarks_list"` -> `crate::browser_storage::browser_bookmarks_list(folder_id, app.state())`
  - `"browser_bookmarks_search"` -> `crate::browser_storage::browser_bookmarks_search(query, app.state())`
  - `"browser_bookmark_is_bookmarked"` -> `crate::browser_storage::browser_bookmark_is_bookmarked(url, app.state())`
  - `"browser_bookmark_create_folder"` -> `crate::browser_storage::browser_bookmark_create_folder(app.clone(), name, parent_id, app.state())`
  - `"browser_bookmark_delete_folder"` -> `crate::browser_storage::browser_bookmark_delete_folder(app.clone(), id, app.state())`

- [ ] **Step 3.2: Wire Downloads Manager**
  - `"browser_download_start"` -> `crate::browser_download::browser_download_start(...)`
  - `"browser_download_cancel"` -> `crate::browser_download::browser_download_cancel(...)`
  - `"browser_download_list"` -> `crate::browser_download::browser_download_list(...)`
  - `"browser_download_get"` -> `crate::browser_download::browser_download_get(...)`
  - `"browser_download_delete_record"` -> `crate::browser_download::browser_download_delete_record(...)`
  - `"browser_download_clear_records"` -> `crate::browser_download::browser_download_clear_records(...)`
  - `"browser_download_show_in_folder"` -> `crate::browser_download::browser_download_show_in_folder(...)`
  - `"browser_download_open_file"` -> `crate::browser_download::browser_download_open_file(...)`

- [ ] **Step 3.3: Wire Profiles Manager**
  - `"browser_profiles_list"` -> `crate::browser_profile::browser_profiles_list(app.state())`
  - `"browser_profile_get"` -> `crate::browser_profile::browser_profile_get(profile_id, app.state())`
  - `"browser_profile_create"` -> `crate::browser_profile::browser_profile_create(...)`
  - `"browser_profile_switch"` -> `crate::browser_profile::browser_profile_switch(...)`
  - `"browser_profile_rename"` -> `crate::browser_profile::browser_profile_rename(...)`
  - `"browser_profile_delete"` -> `crate::browser_profile::browser_profile_delete(...)`
  - `"browser_profile_create_temporary"` -> `crate::browser_profile::browser_profile_create_temporary(...)`
  - `"browser_profile_cleanup_temporary"` -> `crate::browser_profile::browser_profile_cleanup_temporary(...)`

- [ ] **Step 3.4: Wire Privacy, Risk & Safety Controls**
  - `"browser_privacy_get_status"` -> `crate::browser_privacy::browser_privacy_get_status(app.state())`
  - `"browser_privacy_toggle_protection"` -> `crate::browser_privacy::browser_privacy_toggle_protection(...)`
  - `"browser_privacy_allowlist_domain"` -> `crate::browser_privacy::browser_privacy_allowlist_domain(...)`
  - `"browser_privacy_remove_allowlist"` -> `crate::browser_privacy::browser_privacy_remove_allowlist(...)`
  - `"browser_privacy_add_block_rule"` -> `crate::browser_privacy::browser_privacy_add_block_rule(...)`
  - `"browser_privacy_remove_block_rule"` -> `crate::browser_privacy::browser_privacy_remove_block_rule(...)`
  - `"browser_privacy_list_rules"` -> `crate::browser_privacy::browser_privacy_list_rules(app.state())`
  - `"browser_privacy_get_tab_stats"` -> `crate::browser_privacy::browser_privacy_get_tab_stats(...)`
  - `"browser_privacy_reset_stats"` -> `crate::browser_privacy::browser_privacy_reset_stats(...)`
  - `"browser_request_ai_control"` -> `crate::browser_control::browser_request_ai_control(...)`
  - `"browser_takeover_tab"` -> `crate::browser_control::browser_takeover_tab(...)`
  - `"browser_release_ai_control"` -> `crate::browser_control::browser_release_ai_control(...)`
  - `"browser_pause_ai_control"` -> `crate::browser_control::browser_pause_ai_control(...)`
  - `"browser_resume_ai_control"` -> `crate::browser_control::browser_resume_ai_control(...)`
  - `"browser_get_tab_control_info"` -> `crate::browser_control::browser_get_tab_control_info(...)`
  - `"browser_get_all_tab_controls"` -> `crate::browser_control::browser_get_all_tab_controls(...)`
  - `"browser_assess_action_risk"` -> `crate::browser_risk::browser_assess_action_risk(...)`
  - `"browser_get_risk_audit_log"` -> `crate::browser_risk::browser_get_risk_audit_log(...)`
  - `"browser_resolve_action_approval"` -> `crate::browser_risk::browser_resolve_action_approval(...)`
  - `"browser_run_startup_recovery"` -> `crate::browser_recovery::browser_run_startup_recovery(...)`

- [ ] **Step 3.5: Wire AI Agent & Orchestrator Tools**
  - `"browser_agent_run_task"` -> `crate::browser_agent::browser_agent_run_task(...)`
  - `"browser_agent_cancel_task"` -> `crate::browser_agent::browser_agent_cancel_task(...)`
  - `"browser_agent_get_current_task"` -> `crate::browser_agent::browser_agent_get_current_task(...)`
  - `"browser_orchestrator_run_task"` -> `crate::browser_orchestrator::browser_orchestrator_run_task(...)`
  - `"browser_orchestrator_cancel_task"` -> `crate::browser_orchestrator::browser_orchestrator_cancel_task(...)`
  - `"browser_orchestrator_get_current_task"` -> `crate::browser_orchestrator::browser_orchestrator_get_current_task(...)`
  - `"browser_get_tool_definitions_cmd"` -> `crate::browser_tools::get_browser_tool_definitions()`
  - `"browser_execute_tool_cmd"` -> `crate::browser_tools::execute_browser_tool(...)`

---

### Task 4: Full CEF Native Browser Surface, Tab Management, and DOM Automation Engine

**Files:**
- Modify: `src-tauri/src/cef/tabs.rs`
- Modify: `src-tauri/src/core.rs`
- Create: `src-tauri/tests/command_contract_test.rs`

- [ ] **Step 4.1: Extend Tab Management in `src-tauri/src/cef/tabs.rs`**
  - Implement `close_other_tabs(tab_id)`
  - Implement `close_tabs_to_right(tab_id)`
  - Implement `duplicate_tab(tab_id)`
  - Implement `reopen_last_closed_tab()`
  - Implement `toggle_pin_tab(tab_id)`
  - Implement `save_session()` & `restore_session()`
  - Implement Tab Group operations (`tab_group_create`, `tab_group_rename`, `tab_group_delete`, `tab_group_list`, `tab_group_set_collapsed`, `tab_group_move_tab`, `tab_group_remove_tab`, `tab_group_reorder`, `tab_group_close_tabs`).

- [ ] **Step 4.2: Implement DOM Observation & Element Queries**
  - Implement `observe_tab(tab_id)`: injects observation script that assigns deterministic `data-joren-eid` attributes to interactive elements (`<a>`, `<button>`, `<input>`, `<textarea>`, `<select>`, ARIA roles) and returns structured DOM tree, visible text, title, and URL.
  - Implement `get_tab_title(tab_id)` & `get_title()`
  - Implement `get_visible_text(tab_id)` & `get_tab_visible_text(tab_id)`

- [ ] **Step 4.3: Implement Element Interactions & Page Automation**
  - Implement `click_element(tab_id, element_id)` with scroll into view and verification.
  - Implement `type_element(tab_id, element_id, text)` with input dispatch and verification.
  - Implement `select_option(tab_id, element_id, value)`
  - Implement `focus_element(tab_id, element_id)`
  - Implement `scroll(tab_id, delta_y)`
  - Implement `press_key(tab_id, key)`
  - Implement `wait_for_condition(tab_id, selector, timeout_ms)`

- [ ] **Step 4.4: Implement Page Tools (Screenshots, Zoom, Print, Reader Mode, HTML)**
  - Implement `screenshot_tab(tab_id)`: captures window bitmap of the child HWND via Win32 `PrintWindow` or GDI `BitBlt`, encodes to PNG base64.
  - Implement `zoom_in`, `zoom_out`, `zoom_reset`, `zoom_set`.
  - Implement `find_in_page` & `clear_find`.
  - Implement `print_tab`.
  - Implement `save_page_html`.
  - Implement `reader_extract`, `reader_mode_enter`, `reader_mode_exit`, `reader_mode_get`.

- [ ] **Step 4.5: Automated Command Contract Inventory Test**
  - Create `src-tauri/tests/command_contract_test.rs` reading `extracted_commands.json` (or hardcoded complete contract) and asserting every single command maps to a recognized arm in `JorenCore::dispatch()`.

---

### Task 5: Extended Native Smoke Test, Packaging Validation, and CI/Windows Build Verification

**Files:**
- Modify: `src-tauri/src/cef/mod.rs`
- Modify: `.github/workflows/windows-build.yml`
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 5.1: Extend `--smoke-test` sequence in `src-tauri/src/cef/mod.rs`**
  - Verify React UI mount (`__JOREN_APP_READY__`).
  - Verify `policy_list_pending_approvals` IPC return.
  - Call `browser_create_tab` for a test tab.
  - Call `browser_navigate_tab` with a data URL or test page.
  - Call `browser_observe_tab` and verify DOM elements return.
  - Call `browser_type_element` and `browser_click_element`.
  - Call `browser_screenshot_tab` and verify PNG bytes return.
  - Call `browser_close_tab`.
  - Cleanly notify success and exit 0.

- [ ] **Step 5.2: Update `.github/workflows/windows-build.yml`**
  - Ensure smoke test captures and logs the extended browser functionality stages.
  - Validate portable ZIP package extraction and launch test.
  - Validate NSIS installer compilation and integrity.

- [ ] **Step 5.3: Commit, Push, and Monitor Checks**
  - Commit fixes to `feat/full-cef-runtime-migration`.
  - Push to remote `origin`.
  - Monitor `CI` and `Windows Build` runs until both pass green.
  - Leave PR #2 open and prepare final report.

