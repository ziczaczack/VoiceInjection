use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use arboard::Clipboard;
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};

const LONG_TEXT_THRESHOLD: usize = 500;

pub fn inject(text: &str, strategy: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let force_clipboard = text.chars().count() > LONG_TEXT_THRESHOLD;
    let use_clipboard = force_clipboard || strategy != "keystroke";
    if use_clipboard {
        inject_via_clipboard(text)
    } else {
        inject_via_keystroke(text)
    }
}

fn inject_via_clipboard(text: &str) -> Result<()> {
    let mut cb = Clipboard::new().context("failed to open clipboard")?;
    let previous = cb.get_text().ok();

    cb.set_text(text.to_string())
        .context("failed to set clipboard")?;

    thread::sleep(Duration::from_millis(50));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow!("enigo init failed: {e}"))?;
    enigo
        .key(Key::Control, Press)
        .map_err(|e| anyhow!("ctrl press failed: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Click)
        .map_err(|e| anyhow!("v click failed: {e}"))?;
    enigo
        .key(Key::Control, Release)
        .map_err(|e| anyhow!("ctrl release failed: {e}"))?;

    if let Some(prev) = previous {
        thread::sleep(Duration::from_millis(200));
        let _ = cb.set_text(prev);
    }
    Ok(())
}

fn inject_via_keystroke(text: &str) -> Result<()> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow!("enigo init failed: {e}"))?;
    enigo
        .text(text)
        .map_err(|e| anyhow!("text input failed: {e}"))?;
    Ok(())
}
