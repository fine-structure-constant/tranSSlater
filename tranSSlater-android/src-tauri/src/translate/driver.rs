//! 翻译驱动抽象。
//!
//! 每个驱动封装一个上游接口，把响应映射成统一的 [`TranslateResult`]。
//! 新增引擎只需实现 [`Driver`] 并注册进 [`EngineHub`]。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde::Serialize;

use super::error::TranslateError;
use super::google::GoogleDriver;
use super::model::TranslateResult;
use super::youdao::YoudaoDictDriver;
use super::youdao_trans::YoudaoTransDriver;

/// 引擎元信息。前端用它渲染选择器并提示能力差异。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    /// 稳定 id，会写进历史记录
    pub id: String,
    /// 展示名
    pub name: String,
    /// 一句话说明
    pub note: String,
    /// 是否需要代理才能访问
    pub needs_proxy: bool,
    /// 是否支持整句翻译
    pub supports_sentence: bool,
    /// 是否提供词典数据（释义 / 例句 / 同义词等）
    pub supports_dictionary: bool,
    /// 是否提供发音音频
    pub supports_audio: bool,
}

/// 一个翻译引擎。
#[async_trait]
pub trait Driver: Send + Sync {
    fn info(&self) -> EngineInfo;

    /// 发起翻译。`from` 为 `"auto"` 时由上游自动检测。
    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError>;
}

/// 引擎注册表。持有全部驱动，按 id 取用。
pub struct EngineHub {
    drivers: RwLock<HashMap<String, Arc<dyn Driver>>>,
}

impl EngineHub {
    /// 按固定顺序构建全部驱动。`proxy` 只作用于需要翻墙的引擎。
    pub fn new(proxy: Option<&str>) -> Result<Self, TranslateError> {
        let mut drivers: HashMap<String, Arc<dyn Driver>> = HashMap::new();

        let google: Arc<dyn Driver> = Arc::new(GoogleDriver::new(proxy)?);
        drivers.insert(google.info().id, google);

        let youdao_dict: Arc<dyn Driver> = Arc::new(YoudaoDictDriver::new()?);
        drivers.insert(youdao_dict.info().id, youdao_dict);

        let youdao_trans: Arc<dyn Driver> = Arc::new(YoudaoTransDriver::new()?);
        drivers.insert(youdao_trans.info().id, youdao_trans);

        Ok(Self {
            drivers: RwLock::new(drivers),
        })
    }

    /// 取一个驱动。id 不存在时返回错误，避免静默回退到别的引擎。
    pub fn get(&self, id: &str) -> Result<Arc<dyn Driver>, TranslateError> {
        self.drivers
            .read()
            .expect("drivers 锁中毒")
            .get(id)
            .cloned()
            .ok_or_else(|| TranslateError::Invalid(format!("未知的翻译引擎：{id}")))
    }

    /// 引擎列表。顺序固定，方便前端做稳定展示。
    pub fn list(&self) -> Vec<EngineInfo> {
        const ORDER: [&str; 3] = [GoogleDriver::ID, YoudaoDictDriver::ID, YoudaoTransDriver::ID];
        let map = self.drivers.read().expect("drivers 锁中毒");
        ORDER
            .iter()
            .filter_map(|id| map.get(*id).map(|d| d.info()))
            .collect()
    }

    /// 热替换代理：只重建需要代理的引擎。
    pub fn set_proxy(&self, proxy: Option<&str>) -> Result<(), TranslateError> {
        let google: Arc<dyn Driver> = Arc::new(GoogleDriver::new(proxy)?);
        self.drivers
            .write()
            .expect("drivers 锁中毒")
            .insert(google.info().id, google);
        Ok(())
    }

    /// 引擎 id 是否存在。用于校验设置文件。
    pub fn contains(&self, id: &str) -> bool {
        self.drivers.read().expect("drivers 锁中毒").contains_key(id)
    }

    /// 默认引擎 id。
    pub fn default_id(&self) -> &'static str {
        GoogleDriver::ID
    }
}
