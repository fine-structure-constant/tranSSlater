//! 翻译层：驱动抽象、各引擎实现、数据模型、缓存、错误。

pub mod cache;
pub mod driver;
pub mod error;
pub mod google;
pub mod model;
pub mod youdao;
pub mod youdao_trans;

pub use cache::{CacheKey, TranslateCache};
pub use driver::{Driver, EngineHub, EngineInfo};
pub use error::TranslateError;
pub use model::TranslateResult;
