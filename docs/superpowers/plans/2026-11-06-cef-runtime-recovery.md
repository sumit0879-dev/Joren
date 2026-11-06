# Joren CEF Desktop Runtime Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Diagnose, repair, validate, and complete the failed-at-runtime CEF desktop application migration for Joren on Windows on PR #2 (`feat/full-cef-runtime-migration`), achieving a cleanly launching application with mandatory Chromium sandbox, version-matched CEF 156.0.15 generated bindings, full backend initialization, typed IPC & native events, runtime smoke test, and green CI/Windows Build checks.

**Architecture:** 
1. Use the official Chromium/CEF Windows sandbox bootstrap architecture: `joren.exe` (bundled official M138+ `bootstrap.exe`) loads `joren.dll` (Rust cdylib) and invokes exported `RunWinMain(hInstance, lpCmdLine, nCmdShow, sandbox_info, version_info)`.
2. Pin both CEF distribution and official Rust bindings to exact version `156.0.15` (Chromium 156.0.8078.80) with ABI-correct generated type layouts, reference-counted base handlers (`CefClient`, `CefLifeSpanHandler`, `CefLoadHandler`, `CefSchemeHandlerFactory`, `CefResourceHandler`), and dynamic symbol loader.
3. Decouple backend core initialization into `JorenCore::initialize(app_handle)` to execute database migrations, SQLite store, settings, AI providers, tools, policy engine, and voice controllers before CEF UI runs; bridge IPC commands and native DOM events between the React frontend (`joren://app/`) and the Rust backend.
4. Stage MSVC CRT runtime DLLs in the packaging pipeline and add a native automated smoke test step to `windows-build.yml` before artifact publishing.

**Tech Stack:** Rust 2021, Windows MSVC FFI (x86_64), Chromium Embedded Framework (CEF 156.0.15), `cef-dll-sys 156.3.0+156.0.15`, Win32 API (`windows-sys`), TypeScript, React 19, Vite, SQLite (`rusqlite`), NSIS installer.

**Spec:** Master Prompt: PR #2 (`feat/full-cef-runtime-migration`), Base `main`, Commit `ead2804034c36ea521edfddffd6e6b632e8e7cfb`.

## Global Constraints
- Work strictly on branch `feat/full-cef-runtime-migration` and update PR #2.
- Never push to `main`; never merge PR #2; never close PR #2 or delete the branch.
- Mandatory Chromium sandbox (`no_sandbox = 0`, fail-closed if `sandbox_info` is null). Never allow an unsandboxed fallback.
- Pinned CEF distribution: `156.0.15+gcf671c0+chromium-156.0.8078.80_windows64`.
- Frontend bundled at secure origin `joren://app/`; no unauthenticated localhost HTTP servers.
- Preserve legacy database migration, existing settings, user data, and local LLM runtime assets.
- Both GitHub checks (`CI` and `Windows Build`) must be green on the final commit.

## Review Focus
1. **Bootstrap loading failure (Error 126):** Missing MSVC CRT DLLs (`vcruntime140.dll`, `msvcp140.dll`) prevented `bootstrap.exe` from loading `joren.dll`. Tested and verified resolved by staging CRT DLLs and configuring static/dynamic CRT properly.
2. **CEF ABI struct layout corruption:** Broken hand-written `types.rs` structs (missing `size` in `cef_window_info_t`, wrong fields in `cef_settings_t`) caused memory corruption and launch crashes. Resolved with official generated CEF 156 bindings.
3. **Null CefClient & missing window resize handling:** Creating browser with null client dropped all lifecycle callbacks and prevented browser resizing with the host Win32 window (`WM_SIZE`). Resolved with real `CefClient` implementing `CefLifeSpanHandler` and `CefLoadHandler`.
4. **Backend initialization bypass & empty command dispatcher:** `run()` in `lib.rs` was never called by `RunWinMain`, leaving SQLite, AI providers, and tools uninitialized, and `set_command_dispatcher` unwired. Resolved by extracting `JorenCore::initialize` and implementing a typed command dispatcher.
5. **Silent startup crashes:** When startup or sandbox initialization failed, errors went to stderr and process disappeared silently. Resolved by adding persistent logging to `%APPDATA%\Joren\logs\startup.log` and user-facing Win32 `MessageBoxW` on fatal errors.

---

### Task 1: Replace Hand-Written CEF ABI Structures with Version-Matched Generated Bindings (CEF 156.0.15)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/cef/types.rs`
- Modify: `src-tauri/src/cef/api.rs`

**Interfaces:**
- Consumes: Official CEF 156.0.15 struct layouts from `cef-dll-sys`
- Produces: Correct ABI types (`cef_window_info_t`, `cef_settings_t`, `cef_browser_settings_t`, `cef_client_t`, `cef_life_span_handler_t`, `cef_load_handler_t`, `cef_version_info_t`) and dynamic symbol loader in `src-tauri/src/cef/api.rs`.

- [ ] **Step 1: Update `src-tauri/Cargo.toml` with `cef-dll-sys = { version = "156.3.0", features = ["dox"] }`**
- [ ] **Step 2: Re-export or generate exact CEF 156.0.15 types in `src-tauri/src/cef/types.rs`**
  Verify `_cef_window_info_t` has `size: usize` (112 bytes total), `_cef_settings_t` has exact CEF 156 fields (including `log_items`, without `chrome_runtime`), and `_cef_version_info_t` matches M138+ bootstrap layout.
- [ ] **Step 3: Update `src-tauri/src/cef/api.rs` to load all required CEF 156 C symbols**
  Add loaders for `cef_version_info`, `cef_string_userfree_utf16_free`, `cef_post_task`, etc.
- [ ] **Step 4: Verify type layouts with unit tests in `src-tauri/src/cef/types.rs`**
  Assert `std::mem::size_of::<cef_window_info_t>() == 112`, and settings struct sizes match expected ABI.

---

### Task 2: Implement Real CEF Browser Client, LifeSpan & Load Handlers, Win32 Window Resize & Persistent Diagnostics

**Files:**
- Modify: `src-tauri/src/cef/mod.rs`
- Modify: `src-tauri/src/cef/scheme.rs`

**Interfaces:**
- Consumes: `CefApi`, `cef_client_t`, `cef_life_span_handler_t`, `cef_load_handler_t`
- Produces: `JorenCefClient`, child browser HWND tracking, responsive `WM_SIZE` layout, persistent startup logging in `%APPDATA%\Joren\logs\startup.log`, fail-closed Windows error message box.

- [ ] **Step 1: Implement persistent structured logger in `src-tauri/src/cef/mod.rs`**
  Create helper `log_startup(stage: &str, details: &str)` writing timestamps, stage descriptions, and error details to `%APPDATA%\Joren\logs\startup.log`.
- [ ] **Step 2: Validate `sandbox_info` and inspect `version_info`**
  If `sandbox_info` is null, log fatal security error, display Win32 error dialog, and exit with code 1. Inspect `version_info` and log major/minor/patch CEF and Chromium versions.
- [ ] **Step 3: Implement `JorenCefClient` with `CefLifeSpanHandler` and `CefLoadHandler`**
  Attach `on_after_created` to capture the child browser `HWND` and store in `SURFACE_CONTROLLER`. Attach `on_load_end` and `on_load_error` to log navigation outcomes and trigger ready notifications.
- [ ] **Step 4: Handle Win32 `WM_SIZE` and update child browser window**
  In `wnd_proc`, query new client dimensions on `WM_SIZE` and call `SetWindowPos` on the child browser `HWND` so the CEF webview fills the window.
- [ ] **Step 5: Verify browser creation return code and error handling**
  Check `api.browser_host_create_browser(...) != 0`. If failed, log failure and show dialog.

---

### Task 3: Refactor Backend Core Initialization (`JorenCore`) & Build Real Command Dispatcher

**Files:**
- Create: `src-tauri/src/core.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/cef/mod.rs`
- Modify: `src-tauri/src/cef/scheme.rs`

**Interfaces:**
- Consumes: `tauri::AppHandle`, `db`, `ai`, `conversation`, `policy`, `tools`, `voice`, `runtime`
- Produces: `JorenCore::initialize(app_handle) -> Result<JorenCore, String>`, `dispatch_command(app_handle, cmd, args) -> Result<Value, String>`, and event bridge `app_handle.emit -> window.dispatchEvent(joren-event)`.

- [ ] **Step 1: Extract backend initialization into `src-tauri/src/core.rs`**
  Move the complete subsystem setup (database migrations, SQLite init, DbState, AgentState, BrowserState, EventEmitter, TaskRuntime, ProviderRegistry, ConversationCore, PolicyEngine, ToolRegistry, ToolRouter, VoiceController, RealtimeVoiceEngine, EdithRuntimeState) into `JorenCore::initialize(app_handle)`.
- [ ] **Step 2: Implement comprehensive typed command router `dispatch_command`**
  Route all invoked commands (`get_all_settings`, `save_setting`, `sync_settings`, `get_all_sessions`, `create_session`, `get_session_messages`, `save_session_message`, `get_providers`, `runtime_get_status`, `voice_list_devices`, `agent_status`, etc.) directly to the subsystem methods with type-safe argument extraction.
- [ ] **Step 3: Connect `set_command_dispatcher` in `RunWinMain` before CEF message loop**
  Register `dispatch_command` with `scheme::set_command_dispatcher` during startup.
- [ ] **Step 4: Connect native event bridge from `app_handle.emit` to CEF main frame**
  Store the main browser reference in `SURFACE_CONTROLLER`. When `app_handle.emit(event, payload)` is invoked, execute JavaScript `window.dispatchEvent(new CustomEvent('joren-event', { detail: { event: ..., payload: ... } }))` on the active frame.
- [ ] **Step 5: Clean up and audit `tauri-compat`**
  Ensure all necessary state management methods (`manage`, `state`, `try_state`, `emit`, `path`) work seamlessly with `JorenCore`.

---

### Task 4: Packaging, Installer & Runtime Smoke-Test Workflow Integration

**Files:**
- Modify: `.github/workflows/windows-build.yml`
- Modify: `.github/workflows/ci.yml`
- Modify: `src-tauri/installer.nsi`
- Modify: `src-tauri/.cargo/config.toml`

**Interfaces:**
- Consumes: CEF 156.0.15 distribution archive, MSVC CRT DLLs, staged `joren.exe`, `joren.dll`
- Produces: Automated `--smoke-test` CLI verification in `joren.exe`, staged CRT DLLs, portable ZIP, and NSIS installer.

- [ ] **Step 1: Add `--smoke-test` mode to `RunWinMain` / `joren.dll`**
  Support `--smoke-test` command-line argument: initialize sandbox, initialize CEF, verify `JorenCore::initialize`, verify ping IPC command, write smoke-test pass record to startup log, and exit cleanly with code 0 without opening blocking window loop.
- [ ] **Step 2: Update `.github/workflows/windows-build.yml` for CEF 156.0.15 & MSVC CRT staging**
  - Update CEF URL to `https://cef-builds.spotifycdn.com/cef_binary_156.0.15%2Bgcf671c0%2Bchromium-156.0.8078.80_windows64.tar.bz2` (cache key `cef-windows64-156.0.15-v1`).
  - Stage MSVC CRT DLLs (`vcruntime140.dll`, `vcruntime140_1.dll`, `msvcp140.dll`) into `staging\joren\`.
  - Add native launch smoke-test step: `staging\joren\joren.exe --smoke-test`, asserting exit code 0 before building installer and ZIP.
- [ ] **Step 3: Verify and update `src-tauri/installer.nsi`**
  Confirm all staged DLLs and assets in `staging\joren` are installed to `$INSTDIR` and shortcuts point to `$INSTDIR\joren.exe`.
- [ ] **Step 4: Update `.github/workflows/ci.yml`**
  Confirm `cargo test` passes cleanly.

---

### Task 5: End-to-End Verification, Git Commit, Push & CI Validation

**Files:**
- Git repository: `feat/full-cef-runtime-migration`

**Interfaces:**
- Consumes: All completed tasks and verified commits
- Produces: Green CI and Windows Build check runs on PR #2.

- [ ] **Step 1: Run local tests and verify clean compilation**
- [ ] **Step 2: Review git diff, verify no regressions or missing files**
- [ ] **Step 3: Commit all changes to `feat/full-cef-runtime-migration`**
- [ ] **Step 4: Push to `origin feat/full-cef-runtime-migration`**
- [ ] **Step 5: Monitor GitHub Actions workflows (`CI` and `Windows Build`) until both are green on the latest commit**
- [ ] **Step 6: Update `task.md` with final verification evidence and leave PR #2 open for user review**
