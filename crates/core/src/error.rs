use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    ScreenRecording,
    Microphone,
    Accessibility,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Permission::ScreenRecording => "屏幕录制",
            Permission::Microphone => "麦克风",
            Permission::Accessibility => "辅助功能",
        }
    }
}

/// Errors are part of the contract: every variant maps to a stable `code()`
/// and a user facing Chinese message. See docs/02-sdd.md §13.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum Error {
    #[error("no api key configured")]
    NoApiKey,
    #[error("provider rejected the credentials")]
    AuthFailed,
    #[error("provider blocked the request by region")]
    RegionBlocked,
    #[error("missing permission: {0:?}")]
    NoPermission(Permission),
    #[error("no model available")]
    NoModel,

    #[error("rate limited")]
    RateLimited { retry_after: Option<f64> },
    #[error("provider outage")]
    Outage,
    #[error("timed out")]
    Timeout,
    #[error("network error: {0}")]
    Network(String),

    #[error("recording too short")]
    TooShort,
    #[error("empty audio")]
    EmptyAudio,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("budget exhausted")]
    BudgetExhausted,
    #[error("policy denied: {0}")]
    PolicyDenied(String),

    #[error("invalid state transition: {0}")]
    State(String),
    #[error("storage error: {0}")]
    Store(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::NoApiKey => "E_NO_KEY",
            Error::AuthFailed => "E_AUTH",
            Error::RegionBlocked => "E_REGION",
            Error::NoPermission(_) => "E_PERM",
            Error::NoModel => "E_MODEL",
            Error::RateLimited { .. } => "E_RATE",
            Error::Outage => "E_OUTAGE",
            Error::Timeout => "E_TIMEOUT",
            Error::Network(_) => "E_NET",
            Error::TooShort => "E_SHORT",
            Error::EmptyAudio => "E_EMPTY_AUDIO",
            Error::BadRequest(_) => "E_BAD_REQ",
            Error::BudgetExhausted => "E_BUDGET",
            Error::PolicyDenied(_) => "E_POLICY",
            Error::State(_) => "E_STATE",
            Error::Store(_) => "E_STORE",
            Error::Protocol(_) => "E_PROTOCOL",
            Error::Io(_) => "E_IO",
            Error::Internal(_) => "E_INTERNAL",
        }
    }

    pub fn user_message(&self) -> String {
        match self {
            Error::NoApiKey => "还没有配置 API key。打开设置 → 模型，填入你自己的密钥。".into(),
            Error::AuthFailed => "密钥被拒绝了，请在设置里更新。".into(),
            Error::RegionBlocked => {
                "上游按地区拦截了这个请求。换一个 provider，或改用本地兼容端点。".into()
            }
            Error::NoPermission(p) => format!(
                "缺少「{}」权限。在系统设置里打开它，然后回到 OpenClicky。",
                p.label()
            ),
            Error::NoModel => "没有可用的模型。打开设置 → 模型，选一个。".into(),
            Error::RateLimited { .. } => "请求太频繁了，稍后会自动重试。".into(),
            Error::Outage => "模型服务暂时不可用。".into(),
            Error::Timeout => "这次超时了。".into(),
            Error::Network(e) => format!("网络连不上：{e}"),
            Error::TooShort => "按住的时间太短了。".into(),
            Error::EmptyAudio => "没听到声音。检查一下麦克风。".into(),
            Error::BadRequest(m) => format!("请求有问题：{m}"),
            Error::BudgetExhausted => {
                "今天的额度用完了。可以在设置里临时提额，或等明天恢复。".into()
            }
            Error::PolicyDenied(m) => format!("这个操作需要你确认：{m}"),
            Error::State(m) => format!("内部状态异常（{m}）。已经记录下来了。"),
            Error::Store(m) => format!("本地存储出错了：{m}"),
            Error::Protocol(m) => format!("模型返回了看不懂的内容：{m}"),
            Error::Io(m) => format!("文件读写失败：{m}"),
            Error::Internal(m) => format!("内部错误：{m}"),
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Error::RateLimited { .. } | Error::Outage | Error::Timeout | Error::Network(_)
        )
    }
}
