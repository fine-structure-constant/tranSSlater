//! 真实接口的集成测试。默认被 `#[ignore]` 跳过，需要显式运行：
//!
//! ```bash
//! # 直连（需网络可达 Google）
//! cargo test --test live_google -- --ignored --nocapture
//!
//! # 走代理
//! HTTPS_PROXY=http://127.0.0.1:7890 cargo test --test live_google -- --ignored --nocapture
//! ```

use translater_lib::translate::GoogleClient;

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_word_query() {
    let client = GoogleClient::new(None).expect("构建客户端失败");
    let r = client
        .translate("coercion", "en", "zh-CN")
        .await
        .expect("翻译失败");

    println!("--- 单词 coercion ---");
    println!("译文      : {}", r.translation);
    println!("源语言    : {}", r.source_lang);
    println!("拼音      : {:?}", r.romanization);
    println!("音标      : {:?}", r.phonetic);
    println!("是否单词  : {}", r.is_word);
    println!("耗时      : {} ms", r.elapsed_ms);
    for d in &r.dictionary {
        println!("词典 [{}]: {:?}", d.pos, d.terms);
    }
    println!("近义候选  : {:?}", r.alternatives);
    for s in &r.synonyms {
        println!("同义词[{}]: {:?}", s.pos, s.words);
    }
    for g in &r.definitions {
        for d in &g.definitions {
            println!("英英释义[{}]: {}", g.pos, d.text);
        }
    }
    for e in &r.examples {
        println!("例句      : {}", e);
    }

    assert!(!r.translation.is_empty(), "译文不应为空");
    assert_eq!(r.source_lang, "en");
    assert!(r.is_word, "单词查询应识别为单词");
    assert!(!r.dictionary.is_empty(), "单词查询应返回词典数据");
}

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_sentence_query() {
    let client = GoogleClient::new(None).expect("构建客户端失败");
    let r = client
        .translate(
            "The company was disruptive in its early years.",
            "en",
            "zh-CN",
        )
        .await
        .expect("翻译失败");

    println!("--- 整句 ---");
    println!("译文  : {}", r.translation);
    println!("分段数: {}", r.segments.len());
    println!("是否单词: {}", r.is_word);

    assert!(!r.translation.is_empty());
    assert!(!r.is_word, "整句不应被识别为单词");
    assert!(r.dictionary.is_empty(), "整句不应有词典数据");
}

#[tokio::test]
#[ignore = "需要联网访问 Google"]
async fn live_multi_paragraph() {
    let client = GoogleClient::new(None).expect("构建客户端失败");
    let r = client
        .translate("line one\n\nline two", "en", "zh-CN")
        .await
        .expect("翻译失败");

    println!("--- 多段 ---");
    for (i, s) in r.segments.iter().enumerate() {
        println!("段 {}: {:?}  ←  {:?}", i, s.text, s.source);
    }

    assert!(r.segments.len() >= 2, "多段应返回多个分段");
}
