//! 翻译层错误类型。实现 `Serialize` 以便直接作为 Tauri command 的错误返回。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranslateError {
    // 说明：`reqwest::Error` / `url::ParseError` 用 `#[from]` 自动转换，
    // 其余变体带上游返回的文本，方便直接在界面上显示。
    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("URL 构造失败：{0}")]
    Url(#[from] url::ParseError),

    #[error("响应解析失败：{0}")]
    Parse(String),

    #[error("Google 接口限流（HTTP 429），重试 {attempts} 次后仍失败")]
    RateLimited { attempts: u32 },

    #[error("Google 接口返回异常状态：HTTP {status}")]
    Status { status: u16 },

    #[error("待翻译文本为空")]
    Empty,

    #[error("输入或设置无效：{0}")]
    Invalid(String),

    #[error("无法读写设置文件：{0}")]
    Settings(String),

    #[error("历史库操作失败：{0}")]
    History(String),

    #[error("导出失败：{0}")]
    Export(String),
}

impl serde::Serialize for TranslateError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
