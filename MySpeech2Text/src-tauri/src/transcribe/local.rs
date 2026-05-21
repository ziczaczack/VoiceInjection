use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const TARGET_RATE: u32 = 16_000;

struct LoadedModel {
    name: String,
    path: PathBuf,
    ctx: WhisperContext,
}

static MODEL: Mutex<Option<LoadedModel>> = Mutex::new(None);

pub struct LocalRequest<'a> {
    pub model_name: &'a str,
    pub model_path: &'a Path,
    pub samples: &'a [i16],
    pub sample_rate: u32,
    pub language: Option<&'a str>,
    pub initial_prompt: Option<&'a str>,
}

pub fn transcribe(req: LocalRequest<'_>) -> Result<String> {
    let mono_f32 = i16_to_f32(req.samples);
    let resampled = resample_to_16k(&mono_f32, req.sample_rate);

    let mut guard = MODEL.lock().map_err(|_| anyhow!("model lock poisoned"))?;
    let need_reload = match guard.as_ref() {
        Some(m) => m.name != req.model_name || m.path != req.model_path,
        None => true,
    };
    if need_reload {
        log::info!("loading whisper model: {}", req.model_name);
        *guard = None;
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        let path_str = req
            .model_path
            .to_str()
            .ok_or_else(|| anyhow!("model path is not valid UTF-8: {:?}", req.model_path))?;
        let ctx = WhisperContext::new_with_params(path_str, params)
            .with_context(|| format!("loading model from {path_str}"))?;
        *guard = Some(LoadedModel {
            name: req.model_name.to_string(),
            path: req.model_path.to_path_buf(),
            ctx,
        });
    }

    let loaded = guard.as_ref().expect("just loaded");
    let mut state = loaded
        .ctx
        .create_state()
        .context("whisper create_state failed")?;

    let mut full = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    full.set_n_threads(num_threads());
    full.set_translate(false);
    full.set_print_special(false);
    full.set_print_progress(false);
    full.set_print_realtime(false);
    full.set_print_timestamps(false);
    full.set_no_context(true);
    full.set_single_segment(false);
    full.set_suppress_blank(true);
    if let Some(lang) = req.language {
        if lang != "auto" && !lang.is_empty() {
            full.set_language(Some(lang));
        }
    }
    if let Some(p) = req.initial_prompt {
        if !p.is_empty() {
            full.set_initial_prompt(p);
        }
    }

    state
        .full(full, &resampled)
        .context("whisper inference failed")?;

    let n = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n {
        let seg = state
            .get_segment(i)
            .ok_or_else(|| anyhow!("segment {i} missing"))?;
        let s = seg.to_str_lossy().context("segment text")?;
        text.push_str(&s);
    }
    Ok(text.trim().to_string())
}

pub fn unload() {
    if let Ok(mut g) = MODEL.lock() {
        *g = None;
    }
}

fn num_threads() -> i32 {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    n.clamp(1, 8) as i32
}

fn i16_to_f32(input: &[i16]) -> Vec<f32> {
    const SCALE: f32 = 1.0 / 32768.0;
    input.iter().map(|&s| s as f32 * SCALE).collect()
}

fn resample_to_16k(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == TARGET_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / TARGET_RATE as f64;
    let out_len = (samples.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last_idx = samples.len() - 1;
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let lo = src.floor() as usize;
        let hi = (lo + 1).min(last_idx);
        let frac = (src - lo as f64) as f32;
        out.push(samples[lo] * (1.0 - frac) + samples[hi] * frac);
    }
    out
}
