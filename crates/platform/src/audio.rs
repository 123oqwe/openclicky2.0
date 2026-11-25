//! Microphone capture with cpal, resampled to 16 kHz mono PCM16.

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use openclicky_core::error::{Error, Permission, Result};
use openclicky_core::ports::{AudioClip, AudioSource};

struct Inner {
    recording: bool,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    stream: Option<cpal::Stream>,
}

pub struct CpalAudio {
    inner: Mutex<Inner>,
}

impl CpalAudio {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                recording: false,
                samples: Arc::new(Mutex::new(Vec::new())),
                sample_rate: 48_000,
                channels: 1,
                stream: None,
            }),
        }
    }

    fn clip(&self, inner: &Inner) -> AudioClip {
        let raw = inner.samples.lock().unwrap().clone();
        let mono = to_mono(&raw, inner.channels);
        AudioClip {
            pcm: resample(&mono, inner.sample_rate, 16_000),
            sample_rate: 16_000,
            channels: 1,
        }
    }
}

impl Default for CpalAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSource for CpalAudio {
    fn start(&self) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        if inner.recording {
            return Ok(());
        }
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(Error::NoPermission(Permission::Microphone))?;
        let config = device
            .default_input_config()
            .map_err(|_| Error::NoPermission(Permission::Microphone))?;
        inner.sample_rate = config.sample_rate();
        inner.channels = config.channels();
        let buffer = Arc::new(Mutex::new(Vec::new()));
        inner.samples = buffer.clone();
        let format = config.sample_format();
        let stream_config: cpal::StreamConfig = config.into();

        let write = buffer.clone();
        let err_fn = |e| eprintln!("audio: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _| write.lock().unwrap().extend_from_slice(data),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _| {
                    let mut b = write.lock().unwrap();
                    b.extend(data.iter().map(|s| *s as f32 / 32768.0));
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                stream_config,
                move |data: &[u16], _| {
                    let mut b = write.lock().unwrap();
                    b.extend(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0));
                },
                err_fn,
                None,
            ),
            _ => return Err(Error::Internal("不支持的采样格式".into())),
        }
        .map_err(|e| Error::Internal(format!("microphone: {e}")))?;

        stream
            .play()
            .map_err(|e| Error::Internal(format!("microphone: {e}")))?;
        inner.stream = Some(stream);
        inner.recording = true;
        Ok(())
    }

    fn stop(&self) -> Result<AudioClip> {
        let mut inner = self.inner.lock().unwrap();
        inner.recording = false;
        inner.stream = None;
        let clip = self.clip(&inner);
        inner.samples = Arc::new(Mutex::new(Vec::new()));
        Ok(clip)
    }

    fn snapshot(&self) -> Result<AudioClip> {
        let inner = self.inner.lock().unwrap();
        Ok(self.clip(&inner))
    }

    fn is_recording(&self) -> bool {
        self.inner.lock().unwrap().recording
    }
}

fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 || samples.is_empty() {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect()
}

fn resample(input: &[f32], from: u32, to: u32) -> Vec<i16> {
    if from == 0 || input.is_empty() {
        return Vec::new();
    }
    let ratio = to as f64 / from as f64;
    let out_len = (input.len() as f64 * ratio).max(1.0) as usize;
    let mut out: Vec<i16> = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input.get(idx).copied().unwrap_or(0.0);
        let b = input.get(idx + 1).copied().unwrap_or(a);
        let v = a + (b - a) * frac;
        out.push((v.clamp(-1.0, 1.0) * 32767.0) as i16);
    }
    out
}
