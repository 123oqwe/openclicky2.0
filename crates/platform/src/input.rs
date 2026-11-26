//! Text injection: type directly when possible, otherwise paste and restore the
//! clipboard (ADR-0004).

use std::process::Command;

use enigo::{Enigo, Keyboard, Settings};
use openclicky_core::error::{Error, Result};
use openclicky_core::ports::{InjectOutcome, TargetInfo, TargetKind, TextInjector};

const TERMINALS: &[&str] = &[
    "terminal",
    "iterm",
    "warp",
    "alacritty",
    "kitty",
    "wezterm",
    "powershell",
    "cmd",
    "windowsterminal",
    "konsole",
    "gnome-terminal",
    "xterm",
    "hyper",
    "tabby",
];
const EDITORS: &[&str] = &[
    "code", "cursor", "xcode", "sublime", "vim", "nvim", "emacs", "zed",
];

pub fn classify(app: &str) -> TargetKind {
    let a = app.to_lowercase();
    if TERMINALS.iter().any(|t| a.contains(t)) {
        TargetKind::Terminal
    } else if EDITORS.iter().any(|t| a.contains(t)) {
        TargetKind::Editor
    } else if a.contains("chrome")
        || a.contains("safari")
        || a.contains("firefox")
        || a.contains("edge")
        || a.contains("arc")
    {
        TargetKind::Browser
    } else if a.trim().is_empty() {
        TargetKind::Unknown
    } else {
        TargetKind::Other
    }
}

pub struct PlatformInjector;

impl PlatformInjector {
    pub fn new() -> Self {
        Self
    }

    fn frontmost_app(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            let out = Command::new("osascript")
                .args(["-e", "tell application \"System Events\" to get name of first application process whose frontmost is true"])
                .output()
                .ok()?;
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!name.is_empty()).then_some(name)
        }
        #[cfg(target_os = "windows")]
        {
            let script = "Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"user32.dll\")] public static extern System.IntPtr GetForegroundWindow(); [DllImport(\"user32.dll\")] public static extern uint GetWindowThreadProcessId(System.IntPtr h, out uint p);'; $h=[W.U]::GetForegroundWindow(); $p=0; [void][W.U]::GetWindowThreadProcessId($h,[ref]$p); (Get-Process -Id $p).ProcessName";
            let out = Command::new("powershell")
                .args(["-NoProfile", "-Command", script])
                .output()
                .ok()?;
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!name.is_empty()).then_some(name)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let out = Command::new("xdotool")
                .arg("getactivewindow")
                .arg("getwindowclassname")
                .output()
                .ok()?;
            let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!name.is_empty()).then_some(name)
        }
    }

    fn paste(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            Command::new("osascript")
                .args([
                    "-e",
                    "tell application \"System Events\" to keystroke \"v\" using command down",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        #[cfg(target_os = "windows")]
        {
            Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    "$wshell = New-Object -ComObject wscript.shell; $wshell.SendKeys('^v')",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("xdotool")
                .args(["key", "--clearmodifiers", "ctrl+v"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
    }
}

impl Default for PlatformInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInjector for PlatformInjector {
    fn focused(&self) -> Option<TargetInfo> {
        let app = self.frontmost_app()?;
        let kind = classify(&app);
        Some(TargetInfo { app, kind })
    }

    fn inject(&self, text: &str, target: &TargetInfo) -> Result<InjectOutcome> {
        if text.is_empty() {
            return Ok(InjectOutcome::Typed);
        }
        match openclicky_core::text::plan_injection(text, target.kind, true) {
            openclicky_core::text::Injection::Type(payload) => {
                if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
                    if enigo.text(&payload).is_ok() {
                        return Ok(InjectOutcome::Typed);
                    }
                }
                self.paste_text(&payload)
            }
            // A terminal never gets keystrokes, even as a fallback: the paste
            // keystroke does not submit a line, a typed newline does.
            openclicky_core::text::Injection::Paste(payload) => self.paste_text(&payload),
            openclicky_core::text::Injection::CopyOnly(payload) => self.copy_only(&payload),
        }
    }

    fn copy(&self, text: &str) -> Result<InjectOutcome> {
        self.copy_only(&openclicky_core::text::terminal_safe(text))
    }
}

impl PlatformInjector {
    fn copy_only(&self, text: &str) -> Result<InjectOutcome> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| Error::Internal(e.to_string()))?;
        clipboard
            .set_text(text.to_string())
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(InjectOutcome::CopiedOnly)
    }

    fn paste_text(&self, text: &str) -> Result<InjectOutcome> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| Error::Internal(e.to_string()))?;
        let previous = clipboard.get_text().ok();
        clipboard
            .set_text(text.to_string())
            .map_err(|e| Error::Internal(e.to_string()))?;
        let pasted = self.paste();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let restored = match previous {
            Some(prev) => clipboard.set_text(prev).is_ok(),
            None => clipboard.clear().is_ok(),
        };
        if pasted {
            Ok(InjectOutcome::Pasted {
                clipboard_restored: restored,
            })
        } else {
            Ok(InjectOutcome::CopiedOnly)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_terminals_and_editors() {
        assert_eq!(classify("iTerm2"), TargetKind::Terminal);
        assert_eq!(classify("Visual Studio Code"), TargetKind::Editor);
        assert_eq!(classify("Safari"), TargetKind::Browser);
        assert_eq!(classify("Final Cut Pro"), TargetKind::Other);
        assert_eq!(classify(""), TargetKind::Unknown);
    }
}
