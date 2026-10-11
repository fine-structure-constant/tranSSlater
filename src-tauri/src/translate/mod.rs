//! 翻译层：Google 接口客户端、数据模型、缓存、错误。

pub mod cache;
pub mod error;
pub mod google;
pub mod model;

pub use cache::{CacheKey, TranslateCache};
pub use error::TranslateError;
pub use google::GoogleClient;
pub use model::{
    BackEntry, Definition, DefinitionGroup, DictEntry, Segment, SynonymGroup, TranslateResult,
};
