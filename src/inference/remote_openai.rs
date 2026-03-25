//! OpenAI-compatible Chat Completions client (OpenAI, Groq, Together, local vLLM, etc.).

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<Message<'a>>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

pub struct RemoteGenerateResult {
    pub text: String,
    pub tokens_generated: u32,
}

fn chat_completions_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.to_string()
    } else if b.ends_with("/v1") {
        format!("{b}/chat/completions")
    } else {
        format!("{b}/v1/chat/completions")
    }
}

/// Single-turn chat: `user_content` is the full prompt string (same shape as Ollama path).
pub async fn chat_completion(
    base_url: &str,
    api_key: &str,
    model: &str,
    user_content: &str,
    max_tokens: u32,
    temperature: f32,
) -> Result<RemoteGenerateResult, String> {
    let url = chat_completions_url(base_url);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let body = ChatRequest {
        model,
        messages: vec![Message {
            role: "user",
            content: user_content,
        }],
        max_tokens,
        temperature,
    };

    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("remote API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let t = resp.text().await.unwrap_or_default();
        return Err(format!("remote API {status}: {t}"));
    }

    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("invalid JSON from remote API: {e}"))?;

    let text = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();

    let tokens = parsed
        .usage
        .map(|u| {
            if u.completion_tokens > 0 {
                u.completion_tokens
            } else {
                u.total_tokens
            }
        })
        .unwrap_or(0);

    Ok(RemoteGenerateResult {
        text,
        tokens_generated: tokens,
    })
}
