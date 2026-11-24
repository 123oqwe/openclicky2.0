//! Configuration: TOML on disk, unknown fields preserved, validated on load.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub config_version: u32,
    pub provider: ProviderConfig,
    pub voice: VoiceConfig,
    pub shortcuts: Shortcuts,
    pub budget: BudgetConfig,
    pub tools: ToolsConfig,
    pub privacy: PrivacyConfig,
    pub ui: UiConfig,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            provider: ProviderConfig::default(),
            voice: VoiceConfig::default(),
            shortcuts: Shortcuts::default(),
            budget: BudgetConfig::default(),
            tools: ToolsConfig::default(),
            privacy: PrivacyConfig::default(),
            ui: UiConfig::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub chat_model: String,
    pub fast_model: String,
    pub deep_model: String,
    pub stt_model: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub stt_base_url: String,
    pub region_hint: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    Openai,
    Anthropic,
    OpenaiCompatible,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::Openai => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenaiCompatible => "openai_compatible",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::Openai => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::OpenaiCompatible => "http://localhost:1234/v1",
        }
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::Openai,
            base_url: ProviderKind::Openai.default_base_url().to_string(),
            chat_model: String::new(),
            fast_model: String::new(),
            deep_model: String::new(),
            stt_model: "whisper-1".into(),
            tts_model: "gpt-4o-mini-tts".into(),
            tts_voice: "alloy".into(),
            stt_base_url: String::new(),
            region_hint: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VoiceConfig {
    pub captions: bool,
    pub speech_speed: f32,
    pub max_answer_tokens: u32,
    pub language: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            captions: true,
            speech_speed: 1.0,
            max_answer_tokens: 700,
            language: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Shortcuts {
    pub talk: String,
    pub dictation: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            talk: "control+alt".into(),
            dictation: "fn+control".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BudgetConfig {
    pub daily_units: f64,
    pub monthly_units: f64,
    pub weights: Weights,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            daily_units: 200.0,
            monthly_units: 3000.0,
            weights: Weights::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Weights {
    pub voice: f64,
    pub dictation: f64,
    pub agent_step: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            voice: 1.0,
            dictation: 0.5,
            agent_step: 3.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ToolsConfig {
    pub shell: bool,
    pub allow_private_network: bool,
    pub workspace: String,
    pub max_steps: u32,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            shell: false,
            allow_private_network: false,
            workspace: String::new(),
            max_steps: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PrivacyConfig {
    pub debug_allow_text: bool,
    pub keep_screenshots_hours: u32,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            debug_allow_text: false,
            keep_screenshots_hours: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UiConfig {
    pub cursor_color: String,
    pub hide_cursor_when_idle: bool,
    pub interface_language: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            cursor_color: "#ffd166".into(),
            hide_cursor_when_idle: false,
            interface_language: "zh".into(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| Error::Io(e.to_string()))?;
        let cfg: Config =
            toml::from_str(&raw).map_err(|e| Error::BadRequest(format!("config.toml: {e}")))?;
        Ok(cfg)
    }

    pub fn from_toml(raw: &str) -> Result<Self> {
        toml::from_str(raw).map_err(|e| Error::BadRequest(format!("config.toml: {e}")))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Atomic save: write temp then rename, so a crash never truncates config.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(e.to_string()))?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, self.to_toml()).map_err(|e| Error::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::Io(e.to_string()))
    }

    /// Returns human readable problems; empty means usable.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.provider.base_url.trim().is_empty() {
            issues.push("provider.base_url 不能为空".to_string());
        }
        if !self.provider.base_url.starts_with("http") {
            issues.push("provider.base_url 必须以 http(s) 开头".to_string());
        }
        if self.provider.chat_model.trim().is_empty() {
            issues.push("还没有选择模型（Settings → 模型）".to_string());
        }
        if !(0.5..=1.5).contains(&self.voice.speech_speed) {
            issues.push("voice.speech_speed 必须在 0.5 到 1.5 之间".to_string());
        }
        if self.budget.daily_units <= 0.0 || self.budget.monthly_units <= 0.0 {
            issues.push("预算必须大于 0".to_string());
        }
        for s in [&self.shortcuts.talk, &self.shortcuts.dictation] {
            if let Err(e) = crate::hotkey::Binding::parse(s) {
                issues.push(format!("快捷键 {s:?} 无效：{}", e.user_message()));
            }
        }
        issues
    }

    pub fn effective_model(&self, tier: crate::router::Tier) -> String {
        match tier {
            crate::router::Tier::Deep if !self.provider.deep_model.is_empty() => {
                self.provider.deep_model.clone()
            }
            crate::router::Tier::Fast if !self.provider.fast_model.is_empty() => {
                self.provider.fast_model.clone()
            }
            _ => self.provider.chat_model.clone(),
        }
    }

    pub fn stt_endpoint_base(&self) -> &str {
        if self.provider.stt_base_url.is_empty() {
            &self.provider.base_url
        } else {
            &self.provider.stt_base_url
        }
    }
}
