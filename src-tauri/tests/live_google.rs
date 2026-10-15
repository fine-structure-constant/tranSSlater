//! 真实接口的冒烟测试。默认被 `#[ignore]` 跳过，需要显式运行：
//!
//! ```bash
//! # 全部引擎（Google 需网络可达或已配置代理）
//! cargo test --test live_google -- --ignored --nocapture --test-threads=1
//!
//! # 只跑有道（国内直连即可）
//! cargo test --test live_google -- --ignored --nocapture youdao
//! ```
//!
//! 文件名为历史沿用（原项目只有 Google 一个引擎），现在覆盖全部驱动。

use transslater_lib::translate::EngineHub;

fn hub() -> EngineHub {
    EngineHub::new(None).expect("构建引擎表失败")
}

fn dump(r: &transslater_lib::translate::TranslateResult) {
    println!("引擎      : {}", r.engine);
    println!("译文      : {}", r.translation);
    println!("源语言    : {}", r.source_lang);
    println!("是否单词  : {}", r.is_word);
    println!("美式音标  : {:?}", r.phonetic);
    println!("英式音标  : {:?}", r.phonetic_uk);
    println!("发音      : {:?}", r.speak_url);
    println!("耗时      : {} ms", r.elapsed_ms);
    for d in &r.dictionary {
        println!("词典 [{}]: {:?}", d.pos, d.terms);
    }
    for s in &r.synonyms {
        println!("同义词[{}]: {:?}", s.pos, s.words);
    }
    for p in &r.phrases {
        println!("词组      : {} → {:?}", p.term, p.translation);
    }
    for g in &r.related {
        println!("同根词[{}]: {:?}", g.pos, g.words);
    }
    for e in &r.examples {
        println!("例句      : {} → {:?}", e.text, e.translation);
    }
    for s in &r.sections {
        println!(
            "板块      : {} ({}) 分组 {} 个",
            s.title,
            s.key,
            s.groups.len()
        );
    }
}

// ── 引擎表 ──────────────────────────────────────────────────

#[tokio::test]
#[ignore = "需要联网"]
async fn live_hub_lists_all_engines() {
    let list = hub().list();
    let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["google", "youdao", "youdaoTrans"], "引擎顺序必须稳定");

    for e in &list {
        println!(
            "{} | {} | 代理={} 句子={} 词典={} 发音={} | {}",
            e.id, e.name, e.needs_proxy, e.supports_sentence, e.supports_dictionary, e.supports_audio, e.note
        );
        assert!(!e.name.is_empty());
        assert!(!e.note.is_empty());
    }

    // 有道两兄弟必须是国内直连（不需要代理）
    assert!(list[0].needs_proxy, "Google 需要代理");
    assert!(!list[1].needs_proxy, "有道词典应直连");
    assert!(!list[2].needs_proxy, "有道翻译应直连");
}

// ── 有道词典（单词）─────────────────────────────────────────

#[tokio::test]
#[ignore = "需要联网"]
async fn live_youdao_dict_word() {
    let r = hub()
        .get("youdao")
        .unwrap()
        .translate("coercion", "en", "zh-CN")
        .await
        .expect("有道词典查询失败");
    dump(&r);

    assert_eq!(r.engine, "youdao");
    assert!(!r.translation.is_empty(), "译文不应为空");
    assert!(r.is_word, "单词查询应识别为单词");
    assert!(!r.dictionary.is_empty(), "单词查询应返回词典释义");
    assert!(
        r.speak_url.as_deref().unwrap_or("").contains("dictvoice"),
        "有道应给出可直接播放的发音链接"
    );
    assert!(
        !r.sections.is_empty() || !r.examples.is_empty(),
        "有道应至少返回一个扩展板块或例句"
    );
}

#[tokio::test]
#[ignore = "需要联网"]
async fn live_youdao_dict_common_word_has_collins() {
    // "coercion" 是生僻词，有道的柯林斯/辨析/词源都是空的；
    // 换常见词才能验证这几个板块的解析链路真的通。
    let r = hub()
        .get("youdao")
        .unwrap()
        .translate("get", "en", "zh-CN")
        .await
        .expect("有道词典查询失败");
    dump(&r);

    let keys: Vec<&str> = r.sections.iter().map(|s| s.key.as_str()).collect();
    assert!(keys.contains(&"collins"), "常见词应有柯林斯板块，实际：{keys:?}");
    assert!(keys.contains(&"discriminate"), "常见词应有词语辨析，实际：{keys:?}");
    assert!(keys.contains(&"etym"), "常见词应有词源，实际：{keys:?}");
    assert!(r.phonetic.is_some(), "应有美式音标");
    assert!(r.phonetic_uk.is_some(), "应有英式音标");
    assert!(!r.phrases.is_empty(), "get 应返回词组短语");
    assert!(!r.examples.is_empty(), "get 应返回例句");
    assert!(!r.related.is_empty(), "get 应返回同根词");
}

#[tokio::test]
#[ignore = "需要联网"]
async fn live_youdao_dict_sentence() {
    let r = hub()
        .get("youdao")
        .unwrap()
        .translate("The company was disruptive in its early years.", "en", "zh-CN")
        .await
        .expect("有道整句失败");
    dump(&r);

    assert!(!r.translation.is_empty(), "整句译文不应为空");
    assert!(!r.is_word, "整句不应被识别为单词");
    assert!(r.dictionary.is_empty(), "整句不应有词典释义");
}

// ── 有道翻译（aidemo）───────────────────────────────────────

#[tokio::test]
#[ignore = "需要联网"]
async fn live_youdao_trans_word_and_sentence() {
    let driver = hub().get("youdaoTrans").unwrap();

    let word = driver.translate("coercion", "en", "zh-CN").await.expect("有道翻译失败");
    dump(&word);
    assert_eq!(word.engine, "youdaoTrans");
    assert!(!word.translation.is_empty());
    assert_eq!(word.source_lang, "en", "应从 l 字段解析出源语言");
    assert!(word.is_word, "单个词应标记为单词，否则不会自动朗读");

    let sent = driver
        .translate("今天天气不错，我们出去走走吧。", "zh-CN", "en")
        .await
        .expect("中译英失败");
    println!("中译英: {}", sent.translation);
    assert!(!sent.translation.is_empty());
    assert!(!sent.is_word, "带句读的整句不应标记为单词");
}

// ── Google ─────────────────────────────────────────────────

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_google_word() {
    let r = hub()
        .get("google")
        .unwrap()
        .translate("coercion", "en", "zh-CN")
        .await
        .expect("Google 翻译失败");
    dump(&r);

    assert_eq!(r.engine, "google");
    assert!(!r.translation.is_empty());
    assert_eq!(r.source_lang, "en");
    assert!(r.is_word, "单词查询应识别为单词");
    assert!(!r.dictionary.is_empty(), "单词查询应返回词典数据");
}

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_google_sentence() {
    let r = hub()
        .get("google")
        .unwrap()
        .translate("The company was disruptive in its early years.", "en", "zh-CN")
        .await
        .expect("Google 翻译失败");
    dump(&r);

    assert!(!r.translation.is_empty());
    assert!(!r.is_word, "整句不应被识别为单词");
    assert!(r.dictionary.is_empty(), "整句不应有词典数据");
    assert!(r.segments.len() >= 1, "应返回分段");
}

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_google_multi_paragraph() {
    let r = hub()
        .get("google")
        .unwrap()
        .translate("line one\n\nline two", "en", "zh-CN")
        .await
        .expect("Google 翻译失败");
    for (i, s) in r.segments.iter().enumerate() {
        println!("段 {}: {:?}  ←  {:?}", i, s.text, s.source);
    }
    assert!(r.segments.len() >= 2, "多段应返回多个分段");
}
