use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};

#[derive(Debug)]
pub struct RecordedAudio {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

enum Cmd {
    Start,
    Stop,
}

#[derive(Debug)]
enum Reply {
    Started,
    Stopped(RecordedAudio),
    Error(String),
}

pub struct Recorder {
    cmd_tx: mpsc::Sender<Cmd>,
    reply_rx: std::sync::Mutex<mpsc::Receiver<Reply>>,
    running: Arc<AtomicBool>,
}

impl Recorder {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (reply_tx, reply_rx) = mpsc::channel();

        thread::Builder::new()
            .name("audio-recorder".into())
            .spawn(move || run_audio_thread(cmd_rx, reply_tx))
            .expect("failed to spawn audio thread");

        Self {
            cmd_tx,
            reply_rx: std::sync::Mutex::new(reply_rx),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> Result<()> {
        self.cmd_tx
            .send(Cmd::Start)
            .map_err(|_| anyhow!("audio thread is gone"))?;
        let guard = self
            .reply_rx
            .lock()
            .map_err(|_| anyhow!("recorder reply lock poisoned"))?;
        match guard.recv() {
            Ok(Reply::Started) => {
                self.running.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ok(Reply::Error(e)) => Err(anyhow!(e)),
            Ok(other) => Err(anyhow!("unexpected reply: {other:?}")),
            Err(_) => Err(anyhow!("audio thread closed")),
        }
    }

    pub fn stop(&self) -> Result<RecordedAudio> {
        self.cmd_tx
            .send(Cmd::Stop)
            .map_err(|_| anyhow!("audio thread is gone"))?;
        let guard = self
            .reply_rx
            .lock()
            .map_err(|_| anyhow!("recorder reply lock poisoned"))?;
        let result = match guard.recv() {
            Ok(Reply::Stopped(audio)) => Ok(audio),
            Ok(Reply::Error(e)) => Err(anyhow!(e)),
            Ok(other) => Err(anyhow!("unexpected reply: {other:?}")),
            Err(_) => Err(anyhow!("audio thread closed")),
        };
        self.running.store(false, Ordering::SeqCst);
        result
    }
}

fn run_audio_thread(cmd_rx: mpsc::Receiver<Cmd>, reply_tx: mpsc::Sender<Reply>) {
    let mut active: Option<ActiveStream> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Start => {
                if active.is_some() {
                    let _ = reply_tx.send(Reply::Error("already recording".into()));
                    continue;
                }
                match start_stream() {
                    Ok(s) => {
                        active = Some(s);
                        let _ = reply_tx.send(Reply::Started);
                    }
                    Err(e) => {
                        let _ = reply_tx.send(Reply::Error(format!("{e:#}")));
                    }
                }
            }
            Cmd::Stop => {
                let s = match active.take() {
                    Some(s) => s,
                    None => {
                        let _ = reply_tx.send(Reply::Error("not recording".into()));
                        continue;
                    }
                };
                let ActiveStream {
                    stream,
                    sample_rate,
                    channels,
                    samples_rx,
                } = s;
                drop(stream);
                let mut interleaved: Vec<i16> = Vec::new();
                while let Ok(mut chunk) = samples_rx.try_recv() {
                    interleaved.append(&mut chunk);
                }
                let mono = downmix_to_mono(&interleaved, channels);
                let _ = reply_tx.send(Reply::Stopped(RecordedAudio {
                    samples: mono,
                    sample_rate,
                }));
            }
        }
    }
}

struct ActiveStream {
    stream: cpal::Stream,
    sample_rate: u32,
    channels: u16,
    samples_rx: mpsc::Receiver<Vec<i16>>,
}

fn start_stream() -> Result<ActiveStream> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device — check microphone permissions")?;
    let supported = device
        .default_input_config()
        .context("failed to query default input config")?;

    let sample_format = supported.sample_format();
    let channels = supported.channels();
    let sample_rate = supported.sample_rate().0;
    let config = StreamConfig {
        channels,
        sample_rate: SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (samples_tx, samples_rx) = mpsc::channel::<Vec<i16>>();
    let err_fn = |err| log::error!("audio stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            {
                let tx = samples_tx.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let buf: Vec<i16> = data
                        .iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();
                    let _ = tx.send(buf);
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            {
                let tx = samples_tx.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                }
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config,
            {
                let tx = samples_tx.clone();
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let buf: Vec<i16> = data.iter().map(|&s| (s as i32 - 32768) as i16).collect();
                    let _ = tx.send(buf);
                }
            },
            err_fn,
            None,
        )?,
        other => return Err(anyhow!("unsupported sample format: {other:?}")),
    };

    stream.play()?;
    Ok(ActiveStream {
        stream,
        sample_rate,
        channels,
        samples_rx,
    })
}

fn downmix_to_mono(interleaved: &[i16], channels: u16) -> Vec<i16> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    let ch = channels as usize;
    interleaved
        .chunks_exact(ch)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / ch as i32) as i16
        })
        .collect()
}

pub fn write_wav(audio: &RecordedAudio) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf: Vec<u8> = Vec::with_capacity(audio.samples.len() * 2 + 44);
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut writer = hound::WavWriter::new(cursor, spec)?;
        for &s in &audio.samples {
            writer.write_sample(s)?;
        }
        writer.finalize()?;
    }
    Ok(buf)
}

pub fn duration_secs(audio: &RecordedAudio) -> f32 {
    if audio.sample_rate == 0 {
        return 0.0;
    }
    audio.samples.len() as f32 / audio.sample_rate as f32
}

pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    (sum / samples.len() as f64).sqrt() as f32 / i16::MAX as f32
}
