//! Shortcut parsing and normalisation (macOS / Windows / Linux differ in names).

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Control,
    Alt,
    Shift,
    Meta,
    Fn,
}

impl Modifier {
    pub fn as_str(self) -> &'static str {
        match self {
            Modifier::Control => "control",
            Modifier::Alt => "alt",
            Modifier::Shift => "shift",
            Modifier::Meta => "meta",
            Modifier::Fn => "fn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Hold,
    DoubleTap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub modifiers: Vec<Modifier>,
    /// Single character or named key ("a", "control", "fn", "space").
    pub key: String,
    pub gesture: Gesture,
}

impl Binding {
    pub fn parse(spec: &str) -> Result<Self> {
        let spec = spec.trim().to_ascii_lowercase();
        if spec.is_empty() {
            return Err(Error::BadRequest("快捷键不能为空".into()));
        }
        let (gesture, body) = if let Some(rest) = spec.strip_prefix("double:") {
            (Gesture::DoubleTap, rest)
        } else {
            (Gesture::Hold, spec.as_str())
        };
        let parts: Vec<&str> = body
            .split('+')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.is_empty() {
            return Err(Error::BadRequest("快捷键缺少按键".into()));
        }
        let modifier_of = |p: &str| -> Option<Modifier> {
            match p {
                "control" | "ctrl" | "ctl" => Some(Modifier::Control),
                "alt" | "option" | "opt" => Some(Modifier::Alt),
                "shift" => Some(Modifier::Shift),
                "meta" | "cmd" | "command" | "win" | "super" => Some(Modifier::Meta),
                "fn" => Some(Modifier::Fn),
                _ => None,
            }
        };
        let mut modifiers: Vec<Modifier> = Vec::new();
        let mut key = String::new();
        for (i, part) in parts.iter().enumerate() {
            match modifier_of(part) {
                Some(m) => {
                    if !modifiers.contains(&m) {
                        modifiers.push(m);
                    }
                }
                None => {
                    if i != parts.len() - 1 {
                        return Err(Error::BadRequest(format!("无法识别的修饰键：{part}")));
                    }
                    key = (*part).to_string();
                }
            }
        }
        if modifiers.is_empty() && key.is_empty() {
            return Err(Error::BadRequest("快捷键缺少按键".into()));
        }
        // A lone modifier is the trigger key (e.g. "fn").
        if modifiers.len() == 1 && key.is_empty() {
            key = modifiers[0].as_str().to_string();
        }
        Ok(Self {
            modifiers,
            key,
            gesture,
        })
    }

    /// Human readable, platform-neutral form.
    pub fn display(&self) -> String {
        let body = self.hold_display();
        if self.gesture == Gesture::DoubleTap {
            format!("double:{body}")
        } else {
            body
        }
    }

    fn hold_display(&self) -> String {
        let mut parts: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| m.as_str().to_string())
            .collect();
        if !self.key.is_empty() {
            parts.push(self.key.clone());
        }
        parts.join("+")
    }

    pub fn is_hold(&self) -> bool {
        self.gesture == Gesture::Hold
    }

    pub fn has(&self, m: Modifier) -> bool {
        self.modifiers.contains(&m)
    }

    /// Normalised id used for de-duplication and order-insensitive comparison.
    pub fn id(&self) -> String {
        let mut mods: Vec<String> = self
            .modifiers
            .iter()
            .map(|m| m.as_str().to_string())
            .collect();
        mods.sort();
        format!(
            "{}{}:{}",
            if self.gesture == Gesture::DoubleTap {
                "dbl:"
            } else {
                ""
            },
            mods.join("+"),
            self.key
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mac_default_talk_binding() {
        let b = Binding::parse("control+alt").unwrap();
        assert!(b.has(Modifier::Control));
        assert!(b.has(Modifier::Alt));
        assert_eq!(b.key, "", "两键同时按住时没有单独的触发键");
        assert_eq!(b.gesture, Gesture::Hold);
    }

    #[test]
    fn parses_aliases() {
        let a = Binding::parse("ctrl+option").unwrap();
        let b = Binding::parse("control+alt").unwrap();
        assert_eq!(a.modifiers, b.modifiers);
    }

    #[test]
    fn parses_dictation_with_fn() {
        let b = Binding::parse("fn+control").unwrap();
        assert!(b.has(Modifier::Fn));
        assert!(b.has(Modifier::Control));
    }

    #[test]
    fn parses_double_tap_gesture() {
        let b = Binding::parse("double:fn+control").unwrap();
        assert_eq!(b.gesture, Gesture::DoubleTap);
        assert_eq!(b.display(), "double:fn+control");
    }

    #[test]
    fn rejects_empty() {
        assert!(Binding::parse("").is_err());
        assert!(Binding::parse("   ").is_err());
    }

    #[test]
    fn rejects_mid_typo() {
        assert!(Binding::parse("hyper+a").is_err());
    }

    #[test]
    fn ids_are_order_insensitive() {
        assert_eq!(
            Binding::parse("alt+control").unwrap().id(),
            Binding::parse("control+alt").unwrap().id()
        );
    }
}
