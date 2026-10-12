# Translater 桌面翻译器

Windows 桌面应用，使用 Tauri 2、Rust 与 TypeScript。三个可切换的翻译引擎、不限条数的本地历史库、四种格式的学习导出，以及词典释义、同义词、例句与朗读。

## 直接运行

双击 `artifacts/Translater.exe`。这个文件包含界面资源，不需要启动 Vite、Node.js 或 Rust。

运行环境需要 Microsoft Edge WebView2 Runtime（Windows 10/11 通常已安装）。

## 翻译引擎

| 引擎 | id | 国内直连 | 词典数据 | 发音 | 说明 |
| --- | --- | --- | --- | --- | --- |
| Google 翻译 | `google` | 否 | 有 | 系统朗读 | 默认引擎；公开的非官方接口，大陆需代理 |
| 有道词典 | `youdao` | 是 | 最全 | mp3 | 单词查词质量最好 |
| 有道翻译 | `youdaoTrans` | 是 | 无 | mp3 | 单词与整句都能翻，带发音音频 |

三个引擎都无需申请密钥。引擎可在工具栏直接切换，也可在设置里指定默认值；切换引擎会用新引擎重新翻译当前文本。缓存按「原文 + 语言对 + 引擎」区分，换引擎不会拿到另一个引擎的旧结果。

有道词典驱动直接读 `dict.youdao.com/jsonapi`，返回的板块比 Google 免费端点丰富得多，包含柯林斯双解、英英释义（WordNet）、专业释义（论文库）、网络释义、词语辨析、词源、词组短语、同根词、真题例句与 AI 释义。整句查询时该接口不返回词典数据，驱动会自动回落到有道翻译接口取译文。发音走 `dictvoice` 的 mp3，比系统 TTS 稳定。

有道的 `oxford*`、`webster`、`wordElaboration` 板块返回 `encryptedData`，需要逆向前端 JS 才能解，本驱动不取用。

> Google 的公开接口在代理配置正确时工作正常（TUN 模式代理也可以）。它偶尔会返回 HTTP 429，此时会做有限次数的退避重试；仍失败时错误提示里会建议改用有道的两个引擎。

## 历史库与导出

历史保存在应用数据目录的 `history.db`（SQLite，WAL 模式）。与网页版词典不同，**本地历史不设条数上限，也不会自动清理**。

- 工具栏「历史」按钮或 `Ctrl+H` 打开抽屉；点击任意一条可把完整结果回填到主界面。
- 支持按原文或译文搜索、单条删除、一键清空。
- 设置里可关闭记录（`historyMode` 为 `off` 时不再写入新记录，已有记录保留）。

导出支持四种格式，均从数据库流式读取，不受内存限制：

| 格式 | 扩展名 | 用途 |
| --- | --- | --- |
| CSV | `.csv` | Excel 表格；带 BOM，中文不乱码 |
| Markdown | `.md` | 按日期分组的阅读稿 |
| Anki | `.txt` | 制表符分隔两列，可直接导入 Anki |
| JSON | `.json` | 完整结果，含全部词典板块 |

## 开发启动

要求 Node.js 22.12+（或 20.19+）、pnpm、Rust MSVC 工具链、Visual Studio C++ Build Tools 与 Windows SDK。

```powershell
pnpm install
.\dev.bat
```

也可运行 `pnpm desktop`。启动脚本自动查找 PATH、标准 Rust/Visual Studio 安装和本机已有的 `D:\.ENV` 工具。需要定制时，将 `tools.example.json` 复制为 `tools.local.json` 并修改工具及构建缓存目录；该文件不会提交到版本控制。

`pnpm dev` 只启动浏览器界面预览。浏览器预览会显示提示，真实翻译由桌面程序的 Rust 后端执行。开发服务器固定监听 `127.0.0.1:1420`；若端口被占用，请关闭之前启动的 Vite 或开发进程再启动。

## 构建

```powershell
# 生成 EXE 和安装包
.\build.bat

# 只生成 EXE，避免下载安装器工具
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/desktop.ps1 -Action build -NoBundle
```

可执行文件会复制到 `artifacts/Translater.exe`。安装包位于构建缓存目录的 `release/bundle` 下。脚本会输出实际路径；没有配置其他目录时，默认使用 `src-tauri/target`，本机已有 D 盘项目缓存时会复用该目录。

## 使用

- 输入或粘贴文本，停顿后自动翻译；可以在设置中关闭自动翻译。
- `Ctrl+Enter` 立即翻译，`Escape` 清空，`Ctrl+H` 打开历史；中文输入法组词期间不触发翻译。
- 点击互换按钮可将译文放入输入框并反向翻译。自动检测模式需要先取得一次结果。
- 点击译词、同义词或同根词继续查询；词典、释义、同义词、例句和扩展板块按接口返回的实际内容显示，没有数据的板块不会出现。
- 复制按钮复制完整译文。朗读优先播放引擎返回的 mp3（有道提供），没有音频时退回系统 SpeechSynthesis。
- 一次最多翻译 5000 个 Unicode 字符；长文请分段。
- 翻译结果缓存最长 24 小时，最多 2000 条，程序退出后清空。

## 设置文件

桌面设置保存在应用配置目录的 `settings.json`，Windows 通常为 `%APPDATA%\com.tauri-app.translater\settings.json`。启动时读取该文件，界面保存前会验证代理、语言对、引擎 id 与历史策略。设置无效时保留上一份配置并显示原因；文件损坏时应用仍可打开，可通过设置重新保存。

```json
{
  "proxy": "",
  "autoTranslate": true,
  "autoSpeak": false,
  "langPair": "auto|zh-CN",
  "engine": "google",
  "historyMode": "all"
}
```

旧版本写下的设置文件缺少 `engine` 与 `historyMode` 时仍可正常加载，缺失字段会取默认值。

留空代理使用 reqwest 的系统/环境代理检测；显式地址覆盖自动检测。支持 HTTP、HTTPS、SOCKS5、SOCKS5H。代理只影响 Google，有道的两个引擎始终直连。此文件可能包含代理凭据，请勿分享。浏览器预览的设置仅使用浏览器本地存储。

## 验证

运行 Rust 测试前请先关闭开发模式窗口与 `dev.bat` 进程，避免 Windows 锁定正在运行的 EXE。

```powershell
# TypeScript 类型检查及前端生产构建
pnpm build

# Rust 单元测试：设置、缓存、三个引擎的响应解析、历史库、导出
pnpm test:rust

# 真实网络冒烟测试（默认 #[ignore]，需要显式打开）
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/desktop.ps1 -Action smoke

# 启动 Vite 后，在另一终端执行界面回归测试
pnpm dev
pnpm test:ui
```

冒烟测试覆盖引擎表与三个驱动的真实请求。Google 相关用例需要可达的网络或代理，其余用例国内直连即可通过。只跑有道：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test live_google -- --ignored --nocapture youdao
```

界面测试使用已安装的 Microsoft Edge 与模拟 Tauri IPC，覆盖引擎选择器、全部词典板块、历史抽屉（列表、搜索、删除、清空、回填）、导出流程、设置校验、异步竞态、错误恢复、语言互换、输入限制与键盘焦点。截图保存在 `test-results`。这些测试与真实网络测试分开执行。

设计与验证记录见 `design/index.html`。三个引擎都使用公开的非官方接口，服务可用性及词典字段由上游决定；HTTP 429 使用有限次数的退避重试，其他失败提供可恢复错误提示。

## 代码结构

- `src/main.ts`：输入与交互状态、引擎切换、历史抽屉与导出；`src/render.ts`：结果与词典渲染。
- `src/settings.ts`、`src/api.ts`：设置与桌面命令访问。
- `src-tauri/src/lib.rs`：Tauri 命令层，把驱动、缓存与历史库暴露给前端。
- `src-tauri/src/settings.rs`：配置文件默认值、校验与读写。
- `src-tauri/src/translate`：驱动抽象（`driver.rs`）、数据模型（`model.rs`）、三个引擎实现、错误模型、TTL/LRU 缓存。
- `src-tauri/src/history.rs`：SQLite 历史库；`src-tauri/src/export.rs`：四种导出格式。
- `scripts/desktop.ps1`：Windows 工具链检测、开发、测试与构建入口。
