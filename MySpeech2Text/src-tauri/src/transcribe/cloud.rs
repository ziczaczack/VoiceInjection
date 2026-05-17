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
    pub wav_bytes: Vec<u8>,
}

pub async fn transcribe(req: GroqRequest<'_>) -> Result<String> {
    if req.api_key.trim().is_empty() {
        return Err(anyhow!(
            "no Groq API key set — paste one in Settings first"
        ));
    }

    let part = Part::bytes(req.wav_bytes)
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let resp = client
        .post(GROQ_URL)
        .bearer_auth(req.api_key)
        .multipart(form)
        .send()
        .await
        .context("Groq request failed")?;

    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(anyhow!(
            "Groq rate limit hit (429). Wait a bit and try again."
        ));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("Groq HTTP {status}: {body}"));
    }

    let parsed: TranscriptionResponse =
        resp.json().await.context("Groq returned invalid JSON")?;
    Ok(parsed.text.trim().to_string())
}
