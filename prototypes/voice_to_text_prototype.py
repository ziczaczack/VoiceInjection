"""Stage 0 validator: record short clips, send to Groq Whisper, compare models.

Usage:
    set GROQ_API_KEY=gsk_...
    python voice_to_text_prototype.py              # run all 3 scenarios
    python voice_to_text_prototype.py --rerun      # re-transcribe existing samples without recording
    python voice_to_text_prototype.py --seconds 12 # change recording length
"""

from __future__ import annotations

import argparse
import os
import sys
import time
import wave
from pathlib import Path

import numpy as np
import requests
import sounddevice as sd

API_URL = "https://api.groq.com/openai/v1/audio/transcriptions"
MODELS = ["whisper-large-v3", "whisper-large-v3-turbo"]
SAMPLE_RATE = 16_000
CHANNELS = 1

SCENARIOS = [
    ("01_chinese", "Pure Chinese conversational",
        "e.g. 今天天气不错，我想出去走走，顺便买点东西回来"),
    ("02_zh_en", "Chinese + English with technical terms",
        "e.g. 我等下要 commit 然后 deploy 到 production，记得先 review 一下 pull request"),
    ("03_zh_en_long", "Chinese + English, longer mixed sentence",
        "e.g. 这个 function 的 performance 不太行，我打算 refactor 一下，把 cache 加进去"),
]


def record(seconds: float) -> np.ndarray:
    print(f"  Recording for {seconds}s — speak now...")
    audio = sd.rec(
        int(seconds * SAMPLE_RATE),
        samplerate=SAMPLE_RATE,
        channels=CHANNELS,
        dtype="int16",
    )
    sd.wait()
    return audio


def save_wav(audio: np.ndarray, path: Path) -> None:
    with wave.open(str(path), "wb") as wf:
        wf.setnchannels(CHANNELS)
        wf.setsampwidth(2)
        wf.setframerate(SAMPLE_RATE)
        wf.writeframes(audio.tobytes())


def transcribe(wav_path: Path, model: str, api_key: str) -> tuple[str, float]:
    headers = {"Authorization": f"Bearer {api_key}"}
    data = {"model": model, "temperature": "0.0", "response_format": "json"}
    with open(wav_path, "rb") as f:
        files = {"file": (wav_path.name, f, "audio/wav")}
        t0 = time.time()
        r = requests.post(API_URL, headers=headers, data=data, files=files, timeout=60)
    elapsed = time.time() - t0
    if not r.ok:
        raise RuntimeError(f"HTTP {r.status_code}: {r.text}")
    return r.json().get("text", "").strip(), elapsed


def run_scenario(name: str, wav: Path, api_key: str) -> None:
    print(f"\n  Transcribing {wav.name}...")
    for model in MODELS:
        try:
            text, elapsed = transcribe(wav, model, api_key)
            print(f"    [{model:28s}] {elapsed:5.2f}s")
            print(f"      {text}")
        except Exception as e:
            print(f"    [{model:28s}] ERROR: {e}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rerun", action="store_true",
                        help="Skip recording, re-transcribe existing samples")
    parser.add_argument("--seconds", type=float, default=8.0,
                        help="Recording length per scenario (default 8s)")
    args = parser.parse_args()

    api_key = os.environ.get("GROQ_API_KEY")
    if not api_key:
        print("ERROR: set GROQ_API_KEY environment variable first.")
        print("  PowerShell:  $env:GROQ_API_KEY = 'gsk_...'")
        return 1

    samples_dir = Path(__file__).parent / "samples"
    samples_dir.mkdir(exist_ok=True)

    print("Stage 0 — Whisper bilingual (Chinese + English) validator")
    print(f"Models under test: {', '.join(MODELS)}")
    print(f"Samples dir: {samples_dir}")

    for name, title, hint in SCENARIOS:
        wav = samples_dir / f"{name}.wav"
        print(f"\n=== {title} ===")
        print(f"  Hint: {hint}")

        if args.rerun:
            if not wav.exists():
                print(f"  SKIP: {wav.name} not found (run without --rerun first)")
                continue
        else:
            input("  Press Enter when ready to record... ")
            audio = record(args.seconds)
            save_wav(audio, wav)
            print(f"  Saved {wav.name}")

        run_scenario(name, wav, api_key)

    print("\n" + "=" * 60)
    print("Decision time:")
    print("  >90% accuracy across all 3   → proceed to Stage 1")
    print("  Tech terms mistranscribed    → still proceed; note words for dictionary")
    print("  Code-switching breaks output → reconsider scope")
    print("=" * 60)
    return 0


if __name__ == "__main__":
    sys.exit(main())
