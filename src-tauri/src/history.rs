//! 翻译历史：SQLite 单表，**不设条数上限**。
//!
//! 设计取舍：
//! - 只 INSERT，从不裁剪，满足「完整记录」这个核心诉求
//! - 完整结果以 JSON 存在 `payload` 列，列表查询只读摘要列，避免翻页时解析大字段
//! - 导出走流式游标，历史到十万条也不会把内存撑爆
//! - 开 WAL：读写不互相阻塞，且异常退出不容易损坏

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use super::translate::{TranslateError, TranslateResult};

/// 列表用的摘要，不含完整结果。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummary {
    pub id: i64,
    pub query: String,
    pub from_lang: String,
    pub to_lang: String,
    pub engine: String,
    pub translation: String,
    pub is_word: bool,
    /// Unix 毫秒
    pub created_at: i64,
}

/// 导出用的完整行。
#[derive(Debug, Clone)]
pub struct HistoryRecord {
    pub id: i64,
    pub query: String,
    pub from_lang: String,
    pub to_lang: String,
    pub engine: String,
    pub translation: String,
    pub is_word: bool,
    pub created_at: i64,
    /// 完整结果的 JSON
    pub payload: String,
}

impl HistoryRecord {
    /// 解析完整结果。历史数据损坏时返回 `None`，不打断导出。
    pub fn result(&self) -> Option<TranslateResult> {
        serde_json::from_str(&self.payload).ok()
    }
}

pub struct HistoryStore {
    conn: Connection,
}

/// 当前时间的 Unix 毫秒。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl HistoryStore {
    /// 打开（必要时创建）历史库。`path` 为 `":memory:"` 时用于测试。
    pub fn open(path: &Path) -> Result<Self, TranslateError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| TranslateError::History(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| TranslateError::History(e.to_string()))?;
        Self::init(conn)
    }

    /// 内存库，仅测试使用。
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, TranslateError> {
        let conn = Connection::open_in_memory().map_err(|e| TranslateError::History(e.to_string()))?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, TranslateError> {
        let store = Self { conn };
        store.exec_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS history (
               id          INTEGER PRIMARY KEY AUTOINCREMENT,
               query       TEXT    NOT NULL,
               from_lang   TEXT    NOT NULL,
               to_lang     TEXT    NOT NULL,
               engine      TEXT    NOT NULL,
               translation TEXT    NOT NULL,
               is_word     INTEGER NOT NULL DEFAULT 0,
               payload     TEXT    NOT NULL,
               created_at  INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at DESC, id DESC);
             CREATE INDEX IF NOT EXISTS idx_history_query   ON history(query);",
        )?;
        Ok(store)
    }

    fn exec_batch(&self, sql: &str) -> Result<(), TranslateError> {
        self.conn
            .execute_batch(sql)
            .map_err(|e| TranslateError::History(e.to_string()))
    }

    /// 落库一条翻译结果，返回新记录 id。
    pub fn add(&self, result: &TranslateResult, created_at: i64) -> Result<i64, TranslateError> {
        let payload = serde_json::to_string(result)
            .map_err(|e| TranslateError::History(e.to_string()))?;
        self.conn
            .execute(
                "INSERT INTO history
                   (query, from_lang, to_lang, engine, translation, is_word, payload, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    result.query,
                    result.source_lang,
                    result.target_lang,
                    result.engine,
                    result.translation,
                    result.is_word as i32,
                    payload,
                    created_at,
                ],
            )
            .map_err(|e| TranslateError::History(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 分页列出，按时间倒序。`filter` 非空时按原文/译文模糊匹配。
    pub fn list(
        &self,
        offset: i64,
        limit: i64,
        filter: Option<&str>,
    ) -> Result<Vec<HistorySummary>, TranslateError> {
        let filter = filter.map(str::trim).filter(|f| !f.is_empty());
        let like = filter.map(|f| format!("%{}%", escape_like(f)));

        let sql = if like.is_some() {
            "SELECT id, query, from_lang, to_lang, engine, translation, is_word, created_at
               FROM history
              WHERE query LIKE ?1 ESCAPE '\\' OR translation LIKE ?1 ESCAPE '\\'
              ORDER BY created_at DESC, id DESC
              LIMIT ?2 OFFSET ?3"
        } else {
            "SELECT id, query, from_lang, to_lang, engine, translation, is_word, created_at
               FROM history
              ORDER BY created_at DESC, id DESC
              LIMIT ?1 OFFSET ?2"
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| TranslateError::History(e.to_string()))?;

        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HistorySummary> {
            Ok(HistorySummary {
                id: row.get(0)?,
                query: row.get(1)?,
                from_lang: row.get(2)?,
                to_lang: row.get(3)?,
                engine: row.get(4)?,
                translation: row.get(5)?,
                is_word: row.get::<_, i32>(6)? != 0,
                created_at: row.get(7)?,
            })
        };

        let rows = match like {
            Some(pat) => stmt.query_map(params![pat, limit, offset], map),
            None => stmt.query_map(params![limit, offset], map),
        }
        .map_err(|e| TranslateError::History(e.to_string()))?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| TranslateError::History(e.to_string()))
    }

    /// 总条数（`filter` 语义同 [`list`]）。
    pub fn count(&self, filter: Option<&str>) -> Result<i64, TranslateError> {
        let filter = filter.map(str::trim).filter(|f| !f.is_empty());
        let like = filter.map(|f| format!("%{}%", escape_like(f)));
        let n = match like {
            Some(pat) => self.conn.query_row(
                "SELECT COUNT(*) FROM history
                  WHERE query LIKE ?1 ESCAPE '\\' OR translation LIKE ?1 ESCAPE '\\'",
                params![pat],
                |r| r.get(0),
            ),
            None => self
                .conn
                .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0)),
        }
        .map_err(|e| TranslateError::History(e.to_string()))?;
        Ok(n)
    }

    /// 取一条完整结果，用于点击历史回填界面。
    pub fn get(&self, id: i64) -> Result<Option<TranslateResult>, TranslateError> {
        let payload: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM history WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| TranslateError::History(e.to_string()))?;
        Ok(payload.and_then(|p| serde_json::from_str(&p).ok()))
    }

    pub fn delete(&self, id: i64) -> Result<bool, TranslateError> {
        let n = self
            .conn
            .execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| TranslateError::History(e.to_string()))?;
        Ok(n > 0)
    }

    /// 清空全部历史，返回删除条数。
    pub fn clear(&self) -> Result<usize, TranslateError> {
        let n = self
            .conn
            .execute("DELETE FROM history", [])
            .map_err(|e| TranslateError::History(e.to_string()))?;
        // 让自增 id 重新从 1 开始，避免清空后 id 继续膨胀。
        self.exec_batch("DELETE FROM sqlite_sequence WHERE name = 'history';")?;
        Ok(n)
    }

    /// 按时间正序流式遍历全部记录，供导出使用。
    ///
    /// 逐行回调，不把整个历史读进内存。
    pub fn for_each<F>(&self, mut f: F) -> Result<usize, TranslateError>
    where
        F: FnMut(HistoryRecord) -> Result<(), TranslateError>,
    {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, query, from_lang, to_lang, engine, translation, is_word, payload, created_at
                   FROM history
                  ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| TranslateError::History(e.to_string()))?;

        let mut rows = stmt
            .query([])
            .map_err(|e| TranslateError::History(e.to_string()))?;

        let mut n = 0usize;
        while let Some(row) = rows
            .next()
            .map_err(|e| TranslateError::History(e.to_string()))?
        {
            let record = HistoryRecord {
                id: row.get(0).map_err(|e| TranslateError::History(e.to_string()))?,
                query: row.get(1).map_err(|e| TranslateError::History(e.to_string()))?,
                from_lang: row.get(2).map_err(|e| TranslateError::History(e.to_string()))?,
                to_lang: row.get(3).map_err(|e| TranslateError::History(e.to_string()))?,
                engine: row.get(4).map_err(|e| TranslateError::History(e.to_string()))?,
                translation: row.get(5).map_err(|e| TranslateError::History(e.to_string()))?,
                is_word: row.get::<_, i32>(6).map_err(|e| TranslateError::History(e.to_string()))? != 0,
                payload: row.get(7).map_err(|e| TranslateError::History(e.to_string()))?,
                created_at: row.get(8).map_err(|e| TranslateError::History(e.to_string()))?,
            };
            f(record)?;
            n += 1;
        }
        Ok(n)
    }

    /// 数据库文件占用的字节数（含 WAL）。用于在界面上显示体积。
    pub fn size_bytes(&self) -> u64 {
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap_or(0);
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |r| r.get(0))
            .unwrap_or(0);
        (page_count.max(0) as u64) * (page_size.max(0) as u64)
    }
}

/// 转义 LIKE 通配符，让用户输入的 `%` `_` 按字面匹配。
fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::google::parse as parse_google;
    use serde_json::json;

    fn sample(query: &str, translation: &str) -> TranslateResult {
        let raw = json!([[[translation, query]], null, "en"]);
        parse_google(&raw, query, "zh-CN").unwrap()
    }

    fn store() -> HistoryStore {
        HistoryStore::open_in_memory().unwrap()
    }

    #[test]
    fn inserts_without_any_limit_and_counts() {
        let s = store();
        for i in 0..1200 {
            s.add(&sample(&format!("word{i}"), "译"), i as i64).unwrap();
        }
        assert_eq!(s.count(None).unwrap(), 1200);
    }

    #[test]
    fn lists_newest_first_with_paging() {
        let s = store();
        s.add(&sample("a", "甲"), 1).unwrap();
        s.add(&sample("b", "乙"), 2).unwrap();
        s.add(&sample("c", "丙"), 3).unwrap();

        let page = s.list(0, 2, None).unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].query, "c");
        assert_eq!(page[1].query, "b");

        let page2 = s.list(2, 2, None).unwrap();
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].query, "a");
    }

    #[test]
    fn filters_by_query_or_translation() {
        let s = store();
        s.add(&sample("coercion", "强迫"), 1).unwrap();
        s.add(&sample("hello", "你好"), 2).unwrap();

        assert_eq!(s.count(Some("coer")).unwrap(), 1);
        assert_eq!(s.list(0, 10, Some("coer")).unwrap()[0].query, "coercion");
        assert_eq!(s.count(Some("你好")).unwrap(), 1);
        assert_eq!(s.count(Some("absent")).unwrap(), 0);
    }

    #[test]
    fn like_wildcards_are_literal() {
        let s = store();
        s.add(&sample("100%", "百分号"), 1).unwrap();
        s.add(&sample("abc", "字母"), 2).unwrap();
        assert_eq!(s.count(Some("%")).unwrap(), 1);
        assert_eq!(s.count(Some("_")).unwrap(), 0);
    }

    #[test]
    fn round_trips_full_result_and_deletes() {
        let s = store();
        let id = s.add(&sample("coercion", "强迫"), 42).unwrap();
        let got = s.get(id).unwrap().unwrap();
        assert_eq!(got.translation, "强迫");
        assert_eq!(got.engine, "google");
        assert!(s.delete(id).unwrap());
        assert!(!s.delete(id).unwrap());
        assert!(s.get(id).unwrap().is_none());
    }

    #[test]
    fn streams_rows_in_chronological_order() {
        let s = store();
        s.add(&sample("b", "乙"), 2).unwrap();
        s.add(&sample("a", "甲"), 1).unwrap();
        let mut seen = Vec::new();
        let n = s
            .for_each(|r| {
                seen.push(r.query.clone());
                Ok(())
            })
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(seen, vec!["a", "b"]);
    }

    #[test]
    fn clear_removes_everything_and_resets_ids() {
        let s = store();
        s.add(&sample("a", "甲"), 1).unwrap();
        s.add(&sample("b", "乙"), 2).unwrap();
        assert_eq!(s.clear().unwrap(), 2);
        assert_eq!(s.count(None).unwrap(), 0);
        let id = s.add(&sample("c", "丙"), 3).unwrap();
        assert_eq!(id, 1, "清空后自增 id 应重新从 1 开始");
    }

    #[test]
    fn size_is_reported() {
        let s = store();
        s.add(&sample("a", "甲"), 1).unwrap();
        assert!(s.size_bytes() > 0);
    }
}
