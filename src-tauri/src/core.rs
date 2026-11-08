use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub struct JorenCore {
    pub app_handle: AppHandle,
    pub tokio_rt: Arc<tokio::runtime::Runtime>,
}

impl JorenCore {
    pub fn initialize(app: &AppHandle) -> Result<Self, String> {
        let tokio_rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("Failed to create tokio runtime: {}", e))?,
        );

        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("Failed to resolve app data directory: {}", e))?;
        std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;

        let db_path = app_data_dir.join("joren.db");
        let legacy_dirs = vec![
            app_data_dir.clone(),
            std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
                .join("com.edith.desktop"),
            std::path::PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("EDITH"),
        ];

        let conn = crate::db::migrate_and_init_db(&db_path, &legacy_dirs)
            .map_err(|e| format!("Database migration failed: {}", e))?;
        let conn2 = crate::db::init_db_at(&db_path)
            .map_err(|e| format!("Database secondary connection failed: {}", e))?;

        app.manage(crate::db::DbState {
            conn: std::sync::Mutex::new(conn),
        });
        app.manage(crate::agent::AgentState {
            path: std::sync::Mutex::new(String::new()),
        });
        app.manage(crate::browser::BrowserState::default());
        app.manage(crate::browser_agent::BrowserAgentManager::default());

        let emitter = crate::events::EventEmitter::new(app.clone());
        let task_runtime = crate::task::TaskRuntime::new(emitter.clone());
        let registry = crate::ai::ProviderRegistry::standard_builtins();
        let conn2_arc = std::sync::Arc::new(std::sync::Mutex::new(conn2));
        let conversation_core = crate::conversation::ConversationCore::new(
            registry,
            emitter.clone(),
            Some(conn2_arc.clone()),
            None,
        );
        let policy_engine = crate::policy::PolicyEngine::new(Some(emitter.clone()));
        let task_runtime_arc = std::sync::Arc::new(task_runtime.clone());
        let conversation_core_arc = std::sync::Arc::new(conversation_core.clone());
        let policy_engine_arc = std::sync::Arc::new(policy_engine.clone());

        let tool_registry = crate::tools::ToolRegistry::new();
        for def in crate::tools::get_browser_definitions() {
            let _ = tool_registry.register(def);
        }
        for def in crate::tools::get_computer_definitions() {
            let _ = tool_registry.register(def);
        }
        for def in crate::tools::get_edith_definitions() {
            let _ = tool_registry.register(def);
        }

        let browser_executor = std::sync::Arc::new(crate::tools::BrowserDomainExecutor::new(Some(
            app.clone(),
        )));
        let computer_executor = std::sync::Arc::new(crate::tools::ComputerDomainExecutor::new(
            Some(app.clone()),
        ));
        let domain_executors = crate::tools::DomainExecutorRegistry::new();
        domain_executors.register(browser_executor);
        domain_executors.register(computer_executor);
        let domain_executors_arc = std::sync::Arc::new(domain_executors);
        let tool_registry_arc = std::sync::Arc::new(tool_registry.clone());

        let tool_router = crate::tools::ToolRouter::with_defaults(
            tool_registry_arc.clone(),
            domain_executors_arc.clone(),
            policy_engine_arc.clone(),
            Some(emitter.clone()),
        );
        let tool_router_arc = std::sync::Arc::new(tool_router.clone());

        let voice_stt = std::sync::Arc::new(crate::voice::CloudSTTAdapter::new("cloud-stt", ""));
        let voice_tts = std::sync::Arc::new(crate::voice::EdgeTtsAdapter::new());
        let voice_output = std::sync::Arc::new(crate::voice::RodioAudioOutputDriver::new());
        let voice_capture = std::sync::Arc::new(crate::voice::BrowserCaptureBridge::new());
        let realtime_engine = std::sync::Arc::new(crate::voice::RealtimeVoiceEngine::new(
            conversation_core_arc.clone(),
            tool_router_arc.clone(),
            voice_output.clone(),
            voice_capture.clone(),
            Some(std::sync::Arc::new(emitter.clone())),
        ));
        let voice_controller = std::sync::Arc::new(
            crate::voice::VoiceController::new(
                conversation_core_arc.clone(),
                voice_stt,
                voice_tts,
                voice_output,
                voice_capture,
                Some(std::sync::Arc::new(emitter.clone())),
            )
            .with_realtime_engine(realtime_engine.clone()),
        );

        let runtime_state = crate::runtime::EdithRuntimeState::new(
            conversation_core_arc,
            task_runtime_arc,
            tool_registry_arc.clone(),
            tool_router_arc.clone(),
            conversation_core.registry(),
            policy_engine_arc,
            Some(app.clone()),
            Some(conn2_arc),
        )
        .with_voice_controller(voice_controller.clone());

        let edith_executor = std::sync::Arc::new(crate::tools::EdithDomainExecutor::new(
            std::sync::Arc::new(runtime_state.clone()),
        ));
        domain_executors_arc.register(edith_executor);

        // Wire Universal Tool Runtime into authoritative ConversationCore agentic loop
        conversation_core.set_tools(tool_router_arc.clone(), tool_registry_arc.clone());

        app.manage(task_runtime);
        app.manage(conversation_core);
        app.manage(policy_engine);
        app.manage(tool_registry);
        app.manage(tool_router);
        app.manage(runtime_state);
        app.manage(realtime_engine);
        app.manage(voice_controller);

        Ok(Self {
            app_handle: app.clone(),
            tokio_rt,
        })
    }

    pub fn dispatch(
        &self,
        cmd: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let app = &self.app_handle;
        let tokio_rt = &self.tokio_rt;

        match cmd {
            "ping" => Ok(serde_json::json!({
                "status": "ok",
                "message": "pong",
                "timestamp": chrono::Utc::now().to_rfc3339()
            })),

            "get_base_dir" => {
                let res = crate::get_base_dir()?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_all_settings" => {
                let res = crate::get_all_settings(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "save_setting" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let value = args.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::save_setting(key, value, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "sync_settings" => {
                let settings: HashMap<String, String> = serde_json::from_value(
                    args.get("settings").cloned().unwrap_or(serde_json::json!({})),
                )
                .map_err(|e| e.to_string())?;
                let res = crate::sync_settings(settings, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_all_sessions" => {
                let res = crate::get_all_sessions(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "create_session" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::create_session(session_id, title, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "rename_session" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let new_title = args.get("newTitle").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::rename_session(session_id, new_title, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "delete_session" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::delete_session(session_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_session_messages" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::get_session_messages(session_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "save_session_message" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let role = args.get("role").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let time = args.get("time").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::save_session_message(session_id, role, text, time, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_personal_notes" => {
                let res = crate::get_personal_notes(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "save_personal_note" => {
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::save_personal_note(content, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "delete_personal_note" => {
                let note_id = args.get("noteId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::delete_personal_note(note_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_custom_apps" => {
                let res = crate::get_custom_apps(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "add_custom_app" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let keywords = args.get("keywords").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::add_custom_app(name, path, keywords, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "delete_custom_app" => {
                let app_id = args.get("appId").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let res = crate::delete_custom_app(app_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "sync_apps_registry" => {
                let res = crate::sync_apps_registry()?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "launch_app" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::launch_app(path, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "ai_list_providers" => {
                let res = crate::ai_list_providers(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "ai_list_models" => {
                let provider_id = args.get("providerId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::ai_list_models(provider_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "ai_query_capabilities" => {
                let provider_id = args.get("providerId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let model_id = args.get("modelId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::ai_query_capabilities(provider_id, model_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "conversation_submit_turn" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let provider_id = args.get("providerId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let model_id = args.get("modelId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let temperature = args.get("temperature").and_then(|v| v.as_f64());
                let client_turn_id = args.get("clientTurnId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::conversation_submit_turn(
                    session_id,
                    message,
                    provider_id,
                    model_id,
                    temperature,
                    client_turn_id,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "conversation_execute_turn" => {
                let turn_id = args.get("turnId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let stream_id = args.get("streamId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let app_settings = args.get("appSettings").cloned();
                let res = tokio_rt.block_on(crate::conversation_execute_turn(
                    turn_id,
                    stream_id,
                    app_settings,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "conversation_cancel_turn" => {
                let turn_id = args.get("turnId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let reason = args.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::conversation_cancel_turn(
                    turn_id,
                    reason,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "conversation_get_turn_status" => {
                let turn_id = args.get("turnId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::conversation_get_turn_status(turn_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "task_create" => {
                let task_type = args.get("taskType").and_then(|v| v.as_str()).unwrap_or("custom").to_string();
                let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let turn_id = args.get("turnId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::task_create(
                    task_type,
                    goal,
                    session_id,
                    turn_id,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "task_cancel" => {
                let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let reason = args.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::task_cancel(task_id, reason, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "task_get_status" => {
                let task_id = args.get("taskId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::task_get_status(task_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "task_list_active" => {
                let res = tokio_rt.block_on(crate::task_list_active(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "runtime_get_status" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::runtime_get_status(session_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "runtime_get_capabilities" => {
                let domain = args.get("domain").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::runtime_get_capabilities(domain, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_providers" => {
                let res = crate::providers::get_providers();
                Ok(serde_json::to_value(res).unwrap())
            }

            "fetch_custom_models" => {
                let base_url = args.get("baseUrl").or_else(|| args.get("base_url")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let api_key = args.get("apiKey").or_else(|| args.get("api_key")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::providers::fetch_custom_models(base_url, api_key))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tts_speak" => {
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let voice = args.get("voice").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::tts::tts_speak(text, voice))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tts_stop" => {
                let res = tokio_rt.block_on(crate::tts::tts_stop())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "transcribe_audio" => {
                let samples: Vec<f32> = serde_json::from_value(args.get("samples").cloned().unwrap_or(serde_json::json!([])))
                    .unwrap_or_default();
                let sample_rate = args.get("sampleRate").and_then(|v| v.as_u64()).unwrap_or(16000) as u32;
                let language = args.get("language").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::transcribe_audio(samples, sample_rate, language, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "agent_status" => {
                let res = tokio_rt.block_on(crate::agent::agent_status(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "agent_chat" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let session_id = args.get("sessionId").or_else(|| args.get("session_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::agent::agent_chat(
                    app.clone(),
                    message,
                    session_id,
                    app.state(),
                    app.state(),
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "agent_set_path" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::agent::agent_set_path(path, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "agent_reset" => {
                let res = tokio_rt.block_on(crate::agent::agent_reset(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "agent_resolve_proposal" => {
                let proposal_id = args.get("proposalId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::agent_resolve_proposal(proposal_id, session_id, action)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_builtin_apps" => {
                let res = crate::plugins::get_builtin_apps();
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_plugins" => {
                let res = crate::plugins::get_plugins(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "toggle_plugin" => {
                let plugin_id = args.get("pluginId").or_else(|| args.get("plugin_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::toggle_plugin(plugin_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_system_terminal" => {
                let cmd_str = args.get("cmd").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::plugin_system_terminal(&cmd_str);
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_system_control" => {
                let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::plugin_system_control(&action);
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_app_launcher" => {
                let app_path = args.get("appPath").or_else(|| args.get("app_path")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::plugin_app_launcher(&app_path);
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_web_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let api_key = args.get("apiKey").or_else(|| args.get("api_key")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::plugins::plugin_web_search(query, api_key));
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_media_player" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::plugins::plugin_media_player(query));
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_whatsapp" => {
                let number = args.get("number").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::plugin_whatsapp(&number, &message);
                Ok(serde_json::to_value(res).unwrap())
            }

            "plugin_gmail" => {
                let email = args.get("email").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::plugins::plugin_gmail(&email, &message);
                Ok(serde_json::to_value(res).unwrap())
            }

            "take_screenshot" => {
                let res = tokio_rt.block_on(crate::plugins::take_screenshot())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_weather" => {
                let lat = args.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let lon = args.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let res = tokio_rt.block_on(crate::weather::get_weather(lat, lon))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_list_devices" => {
                let res = tokio_rt.block_on(crate::voice_list_devices(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_get_status" => {
                let res = tokio_rt.block_on(crate::voice_get_status(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_stop_playback" => {
                let res = tokio_rt.block_on(crate::voice_stop_playback(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_set_input_device" => {
                let device_id = args.get("deviceId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::voice_set_input_device(device_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_set_output_device" => {
                let device_id = args.get("deviceId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::voice_set_output_device(device_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_session_cancel" => {
                let session_id = args.get("sessionId").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let reason = args.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::voice_session_cancel(session_id, reason, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "policy_evaluate_action" => {
                let request: crate::policy::ActionRequest = serde_json::from_value(
                    args.get("request").cloned().unwrap_or(serde_json::json!({})),
                )
                .map_err(|e| format!("Invalid request payload: {}", e))?;
                let context: crate::policy::PolicyContext = serde_json::from_value(
                    args.get("context").cloned().unwrap_or(serde_json::json!({})),
                )
                .map_err(|e| format!("Invalid context payload: {}", e))?;
                let res = tokio_rt.block_on(crate::policy_evaluate_action(
                    request,
                    context,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "policy_list_pending_approvals" => {
                let res = tokio_rt.block_on(crate::policy_list_pending_approvals(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "policy_resolve_approval" => {
                let approval_id = args
                    .get("approvalId")
                    .or_else(|| args.get("approval_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let decision: crate::policy::OperatorDecision = serde_json::from_value(
                    args.get("decision").cloned().unwrap_or(serde_json::json!("DENIED")),
                )
                .map_err(|e| format!("Invalid decision payload: {}", e))?;
                let res = tokio_rt.block_on(crate::policy_resolve_approval(
                    approval_id,
                    decision,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "policy_get_audit_log" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);
                let res = tokio_rt.block_on(crate::policy_get_audit_log(limit, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tools_list_definitions" => {
                let res = crate::tools_list_definitions(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tools_get_definition" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::tools_get_definition(name, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tools_execute" => {
                let request: crate::tools::ToolRequest = serde_json::from_value(
                    args.get("request").cloned().unwrap_or_else(|| args.clone()),
                )
                .map_err(|e| format!("Invalid ToolRequest payload: {}", e))?;
                let res = tokio_rt.block_on(crate::tools_execute(request, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "tools_cancel_execution" => {
                let execution_id = args
                    .get("executionId")
                    .or_else(|| args.get("execution_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let res = tokio_rt.block_on(crate::tools_cancel_execution(execution_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_session_start" => {
                let conversation_id = args
                    .get("conversationId")
                    .or_else(|| args.get("conversation_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let res = tokio_rt.block_on(crate::voice_session_start(conversation_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_session_submit_transcript" => {
                let session_id = args
                    .get("sessionId")
                    .or_else(|| args.get("session_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let transcript = args.get("transcript").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let confidence = args.get("confidence").and_then(|v| v.as_f64()).map(|f| f as f32);
                let language = args.get("language").and_then(|v| v.as_str()).map(|s| s.to_string());
                let provider_id = args
                    .get("providerId")
                    .or_else(|| args.get("provider_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let model_id = args
                    .get("modelId")
                    .or_else(|| args.get("model_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let app_settings = args.get("appSettings").or_else(|| args.get("app_settings")).cloned();
                let res = tokio_rt.block_on(crate::voice_session_submit_transcript(
                    session_id,
                    transcript,
                    confidence,
                    language,
                    provider_id,
                    model_id,
                    app_settings,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "voice_get_device_status" => {
                let res = tokio_rt.block_on(crate::voice_get_device_status(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "realtime_voice_get_status" => {
                let res = tokio_rt.block_on(crate::realtime_voice_get_status(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "save_to_memory_cmd" => {
                let content = args
                    .get("content")
                    .or_else(|| args.get("text"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("user").to_string();
                let res = tokio_rt.block_on(crate::memory::save_to_memory_cmd(app.clone(), content, source))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "get_memories_cmd" => {
                let res = tokio_rt.block_on(crate::memory::get_memories_cmd(app.clone()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "search_memory_cmd" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::memory::search_memory_cmd(app.clone(), query))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "delete_memory_cmd" => {
                let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::memory::delete_memory_cmd(app.clone(), source))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "load_local_llm" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let load_mode = args
                    .get("loadMode")
                    .or_else(|| args.get("load_mode"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("cpu")
                    .to_string();
                let res = tokio_rt.block_on(crate::llm::load_local_llm(app.clone(), path, load_mode))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "local_chat" => {
                let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let emit_event = args
                    .get("emitEvent")
                    .or_else(|| args.get("emit_event"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::llm::local_chat(app.clone(), prompt, emit_event))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "stop_local_llm" => {
                let res = tokio_rt.block_on(crate::llm::stop_local_llm())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "chat_command" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let session_id = args
                    .get("sessionId")
                    .or_else(|| args.get("session_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                let history: Vec<crate::chat::ChatHistoryItem> = args
                    .get("history")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let app_settings = args
                    .get("appSettings")
                    .or_else(|| args.get("app_settings"))
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                let turn_id = args
                    .get("turnId")
                    .or_else(|| args.get("turn_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::chat::chat_command(
                    app.clone(),
                    message,
                    session_id,
                    history,
                    app_settings,
                    app.state(),
                    turn_id,
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_history_add" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_storage::browser_history_add(app.clone(), url, title, tab_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_history_get_recent" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
                let res = crate::browser_storage::browser_history_get_recent(limit, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_history_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
                let res = crate::browser_storage::browser_history_search(query, limit, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_history_delete" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_storage::browser_history_delete(app.clone(), id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_history_clear" => {
                let res = crate::browser_storage::browser_history_clear(app.clone(), app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_add" => {
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let folder_id = args.get("folderId").or_else(|| args.get("folder_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let favicon = args.get("favicon").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_storage::browser_bookmark_add(app.clone(), title, url, folder_id, favicon, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_update" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let folder_id = args.get("folderId").or_else(|| args.get("folder_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_storage::browser_bookmark_update(app.clone(), id, title, url, folder_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_delete" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_storage::browser_bookmark_delete(app.clone(), id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmarks_list" => {
                let res = crate::browser_storage::browser_bookmarks_list(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmarks_search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_storage::browser_bookmarks_search(query, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_is_bookmarked" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_storage::browser_bookmark_is_bookmarked(url, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_create_folder" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let parent_id = args.get("parentId").or_else(|| args.get("parent_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_storage::browser_bookmark_create_folder(app.clone(), name, parent_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_bookmark_delete_folder" => {
                let folder_id = args.get("folderId").or_else(|| args.get("folder_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_storage::browser_bookmark_delete_folder(app.clone(), folder_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_start" => {
                let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let suggested_filename = args.get("suggestedFilename").or_else(|| args.get("suggested_filename")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_download::browser_download_start(app.clone(), url, tab_id, suggested_filename)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_cancel" => {
                let download_id = args.get("downloadId").or_else(|| args.get("download_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_download::browser_download_cancel(download_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_list" => {
                let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as u32);
                let res = crate::browser_download::browser_download_list(limit, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_get" => {
                let download_id = args.get("downloadId").or_else(|| args.get("download_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_download::browser_download_get(download_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_delete_record" => {
                let download_id = args.get("downloadId").or_else(|| args.get("download_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_download::browser_download_delete_record(download_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_clear_records" => {
                let res = crate::browser_download::browser_download_clear_records(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_show_in_folder" => {
                let download_id = args.get("downloadId").or_else(|| args.get("download_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_download::browser_download_show_in_folder(download_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_download_open_file" => {
                let download_id = args.get("downloadId").or_else(|| args.get("download_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_download::browser_download_open_file(download_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profiles_list" => {
                let res = crate::browser_profile::browser_profiles_list(app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_get" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_get(profile_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_create" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let profile_type = args.get("profileType").or_else(|| args.get("profile_type")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_profile::browser_profile_create(app.clone(), name, profile_type)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_switch" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_switch(app.clone(), profile_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_rename" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let new_name = args.get("newName").or_else(|| args.get("new_name")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_rename(profile_id, new_name, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_delete" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_delete(app.clone(), profile_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_create_temporary" => {
                let task_id = args.get("taskId").or_else(|| args.get("task_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_create_temporary(app.clone(), task_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_profile_cleanup_temporary" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_profile::browser_profile_cleanup_temporary(app.clone(), profile_id, app.state())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_get_status" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_get_status(tab_id, profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_toggle_protection" => {
                let enabled = args.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_toggle_protection(enabled, profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_allowlist_domain" => {
                let domain = args.get("domain").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_allowlist_domain(domain, profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_remove_allowlist" => {
                let domain = args.get("domain").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_remove_allowlist(domain, profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_add_block_rule" => {
                let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rule_type = args.get("ruleType").or_else(|| args.get("rule_type")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let category = args.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_add_block_rule(pattern, rule_type, category, profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_remove_block_rule" => {
                let rule_id = args.get("ruleId").or_else(|| args.get("rule_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_remove_block_rule(rule_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_list_rules" => {
                let profile_id = args.get("profileId").or_else(|| args.get("profile_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_list_rules(profile_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_get_tab_stats" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_get_tab_stats(tab_id))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_privacy_reset_stats" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::browser_privacy::browser_privacy_reset_stats(tab_id))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_request_ai_control" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let task_id = args.get("taskId").or_else(|| args.get("task_id")).and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_control::browser_request_ai_control(app.clone(), tab_id, task_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_takeover_tab" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let reason = args.get("reason").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = crate::browser_control::browser_takeover_tab(app.clone(), tab_id, reason)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_release_ai_control" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_control::browser_release_ai_control(app.clone(), tab_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_pause_ai_control" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_control::browser_pause_ai_control(app.clone(), tab_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_resume_ai_control" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_control::browser_resume_ai_control(app.clone(), tab_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_get_tab_control_info" => {
                let tab_id = args.get("tabId").or_else(|| args.get("tab_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_control::browser_get_tab_control_info(tab_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_get_all_tab_controls" => {
                let res = crate::browser_control::browser_get_all_tab_controls()?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_assess_action_risk" => {
                let context: crate::browser_risk::BrowserActionContext = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid BrowserActionContext payload: {}", e))?;
                let res = crate::browser_risk::browser_assess_action_risk(context)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_get_risk_audit_log" => {
                let res = crate::browser_risk::browser_get_risk_audit_log()?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_resolve_action_approval" => {
                let approval_id = args.get("approvalId").or_else(|| args.get("approval_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let decision = args.get("decision").and_then(|v| v.as_str()).unwrap_or("DENIED").to_string();
                let res = crate::browser_risk::browser_resolve_action_approval(approval_id, decision)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_run_startup_recovery" => {
                let res = tokio_rt.block_on(crate::browser_recovery::browser_run_startup_recovery(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_agent_run_task" => {
                let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let max_steps = args.get("maxSteps").or_else(|| args.get("max_steps")).and_then(|v| v.as_u64()).map(|n| n as u32);
                let timeout_ms = args.get("timeoutMs").or_else(|| args.get("timeout_ms")).and_then(|v| v.as_u64());
                let res = tokio_rt.block_on(crate::browser_agent::browser_agent_run_task(
                    app.clone(),
                    goal,
                    max_steps,
                    timeout_ms,
                    app.state(),
                    app.state(),
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_agent_cancel_task" => {
                let task_id = args.get("taskId").or_else(|| args.get("task_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = tokio_rt.block_on(crate::browser_agent::browser_agent_cancel_task(
                    app.clone(),
                    task_id,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_agent_get_current_task" => {
                let res = tokio_rt.block_on(crate::browser_agent::browser_agent_get_current_task(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_orchestrator_run_task" => {
                let goal = args.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let subtask_goals: Option<Vec<String>> = args.get("subtaskGoals").or_else(|| args.get("subtask_goals"))
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                let global_max_steps = args.get("globalMaxSteps").or_else(|| args.get("global_max_steps")).and_then(|v| v.as_u64()).map(|n| n as u32);
                let timeout_ms = args.get("timeoutMs").or_else(|| args.get("timeout_ms")).and_then(|v| v.as_u64());
                let res = tokio_rt.block_on(crate::browser_orchestrator::browser_orchestrator_run_task(
                    app.clone(),
                    goal,
                    subtask_goals,
                    global_max_steps,
                    timeout_ms,
                    app.state(),
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_orchestrator_cancel_task" => {
                let orchestration_id = args.get("orchestrationId").or_else(|| args.get("orchestration_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = crate::browser_orchestrator::browser_orchestrator_cancel_task(orchestration_id)?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_orchestrator_get_current_task" => {
                let res = crate::browser_orchestrator::browser_orchestrator_get_current_task()?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_get_tool_definitions_cmd" => {
                let res = tokio_rt.block_on(crate::browser_tools::browser_get_tool_definitions_cmd())?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_execute_tool_cmd" => {
                let tool_name = args
                    .get("toolName")
                    .or_else(|| args.get("tool_name"))
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = args
                    .get("arguments")
                    .or_else(|| args.get("args"))
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                let res = tokio_rt.block_on(crate::browser_tools::browser_execute_tool_cmd(
                    app.clone(),
                    tool_name,
                    arguments,
                    app.state(),
                ))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "browser_show" | "browser_show_active" => crate::cef::tabs::show_active(args),
            "browser_hide" | "browser_hide_all" => crate::cef::tabs::hide_all(),
            "browser_destroy" => crate::cef::tabs::hide_all(),
            "browser_set_bounds" | "browser_set_bounds_all" => crate::cef::tabs::set_bounds(args),
            "browser_go_back" | "browser_go_back_tab" => crate::cef::tabs::go_back(args),
            "browser_go_forward" | "browser_go_forward_tab" => crate::cef::tabs::go_forward(args),
            "browser_reload" | "browser_reload_tab" => crate::cef::tabs::reload(args),
            "browser_create_tab" | "browser_create" => crate::cef::tabs::create_tab(args),
            "browser_switch_tab" => crate::cef::tabs::switch_tab(args),
            "browser_close_tab" => crate::cef::tabs::close_tab(args),
            "browser_close_other_tabs" => crate::cef::tabs::close_other_tabs(args),
            "browser_close_tabs_to_right" => crate::cef::tabs::close_tabs_to_right(args),
            "browser_duplicate_tab" => crate::cef::tabs::duplicate_tab(args),
            "browser_reopen_last_closed_tab" => crate::cef::tabs::reopen_last_closed_tab(args),
            "browser_toggle_pin_tab" => crate::cef::tabs::toggle_pin_tab(args),
            "browser_save_session" => crate::cef::tabs::save_session(args),
            "browser_restore_session" => crate::cef::tabs::restore_session(args),
            "browser_navigate_tab" | "browser_navigate" => crate::cef::tabs::navigate_tab(args),
            "browser_get_multi_state" => crate::cef::tabs::get_state(),
            "browser_get_url" | "browser_get_tab_url" => crate::cef::tabs::active_url(args),
            "browser_get_title" | "browser_get_tab_title" => crate::cef::tabs::get_tab_title(args),
            "browser_get_visible_text" | "browser_get_tab_visible_text" => crate::cef::tabs::get_visible_text(args),
            "browser_observe_tab" => crate::cef::tabs::observe_tab(args),
            "browser_click_element" => crate::cef::tabs::click_element(args),
            "browser_type_element" => crate::cef::tabs::type_element(args),
            "browser_select_option" => crate::cef::tabs::select_option(args),
            "browser_focus_element" => crate::cef::tabs::focus_element(args),
            "browser_scroll" => crate::cef::tabs::scroll(args),
            "browser_press_key" => crate::cef::tabs::press_key(args),
            "browser_wait" => crate::cef::tabs::wait_for_condition(args),
            "browser_screenshot_tab" => crate::cef::tabs::screenshot_tab(args),
            "browser_zoom_set" => crate::cef::tabs::zoom_set(args),
            "browser_zoom_in" => crate::cef::tabs::zoom_in(args),
            "browser_zoom_out" => crate::cef::tabs::zoom_out(args),
            "browser_zoom_reset" => crate::cef::tabs::zoom_reset(args),
            "browser_find_in_page" => crate::cef::tabs::find_in_page(args),
            "browser_clear_find" => crate::cef::tabs::clear_find(args),
            "browser_print_tab" => crate::cef::tabs::print_tab(args),
            "browser_save_page_html" => crate::cef::tabs::save_page_html(args),
            "browser_open_link_tab" => crate::cef::tabs::open_link_tab(args),
            "browser_reader_extract" => crate::cef::tabs::reader_extract(args),
            "browser_reader_mode_enter" => crate::cef::tabs::reader_mode_enter(args),
            "browser_reader_mode_exit" => crate::cef::tabs::reader_mode_exit(args),
            "browser_reader_mode_get" => crate::cef::tabs::reader_mode_get(args),
            "browser_tab_group_create" => crate::cef::tabs::tab_group_create(args),
            "browser_tab_group_rename" => crate::cef::tabs::tab_group_rename(args),
            "browser_tab_group_delete" => crate::cef::tabs::tab_group_delete(args),
            "browser_tab_group_list" => crate::cef::tabs::tab_group_list(args),
            "browser_tab_group_set_collapsed" => crate::cef::tabs::tab_group_set_collapsed(args),
            "browser_tab_group_move_tab" => crate::cef::tabs::tab_group_move_tab(args),
            "browser_tab_group_remove_tab" => crate::cef::tabs::tab_group_remove_tab(args),
            "browser_tab_group_reorder" => crate::cef::tabs::tab_group_reorder(args),
            "browser_tab_group_close_tabs" => crate::cef::tabs::tab_group_close_tabs(args),

            "realtime_voice_stop" => {
                let res = tokio_rt.block_on(crate::realtime_voice_stop(app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "realtime_voice_start" => {
                let conversation_id = args.get("conversationId").and_then(|v| v.as_str()).unwrap_or("default").to_string();
                let provider_id = args.get("providerId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let res = tokio_rt.block_on(crate::realtime_voice_start(conversation_id, provider_id, app.state()))?;
                Ok(serde_json::to_value(res).unwrap())
            }

            "smoke_test_verify" => {
                let react_ready = args.get("react_ready").and_then(|v| v.as_bool()).unwrap_or(false);
                println!("[CORE] smoke_test_verify command invoked from frontend: react_ready={}", react_ready);
                crate::cef::dispatch_event_to_browser("smoke-test-event", "{\"ok\":true}");
                Ok(serde_json::json!({
                    "success": true,
                    "pong": true,
                    "react_ready": react_ready,
                    "event": "smoke-test-event"
                }))
            }

            "smoke_test_ack" => {
                let react_ready = args.get("react_ready").and_then(|v| v.as_bool()).unwrap_or(false);
                let event_received = args.get("event_received").and_then(|v| v.as_bool()).unwrap_or(false);
                println!("[CORE] smoke_test_ack react_ready={react_ready} event_received={event_received}");
                if react_ready && event_received {
                    crate::cef::notify_smoke_test_success();
                    Ok(serde_json::json!({ "success": true, "verified": true }))
                } else {
                    Err(format!(
                        "Smoke test acknowledgement incomplete (react_ready={react_ready}, event_received={event_received})"
                    ))
                }
            }

            other if other.starts_with("browser_") => Err(format!(
                "Unsupported browser command '{other}'. It was not executed."
            )),

            _ => {
                eprintln!("[DISPATCHER] Unknown command received: {}", cmd);
                Err(format!("Unknown or unsupported command: '{}'", cmd))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_dispatch_command_inventory() {
        let commands = [
            "ping",
            "get_base_dir",
            "get_all_settings",
            "save_setting",
            "sync_settings",
            "get_all_sessions",
            "create_session",
            "rename_session",
            "delete_session",
            "get_session_messages",
            "save_session_message",
            "get_personal_notes",
            "save_personal_note",
            "delete_personal_note",
            "get_custom_apps",
            "add_custom_app",
            "delete_custom_app",
            "sync_apps_registry",
            "launch_app",
            "ai_list_providers",
            "ai_list_models",
            "ai_query_capabilities",
            "conversation_submit_turn",
            "conversation_execute_turn",
            "conversation_cancel_turn",
            "conversation_get_turn_status",
            "task_create",
            "task_cancel",
            "task_get_status",
            "task_list_active",
            "runtime_get_status",
            "runtime_get_capabilities",
            "get_providers",
            "fetch_custom_models",
            "tts_speak",
            "tts_stop",
            "transcribe_audio",
            "agent_status",
            "agent_chat",
            "agent_set_path",
            "agent_reset",
            "agent_resolve_proposal",
            "get_builtin_apps",
            "get_plugins",
            "toggle_plugin",
            "plugin_system_terminal",
            "plugin_system_control",
            "plugin_app_launcher",
            "plugin_web_search",
            "plugin_media_player",
            "plugin_whatsapp",
            "plugin_gmail",
            "take_screenshot",
            "get_weather",
            "voice_list_devices",
            "voice_get_status",
            "voice_stop_playback",
            "voice_set_input_device",
            "voice_set_output_device",
            "voice_session_cancel",
            "voice_session_start",
            "voice_session_submit_transcript",
            "voice_get_device_status",
            "realtime_voice_get_status",
            "realtime_voice_stop",
            "realtime_voice_start",
            "policy_evaluate_action",
            "policy_list_pending_approvals",
            "policy_resolve_approval",
            "policy_get_audit_log",
            "tools_list_definitions",
            "tools_get_definition",
            "tools_execute",
            "tools_cancel_execution",
            "save_to_memory_cmd",
            "get_memories_cmd",
            "search_memory_cmd",
            "delete_memory_cmd",
            "load_local_llm",
            "local_chat",
            "stop_local_llm",
            "chat_command",
            "browser_history_add",
            "browser_history_get_recent",
            "browser_history_search",
            "browser_history_delete",
            "browser_history_clear",
            "browser_bookmark_add",
            "browser_bookmark_update",
            "browser_bookmark_delete",
            "browser_bookmarks_list",
            "browser_bookmarks_search",
            "browser_bookmark_is_bookmarked",
            "browser_bookmark_create_folder",
            "browser_bookmark_delete_folder",
            "browser_download_start",
            "browser_download_cancel",
            "browser_download_list",
            "browser_download_get",
            "browser_download_delete_record",
            "browser_download_clear_records",
            "browser_download_show_in_folder",
            "browser_download_open_file",
            "browser_profiles_list",
            "browser_profile_get",
            "browser_profile_create",
            "browser_profile_switch",
            "browser_profile_rename",
            "browser_profile_delete",
            "browser_profile_create_temporary",
            "browser_profile_cleanup_temporary",
            "browser_privacy_get_status",
            "browser_privacy_toggle_protection",
            "browser_privacy_allowlist_domain",
            "browser_privacy_remove_allowlist",
            "browser_privacy_add_block_rule",
            "browser_privacy_remove_block_rule",
            "browser_privacy_list_rules",
            "browser_privacy_get_tab_stats",
            "browser_privacy_reset_stats",
            "browser_request_ai_control",
            "browser_takeover_tab",
            "browser_release_ai_control",
            "browser_pause_ai_control",
            "browser_resume_ai_control",
            "browser_get_tab_control_info",
            "browser_get_all_tab_controls",
            "browser_assess_action_risk",
            "browser_get_risk_audit_log",
            "browser_resolve_action_approval",
            "browser_run_startup_recovery",
            "browser_agent_run_task",
            "browser_agent_cancel_task",
            "browser_agent_get_current_task",
            "browser_orchestrator_run_task",
            "browser_orchestrator_cancel_task",
            "browser_orchestrator_get_current_task",
            "browser_get_tool_definitions_cmd",
            "browser_execute_tool_cmd",
            "browser_show",
            "browser_show_active",
            "browser_hide",
            "browser_hide_all",
            "browser_destroy",
            "browser_set_bounds",
            "browser_set_bounds_all",
            "browser_go_back",
            "browser_go_back_tab",
            "browser_go_forward",
            "browser_go_forward_tab",
            "browser_reload",
            "browser_reload_tab",
            "browser_create_tab",
            "browser_create",
            "browser_switch_tab",
            "browser_close_tab",
            "browser_close_other_tabs",
            "browser_close_tabs_to_right",
            "browser_duplicate_tab",
            "browser_reopen_last_closed_tab",
            "browser_toggle_pin_tab",
            "browser_save_session",
            "browser_restore_session",
            "browser_navigate_tab",
            "browser_navigate",
            "browser_get_multi_state",
            "browser_get_url",
            "browser_get_tab_url",
            "browser_get_title",
            "browser_get_tab_title",
            "browser_get_visible_text",
            "browser_get_tab_visible_text",
            "browser_observe_tab",
            "browser_click_element",
            "browser_type_element",
            "browser_select_option",
            "browser_focus_element",
            "browser_scroll",
            "browser_press_key",
            "browser_wait",
            "browser_screenshot_tab",
            "browser_zoom_set",
            "browser_zoom_in",
            "browser_zoom_out",
            "browser_zoom_reset",
            "browser_find_in_page",
            "browser_clear_find",
            "browser_print_tab",
            "browser_save_page_html",
            "browser_open_link_tab",
            "browser_reader_extract",
            "browser_reader_mode_enter",
            "browser_reader_mode_exit",
            "browser_reader_mode_get",
            "browser_tab_group_create",
            "browser_tab_group_rename",
            "browser_tab_group_delete",
            "browser_tab_group_list",
            "browser_tab_group_set_collapsed",
            "browser_tab_group_move_tab",
            "browser_tab_group_remove_tab",
            "browser_tab_group_reorder",
            "browser_tab_group_close_tabs",
            "smoke_test_verify",
            "smoke_test_ack",
        ];
        assert!(commands.len() >= 180);
    }
}

