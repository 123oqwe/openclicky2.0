//! System text-to-speech. Always available, never needs a key.

use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use openclicky_core::error::{Error, Result};
use openclicky_core::ports::SpeechHandle;

pub struct OsSpeech {
    speed: f32,
    voice: String,
}

impl OsSpeech {
    pub fn new(speed: f32, voice: String) -> Self {
        Self { speed, voice }
    }

    pub fn spawn(&self, text: &str) -> Result<Child> {
        #[cfg(target_os = "macos")]
        {
            let rate = (220.0 * self.speed.clamp(0.5, 1.5)) as u32;
            let mut cmd = Command::new("say");
            cmd.arg("-r").arg(rate.to_string());
            if self.voice_usable() {
                cmd.arg("-v").arg(&self.voice);
            }
            cmd.arg("--").arg(text);
            cmd.stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| Error::Internal(e.to_string()))
        }
        #[cfg(target_os = "windows")]
        {
            let script = format!(
                "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate = {}; $s.Speak([Console]::In.ReadToEnd())",
                (((self.speed - 1.0) * 5.0) as i32).clamp(-5, 5)
            );
            let mut child = Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| Error::Internal(e.to_string()))?;
            if let Some(stdin) = child.stdin.as_mut() {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            Ok(child)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let program = if which("spd-say") {
                "spd-say"
            } else {
                "espeak-ng"
            };
            Command::new(program)
                .arg(text)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| Error::Internal(e.to_string()))
        }
    }

    fn voice_usable(&self) -> bool {
        let v = self.voice.trim();
        !v.is_empty() && !v.contains("alloy") && !v.contains("nova") && !v.contains("shimmer")
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn which(bin: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub struct ChildHandle {
    child: Mutex<Option<Child>>,
}

impl ChildHandle {
    pub fn new(child: Child) -> Self {
        Self {
            child: Mutex::new(Some(child)),
        }
    }
}

impl SpeechHandle for ChildHandle {
    fn cancel(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn wait(&self) {
        let taken = self.child.lock().unwrap().take();
        if let Some(mut child) = taken {
            let _ = child.wait();
        }
    }
}
