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
            "browser_navigate_tab" | "browser_navigate" => crate::cef::tabs::navigate_tab(args),
            "browser_get_multi_state" => crate::cef::tabs::get_state(),
            "browser_get_url" | "browser_get_tab_url" => crate::cef::tabs::active_url(args),
            "browser_click_element" => {
                let element_id = args.get("elementId").and_then(|v| v.as_str()).unwrap_or("");
                let escaped = element_id.replace('\\', "\\\\").replace('\'', "\\'");
                let script = format!(
                    "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped}\"]')||document.getElementById('{escaped}');if(!t)return;t.scrollIntoView({{block:'center'}});t.focus();t.click();}})();"
                );
                crate::cef::tabs::run_script(args, script)
            }
            "browser_type_element" => {
                let element_id = args.get("elementId").and_then(|v| v.as_str()).unwrap_or("");
                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let escaped_id = element_id.replace('\\', "\\\\").replace('\'', "\\'");
                let escaped_text = text.replace('\\', "\\\\").replace('\'', "\\'");
                let script = format!(
                    "(function(){{var t=document.querySelector('[data-joren-eid=\"{escaped_id}\"]')||document.getElementById('{escaped_id}');if(!t)return;t.focus();t.value='{escaped_text}';t.dispatchEvent(new Event('input',{{bubbles:true}}));}})();"
                );
                crate::cef::tabs::run_script(args, script)
            }
            "browser_scroll" => {
                let dy = args.get("deltaY").and_then(|v| v.as_f64()).unwrap_or(400.0) as i32;
                crate::cef::tabs::run_script(args, format!("window.scrollBy(0,{dy});"))
            }
            "browser_press_key" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("Enter");
                let escaped = key.replace('\\', "\\\\").replace('\'', "\\'");
                crate::cef::tabs::run_script(
                    args,
                    format!("document.activeElement&&document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{{key:'{escaped}',bubbles:true}}));"),
                )
            }
            "browser_focus_element" => {
                let element_id = args.get("elementId").and_then(|v| v.as_str()).unwrap_or("");
                let escaped = element_id.replace('\\', "\\\\").replace('\'', "\\'");
                crate::cef::tabs::run_script(
                    args,
                    format!("(function(){{var t=document.getElementById('{escaped}');if(t)t.focus();}})();"),
                )
            }

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

