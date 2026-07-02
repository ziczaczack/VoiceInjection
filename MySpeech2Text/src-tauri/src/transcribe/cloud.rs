use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const GROQ_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub struct GroqRequest<'a> {
    pub api_key: &'a str,
    pub model: &'a str,
    pub language: Option<&'a str>,
    pub initial_prompt: Option<&'a str>,
    pub wav_bytes: Vec<u8>,
}

const MAX_RETRIES: u32 = 3;

pub async fn transcribe(req: GroqRequest<'_>) -> Result<String> {
    if req.api_key.trim().is_empty() {
        return Err(anyhow!("no Groq API key set — paste one in Settings first"));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let mut attempt: u32 = 0;
    loop {
        // The multipart Form is consumed by `send()`, so rebuild it (cloning the
        // WAV bytes) on every attempt.
        let part = Part::bytes(req.wav_bytes.clone())
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let mut form = Form::new()
            .text("model", req.model.to_string())
            .text("temperature", "0.0")
            .text("response_format", "json")
            .part("file", part);
        if let Some(lang) = req.language {
            if lang != "auto" && !lang.is_empty() {
                form = form.text("language", lang.to_string());
            }
        }
        if let Some(p) = req.initial_prompt {
            if !p.is_empty() {
                form = form.text("prompt", p.to_string());
            }
        }

        let resp = client
            .post(GROQ_URL)
            .bearer_auth(req.api_key)
            .multipart(form)
            .send()
            .await
            .context("Groq request failed")?;

        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if attempt >= MAX_RETRIES {
                return Err(anyhow!(
                    "Groq rate limit hit (429) after {MAX_RETRIES} retries. Wait a bit and try again."
                ));
            }
            // Honor Retry-After (seconds) if present, else exponential backoff:
            // 0.5s, 1s, 2s.
            let wait = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(std::time::Duration::from_secs_f64)
                .unwrap_or_else(|| std::time::Duration::from_millis(500u64 << attempt));
            log::warn!(
                "Groq 429 — retrying in {:.1}s (attempt {}/{})",
                wait.as_secs_f64(),
                attempt + 1,
                MAX_RETRIES
            );
            tokio::time::sleep(wait).await;
            attempt += 1;
            continue;
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Groq HTTP {status}: {body}"));
        }

        let parsed: TranscriptionResponse =
            resp.json().await.context("Groq returned invalid JSON")?;
        return Ok(parsed.text.trim().to_string());
    }
}
