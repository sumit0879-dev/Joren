# CEF Blank UI & Runtime Failure Recovery Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Diagnose and repair the blank white content area and early exit in the packaged Windows CEF desktop application, implement full custom scheme lifecycle registration, guarantee C ABI memory layout across all CEF wrapper structs, establish a deterministic asset and IPC bridge, eliminate fake compatibility stubs, and build an end-to-end native runtime smoke test that validates real browser creation and React mounting.

**Architecture:** 
1. Memory Layout: Add `#[repr(C)]` to all Rust wrapper structs (`JorenResourceHandler`, `JorenSchemeHandlerFactory`, `JorenCefClient`, `JorenLifeSpanHandler`, `JorenLoadHandler`, `JorenCefApp`) so that CEF base ref-counted structs are guaranteed at offset 0, fixing the fatal crash `Cannot wrap struct with invalid base.size value (got 1347, expected 96)`.
2. Process & Scheme Architecture: Implement `JorenCefApp` implementing `cef_app_t` and `on_register_custom_schemes` registering `joren://` with standard, secure, cors-enabled, and fetch-enabled scheme options across all CEF processes (`cef_execute_process` and `cef_initialize`).
3. Scheme & Asset Handler: Robustly parse `joren://app/...` URLs, map file extensions to exact MIME types, prevent path traversal, prevent SPA fallback for static asset requests, and route `/api/invoke` commands to `JorenCore::dispatch`.
4. Command Inventory & Dispatch: Audit all frontend `invoke()` calls, implement real handlers in `JorenCore::dispatch`, and return structured errors for unsupported commands instead of dummy success.
5. React Readiness & Real Smoke Test: Expose `window.__JOREN_APP_READY__` in React, create a real Win32 host window and CEF browser in `--smoke-test`, wait for frame load and React mount, test IPC roundtrip from the browser, and cleanly shut down.

**Tech Stack:** Rust 2021, Windows Win32 API, Chromium Embedded Framework (CEF 156.0.15 / `cef-dll-sys 156.3.0`), TypeScript, React 19, Vite, GitHub Actions.

**Spec:** User request in prompt detailing Findings A through F, local logs at `%APPDATA%\Joren\logs`, and runtime crash logs.

## Global Constraints
- Target Git Branch: `feat/full-cef-runtime-migration` on PR #2.
- Never push directly to `main`; never merge PR #2; leave PR #2 open.
- CEF runtime distribution pinned to `156.0.15+gcf671c0+chromium-156.0.8078.80_windows64`.
- Mandatory Chromium Sandbox (`no_sandbox = 0`). Fail-closed if `sandbox_info` is null.
- Zero fake compatibility stubs: no dummy success returns for unimplemented commands.
- Both GitHub checks (`CI` and `Windows Build`) must be green with verified runtime smoke test.

---

## File Structure

- `src-tauri/src/cef/types.rs`: Static layout assertions for all `#[repr(C)]` wrapper structs.
- `src-tauri/src/cef/api.rs`: C FFI definitions and symbol loaders for `cef_register_scheme_handler_factory`, `cef_api_hash`, and scheme registration.
- `src-tauri/src/cef/app.rs`: Dedicated `JorenCefApp` implementation (`cef_app_t`) with `on_register_custom_schemes`.
- `src-tauri/src/cef/scheme.rs`: `#[repr(C)]` on `JorenResourceHandler` and `JorenSchemeHandlerFactory`, robust URL parsing, strict MIME resolution, no index.html fallback for assets, and `/api/invoke` dispatch.
- `src-tauri/src/cef/mod.rs`: Integration of `JorenCefApp` into `execute_process` and `initialize`, Win32 host window, verified scheme factory registration, and genuine runtime smoke test.
- `src-tauri/src/core.rs`: Comprehensive frontend command inventory dispatch, structured errors for unknown commands.
- `src-tauri/tauri-compat/src/lib.rs`: Elimination of misleading fake methods.
- `src/main.tsx` / `src/App.tsx`: Expose `window.__JOREN_APP_READY__` sentinel.
- `.github/workflows/windows-build.yml`: Sandboxed native launch smoke test validating browser creation, main frame load, and React readiness.

---

## Tasks

### Task 1: Fix Memory Layout (`#[repr(C)]`) & Add Struct Offset Tests
- [ ] Add `#[repr(C)]` to `JorenResourceHandler` and `JorenSchemeHandlerFactory` in `src-tauri/src/cef/scheme.rs`.
- [ ] Add `#[repr(C)]` to `JorenCefClient`, `JorenLifeSpanHandler`, and `JorenLoadHandler` in `src-tauri/src/cef/mod.rs`.
- [ ] Add unit tests in `src-tauri/src/cef/types.rs` testing `std::mem::offset_of!(Struct, base) == 0` for all wrapper types.

### Task 2: Implement `JorenCefApp` and Custom Scheme Registration
- [ ] Create `src-tauri/src/cef/app.rs` with `#[repr(C)] pub struct JorenCefApp`.
- [ ] Implement `cef_app_t` callbacks: `add_ref`, `release`, `has_one_ref`, `has_at_least_one_ref`, and `on_register_custom_schemes`.
- [ ] In `on_register_custom_schemes`, register `joren` with `CEF_SCHEME_OPTION_STANDARD | CEF_SCHEME_OPTION_SECURE | CEF_SCHEME_OPTION_CORS_ENABLED | CEF_SCHEME_OPTION_FETCH_ENABLED`.
- [ ] Update `src-tauri/src/cef/mod.rs` to pass `JorenCefApp::new()` into both `api.execute_process` and `api.initialize`.
- [ ] Verify `(api.register_scheme_handler_factory)` return value, log result, and fail-closed if 0.

### Task 3: Repair Resource & Asset Resolution in `scheme.rs`
- [ ] Parse `joren://app/...` URLs cleanly using standard string splitting and URL decoding.
- [ ] Reject path traversal (`..`, `:`, backslashes).
- [ ] Map all production extensions (.html, .js, .mjs, .css, .json, .svg, .png, .jpg, .webp, .ico, .woff, .woff2, .wasm).
- [ ] Only fallback to `index.html` for extensionless routes. For missing assets with extensions, return HTTP 404.
- [ ] Log every served resource with HTTP status and size.

### Task 4: Command Inventory Audit & Clean up Fake Stubs
- [ ] Inventory all `invoke()` calls from `src/services/tauri.ts`, `src/services/browserController.ts`, and frontend components.
- [ ] Ensure all invoked commands are handled in `JorenCore::dispatch`.
- [ ] For any unhandled command, return `{ "error": "Command not implemented", "command": cmd }` with error status. Remove generic `{ "success": true }`.
- [ ] Remove misleading no-op fake methods from `tauri-compat/src/lib.rs`.

### Task 5: Expose React Readiness Sentinel
- [ ] In `src/App.tsx`, set `(window as any).__JOREN_APP_READY__ = true;` once the application mounts and initial store loads.
- [ ] Add error listeners for `window.addEventListener('error')` and `unhandledrejection` logging to console.

### Task 6: Implement Genuine Runtime Smoke Test
- [ ] Update `--smoke-test` in `src-tauri/src/cef/mod.rs`:
  - Create Win32 host window (offscreen / hidden if smoke test).
  - Create CEF browser pointing to `joren://app/index.html`.
  - In `on_after_created`, record browser handle.
  - In `on_load_end`, check if main frame URL is `joren://app/index.html` and HTTP status is 200.
  - Run message loop with timeout (up to 20 seconds).
  - Verify frame load and React readiness, verify backend ping via IPC, and trigger clean shutdown (`quit_message_loop`).
  - Return exit code 0 on complete verified success.

### Task 7: Verification, Git Push & CI/CD Validation
- [ ] Run unit tests and cargo check.
- [ ] Commit and push to `feat/full-cef-runtime-migration`.
- [ ] Monitor GitHub Actions `CI` and `Windows Build` until both are green.
- [ ] Verify generated artifacts (`joren-setup-x64.exe` and portable zip).
- [ ] Produce final evidence report while keeping PR #2 open.
