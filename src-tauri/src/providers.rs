use crate::ai::adapters::GeminiAdapter;
use crate::ai::provider::ModelDiscoveryCapability;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FetchedModel {
    pub id: String,
    pub label: String,
}

/// Returns the supported built-in providers with dynamic model catalogs (zero hardcoded models).
/// Models are discovered dynamically and persisted via SQLite settings.
#[tauri::command]
pub fn get_providers() -> Result<Value, String> {
    let provider_list = vec![
        json!({
            "id": "groq",
            "name": "Groq Cloud (Ultra-Fast)",
            "models": [],
        }),
        json!({
            "id": "gemini",
            "name": "Google Gemini API",
            "models": [],
        }),
        json!({
            "id": "huggingface",
            "name": "Hugging Face Inference",
            "models": [],
        }),
        json!({
            "id": "openrouter",
            "name": "OpenRouter AI",
            "models": [],
        }),
        json!({
            "id": "openai",
            "name": "OpenAI API",
            "models": [],
        }),
        json!({
            "id": "deepseek",
            "name": "DeepSeek AI",
            "models": [],
        }),
        json!({
            "id": "together",
            "name": "Together AI",
            "models": [],
        }),
        json!({
            "id": "ollama",
            "name": "Ollama Local",
            "models": [],
        }),
    ];

    Ok(json!({ "providers": provider_list }))
}

/// Dynamically discovers models from provider-specific endpoints including:
/// - Hugging Face (`https://huggingface.co/api/models`) with Bearer token authentication
/// - Google Gemini (`https://generativelanguage.googleapis.com/v1beta/models?key=...`)
/// - Ollama (`/api/tags` or `/v1/models`)
/// - Groq, OpenRouter, OpenAI, DeepSeek, Together, and generic OpenAI-compatible endpoints
#[tauri::command]
pub async fn fetch_custom_models(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<FetchedModel>, String> {
    let raw_url = base_url.trim().trim_end_matches('/').to_string();
    let creds = api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut result: Vec<FetchedModel> = Vec::new();

    // 1. Google Gemini API
    if raw_url.contains("generativelanguage.googleapis.com") || raw_url == "gemini" {
        let adapter = GeminiAdapter::new();
        let discovered = adapter
            .discover_models(&creds)
            .await
            .map_err(|e| e.to_string())?;

        for m in discovered {
            result.push(FetchedModel {
                id: m.model_id,
                label: m.display_name,
            });
        }
    }
    // 2. Hugging Face Inference API
    else if raw_url.contains("huggingface.co") || raw_url == "huggingface" {
        let hf_url = "https://huggingface.co/api/models?pipeline_tag=text-generation&sort=trending&limit=60";
        let mut req = client.get(hf_url);
        if let Some(ref key) = creds {
            req = req.bearer_auth(key);
        }
        let res = req.send().await.map_err(|e| {
            format!("Hugging Face network request failed: {}", e)
        })?;
        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            return Err(format!("Hugging Face returned status {}: {}", status, err_text));
        }
        let list: Value = res.json().await.map_err(|e| {
            format!("Failed to parse Hugging Face models JSON: {}", e)
        })?;

        if let Some(arr) = list.as_array() {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    result.push(FetchedModel {
                        id: id.to_string(),
                        label: id.to_string(),
                    });
                }
            }
        }
    }
    // 3. Ollama Local Endpoint
    else if raw_url.contains(":11434") || raw_url == "ollama" || raw_url == "local" {
        let host = if raw_url == "ollama" || raw_url == "local" {
            "http://localhost:11434"
        } else {
            &raw_url
        };
        let tags_url = format!("{}/api/tags", host.trim_end_matches('/'));
        let res = client.get(&tags_url).send().await;
        if let Ok(resp) = res {
            if resp.status().is_success() {
                if let Ok(val) = resp.json::<Value>().await {
                    if let Some(models) = val.get("models").and_then(|m| m.as_array()) {
                        for item in models {
                            let name = item
                                .get("name")
                                .or_else(|| item.get("model"))
                                .and_then(|n| n.as_str())
                                .unwrap_or("");
                            if !name.is_empty() {
                                result.push(FetchedModel {
                                    id: name.to_string(),
                                    label: name.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    // 4. OpenAI-compatible endpoints (Groq, OpenRouter, OpenAI, DeepSeek, Together, Custom)
    else {
        let mut target_endpoint = if raw_url == "groq" {
            "https://api.groq.com/openai/v1/models".to_string()
        } else if raw_url == "openrouter" {
            "https://openrouter.ai/api/v1/models".to_string()
        } else if raw_url == "openai" {
            "https://api.openai.com/v1/models".to_string()
        } else if raw_url == "deepseek" {
            "https://api.deepseek.com/models".to_string()
        } else if raw_url == "together" {
            "https://api.together.xyz/v1/models".to_string()
        } else if raw_url.ends_with("/models") {
            raw_url.clone()
        } else if raw_url.ends_with("/v1") {
            format!("{}/models", raw_url)
        } else {
            format!("{}/v1/models", raw_url)
        };

        let mut req = client.get(&target_endpoint);
        if let Some(ref key) = creds {
            req = req.bearer_auth(key);
        }

        let mut res = req.send().await;

        // If /v1/models failed with 404, fallback to /models directly
        if let Ok(ref resp) = res {
            if resp.status().as_u16() == 404 && target_endpoint.contains("/v1/models") {
                target_endpoint = target_endpoint.replace("/v1/models", "/models");
                let mut fallback_req = client.get(&target_endpoint);
                if let Some(ref key) = creds {
                    fallback_req = fallback_req.bearer_auth(key);
                }
                res = fallback_req.send().await;
            }
        }

        let response = res.map_err(|e| {
            format!("Provider models endpoint request failed ({}): {}", target_endpoint, e)
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            return Err(format!("Provider returned status {} from {}: {}", status, target_endpoint, err_text));
        }

        let val: Value = response.json().await.map_err(|e| {
            format!("Failed to parse models JSON: {}", e)
        })?;

        if let Some(data) = val.get("data").and_then(|d| d.as_array()) {
            for item in data {
                if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                    let label = item
                        .get("name")
                        .or_else(|| item.get("display_name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or(id);
                    result.push(FetchedModel {
                        id: id.to_string(),
                        label: label.to_string(),
                    });
                }
            }
        } else if let Some(models) = val.get("models").and_then(|m| m.as_array()) {
            for item in models {
                if let Some(id) = item.get("name").or_else(|| item.get("id")).and_then(|i| i.as_str()) {
                    result.push(FetchedModel {
                        id: id.to_string(),
                        label: id.to_string(),
                    });
                }
            }
        } else if let Some(arr) = val.as_array() {
            for item in arr {
                if let Some(id) = item.get("id").or_else(|| item.get("name")).and_then(|i| i.as_str()) {
                    result.push(FetchedModel {
                        id: id.to_string(),
                        label: id.to_string(),
                    });
                }
            }
        }
    }

    if result.is_empty() {
        return Err(
            "No models found in provider response. Check that your API key is correct and has access to models."
                .to_string(),
        );
    }

    result.sort_by(|a, b| a.label.cmp(&b.label));
    result.dedup_by(|a, b| a.id == b.id);
    Ok(result)
}
