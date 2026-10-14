# Translater 桌面翻译器

Windows 桌面应用，使用 Tauri 2、Rust 与 TypeScript。两个可切换的界面皮肤、四档字号、可记住的窗口尺寸、菜单栏，三个可切换的翻译引擎、不限条数的本地历史库、四种格式的学习导出，词典释义、同义词、例句与朗读，以及在任意程序里划词取词的悬浮图标与资源管理器右键菜单。

## 直接运行

双击 `artifacts/Translater.exe`。这个文件包含界面资源，不需要启动 Vite、Node.js 或 Rust。

当前优化版使用 `artifacts/Translater.exe`；此前的满窗狂欢版保留在 `artifacts/Translater-party.exe` 供比较。两者共用已有设置与历史。构建脚本可通过 `-ArtifactName` 指定输出文件名。

运行环境需要 Microsoft Edge WebView2 Runtime（Windows 10/11 通常已安装）。

## 界面

窗口顶部是菜单栏（文件 / 编辑 / 视图 / 说明），下面一行是语言对与引擎选择。界面上不再挂任何小字说明——引擎差异、快捷键、数据位置、版本信息都收在「说明」对话框里（`F1` 或菜单打开）。

### 皮肤

皮肤只覆盖 CSS 变量，不碰布局，切换即时生效并写入设置。

| 皮肤 | id | 风格 |
| --- | --- | --- |
| 简约 | `plain` | 默认。白底、浅灰描边、大圆角，素净 |
| 远点 | `meridian` | 奶油色工业面板 + 深色仪表屏输入区，近直角边框、四角螺钉、顶部四段彩虹条（蓝 40% / 橄榄 25% / 金 20% / 橙 15%） |

`meridian` 取自 `concept/meridian` 的设计稿（Nasapunk 取向：橙色只用在动作上，深度靠一道柔和的投影和一块凹陷的深色显示屏）。彩虹条、螺钉、品牌四段条只在 `meridian` 下显示，`plain` 下是 `display: none`。

新增皮肤：在 `src/theme.css` 写一个 `:root[data-theme="x"]` 块，在 `src/settings.ts` 的 `THEMES` 和 `src-tauri/src/settings.rs` 的 `THEMES` 里登记，并在 `index.html` 的「视图」菜单里加两个按钮。没登记的皮肤会被后端校验拒绝，避免界面退回没有令牌的裸样式。

### 字号与窗口

- 字号四档（小 / 标准 / 大 / 特大）对应 `--fs` 的 0.875 / 1 / 1.125 / 1.25。所有 `font-size` 都写成 `calc(Npx * var(--fs))`，切换只改 `<html data-font>`。
- 窗口尺寸有四个预设（紧凑 900×620、标准 1020×760、宽屏 1280×820、大屏 1440×900），从「视图 → 窗口大小」直接改，也可在设置里选。
- 打开「记住窗口大小」后，手动拖动窗口会在 600 ms 防抖后写回设置；关闭时只保留预设值。
- 双击菜单栏空白处放大 / 还原窗口。

## 划词翻译

在任意程序里拖选一段文字（浏览器里也一样），松开鼠标后光标旁浮出一枚 **translate** 小图标，点它就把选中内容送进本窗口翻译。**原文不会被替换，也不会被修改** —— 译文只出现在这里。

| 操作 | 结果 |
| --- | --- |
| 拖选一段文字 | 松开鼠标后浮出图标（需拖出 6px 且超过 60ms，避免和单击混淆） |
| 双击一个词 | 同上，方便查单词 |
| 点图标 | 窗口置顶、还原并聚焦，随后翻译选中内容 |
| 右键 / 中键 / 滚轮 / 点别处 | 收起图标，不做任何事 |

图标是一个独立的无边框窗口（`bubble.html`），带 `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`，**不会抢焦点**，所以它不会打断你在原程序里的输入或选择。正因为拿不到鼠标焦点，点击不能依赖 WebView 事件，而是由全局鼠标钩子（`WH_MOUSE_LL`）对图标窗口的屏幕矩形做命中测试后接管。

**外观**：一枚横着的字标 —— 小写的 `translate`（字距放宽）加底边一条四段彩虹条（蓝 40% / 橄榄 25% / 金 20% / 橙 15%，和主界面顶部条同一套比例与配色）。整体跟随皮肤：`plain` 下是白底深字、两端全圆的胶囊；`meridian` 下是深色仪表屏加金色描边、接近直角，和输入卡片是同一种语言。设计尺寸 96 × 30 逻辑像素，字号按开方阻尼缩放，再乘显示器 DPI 得到窗口物理尺寸；贴近屏幕边缘时自动收进工作区。

> 关闭主窗口就是退出程序：划词图标是个常驻的隐藏窗口，所以 `main` 的关闭事件会显式走一遍收摊流程（停钩子 → 销毁图标窗口 → 退进程）。否则进程会一直挂着，钩子还在装、划词还会弹图标，而且单实例插件会把下一次启动当成重复启动直接吞掉。

### 取词方式

优先走 Windows UI Automation（`IUIAutomationTextPattern::GetSelection`），这是**纯只读**的，不碰剪贴板，也不会向目标程序发送任何输入。Chromium 把文本模式挂在文档节点上，所以读不到时会沿控件树向上找最多三层祖先。

读不到时才退到剪贴板兜底：先快照剪贴板 → 清空 → 模拟 `Ctrl+C` → 轮询 `CF_UNICODETEXT` → **把剪贴板原样还原**。先清空是为了区分「复制成功但内容为空」和「根本没复制」。含 GDI 句柄的格式（位图、元文件等）不参与还原，避免句柄失效。在资源管理器里这条兜底路径会被禁用，免得选中若干文件时被误触发成「复制文件」。若你此刻正按着 `Ctrl`，也不会发送合成的 `Ctrl+C`。

> 一次最多取 5000 个 Unicode 字符，超出部分截断。

### 资源管理器右键菜单

在文件或文件夹上点右键会多出一项 **翻译（translater）**，选中文本文件即可把内容读进本窗口翻译。支持 UTF-8（含 BOM）、UTF-16 LE/BE 与 GBK 编码，单个文件上限 512 KB。

菜单项写在 `HKCU\Software\Classes` 下（`*\shell\Translater` 与 `Directory\shell\Translater`），**只影响当前用户、不需要管理员权限**，取消勾选就整项删除。`MultiSelectModel=Single` 保证选中多个文件时只开一个窗口。Windows 11 默认把它折叠进「显示更多选项」，这是系统行为。

从菜单启动时若程序已在运行，会由单实例插件把命令行参数转交给已有窗口，不会开出第二个进程。

> **浏览器右键菜单做不了。** Chrome / Edge 的右键菜单是浏览器自己绘制的，Windows 没有提供任何注册表或 Shell 扩展机制能往里加条目，唯一的正规途径是安装浏览器扩展。所以浏览器里用「划词弹图标」；资源管理器的右键菜单则可以注册。

三项开关在「视图」菜单与设置对话框里，分别控制划词取词、剪贴板兜底、资源管理器右键菜单。

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

### 单词礼炮

点击窗口右上角、历史旁边的「礼炮」，查过的单词就会和彩纸、emoji 一起从底部四个位置同时喷出一波。保留旋转和重叠，标准字号下单词约 22–40px、emoji 约 30–54px，最多 232 个粒子；自然落出窗口后结束，不再绕屏循环。每次从本地历史中读取最近最多 128 个不重复的短词条，随机选取展示；它不受历史抽屉的分页和搜索影响，也不触发翻译或写入历史。词条以引擎的 `isWord` 标记为准，最长 32 个字符。

没有词条历史时仍有 emoji 礼炮，不会编造查过的词。动画约 3.6 秒自动结束，可重复点击重新播放；`Escape`、缩放窗口或切走页面会立即清理效果。尊重系统「减少动态效果」设置，改为静态词汇庆祝卡片。没有声音，不妨碍输入、选择或点击。

为改善掉帧，文字和 emoji 预先绘制为位图，动画帧只旋转和贴图，不再逐帧排字、描边。位图缓存采用最多 96 张 / 约 8 MB 像素的 LRU 上限，画布 DPR 上限 1.5，离屏粒子跳过绘制。可在启动 Vite 后运行 `node scripts/benchmark-confetti.mjs after`；增加 `--text-control` 可做同场景逐帧文字绘制的 A/B 对照。测试使用公开词汇与模拟 IPC，数据保存在 `test-results`，不读取真实历史。

### 历史与导出

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

不要直接把普通 `cargo build --release` 的输出当作正式版：Rust 的 release 优化并不代表 Tauri 已启用生产资源协议。缺少 `tauri/custom-protocol` 时，主窗口与划词悬浮窗口仍会连接 `127.0.0.1:1420`，关闭 Vite 后就会白屏、连接失败。构建脚本现在会拒绝这种 release 构建；上面的桌面构建入口会显式启用生产资源协议并先构建两个 HTML 入口。手动构建须先执行 `pnpm build`，再执行 `cargo build --manifest-path src-tauri/Cargo.toml --release --features tauri/custom-protocol`。

## 使用

- 输入或粘贴文本，停顿后自动翻译；可以在设置中关闭自动翻译。
- 菜单栏：`Alt` 聚焦菜单栏，`←` `→` 换菜单、`↑` `↓` 选条目、`Enter` 执行；鼠标滑过已展开的菜单标题会跟着切换。
- 快捷键：`Ctrl+Enter` 翻译，`Esc` 清空（菜单开着时先关菜单），`Ctrl+H` 历史，`F1` 说明。
- 点击互换按钮可将译文放入输入框并反向翻译。自动检测模式需要先取得一次结果。
- 点击译词、同义词或同根词继续查询；词典、释义、同义词、例句和扩展板块按接口返回的实际内容显示，没有数据的板块不会出现。
- 复制按钮复制完整译文。朗读优先播放引擎返回的 mp3（有道提供），没有音频时退回系统 SpeechSynthesis。
- 一次最多翻译 5000 个 Unicode 字符；长文请分段。
- 翻译结果缓存最长 24 小时，最多 2000 条，程序退出后清空。
- 在别的程序里划词后点浮出的 translate 图标，或对文本文件用右键的「翻译（translater）」，选中内容会直接进输入框并立即翻译。

## 设置文件

桌面设置保存在应用配置目录的 `settings.json`，Windows 通常为 `%APPDATA%\com.tauri-app.translater\settings.json`。启动时读取该文件，界面保存前会验证代理、语言对、引擎 id、历史策略、皮肤、字号与窗口尺寸。设置无效时保留上一份配置并显示原因；文件损坏时应用仍可打开，可通过设置重新保存。

```json
{
  "proxy": "",
  "autoTranslate": true,
  "autoSpeak": false,
  "langPair": "auto|zh-CN",
  "engine": "google",
  "historyMode": "all",
  "theme": "plain",
  "font": "md",
  "windowWidth": 1020,
  "windowHeight": 760,
  "rememberWindow": false,
  "selectTranslate": true,
  "clipboardFallback": true,
  "shellMenu": true
}
```

`selectTranslate` 控制划词取词（全局鼠标钩子），`clipboardFallback` 控制读不到 UI Automation 时是否用剪贴板兜底，`shellMenu` 控制资源管理器右键菜单是否注册。

`shellMenu` 只是意图，**界面上的勾选状态以注册表实际内容为准**：启动时会重新查一次注册表，你若在别处删掉了菜单项，勾会自动落下，避免显示与实际不符。

旧版本写下的设置文件缺少 `engine`、`historyMode`、`theme` 等字段时仍可正常加载，缺失字段会取默认值。窗口尺寸限制在 720×480 到 3840×2160 之间，越界会被拒绝（启动读取时则收敛到边界）。

留空代理使用 reqwest 的系统/环境代理检测；显式地址覆盖自动检测。支持 HTTP、HTTPS、SOCKS5、SOCKS5H。代理只影响 Google，有道的两个引擎始终直连。此文件可能包含代理凭据，请勿分享。浏览器预览的设置仅使用浏览器本地存储。

## 验证

运行 Rust 测试前请先关闭开发模式窗口与 `dev.bat` 进程，避免 Windows 锁定正在运行的 EXE。

```powershell
# TypeScript 类型检查及前端生产构建
pnpm build

# Rust 单元测试：设置、缓存、三个引擎的响应解析、历史库、导出、取词与右键菜单
pnpm test:rust

# 真实网络冒烟测试（默认 #[ignore]，需要显式打开）
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/desktop.ps1 -Action smoke

# 启动 Vite 后，在另一终端执行界面回归测试
pnpm dev
pnpm test:ui

# 关闭 Vite 和已有桌面实例后，验证正式 EXE 独立启动、双窗口资源、真实 IPC 与退出
pnpm test:desktop
```

冒烟测试覆盖引擎表与三个驱动的真实请求。Google 相关用例需要可达的网络或代理，其余用例国内直连即可通过。只跑有道：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test live_google -- --ignored --nocapture youdao
```

界面测试使用已安装的 Microsoft Edge 与模拟 Tauri IPC，共 167 项断言，覆盖引擎选择器、全部词典板块、历史抽屉（列表、搜索、删除、清空、回填）、导出流程、设置校验、皮肤与字号切换、窗口预设、菜单栏（点击 / 悬停 / 键盘导航 / 关闭）、说明对话框、划词取词与右键菜单开关、`translate-selection` 事件回填、`--translate-file` 冷启动与注册表状态同步、悬浮图标页渲染（字标、彩虹条占比、圆角与字号随高度缩放、换皮肤重绘）、异步竞态、错误恢复、语言互换、输入限制与键盘焦点。截图保存在 `test-results`（含 `meridian.png` 与 `meridian-compact.png`）。这些测试与真实网络测试分开执行。

> 划词取词依赖真实的桌面会话，无法在无头测试里跑通。回归测试只覆盖「后端发事件之后界面怎么做」以及图标页的渲染，钩子 → 图标 → 点击的完整往返需要手动验收。

设计与验证记录见 `design/index.html`。三个引擎都使用公开的非官方接口，服务可用性及词典字段由上游决定；HTTP 429 使用有限次数的退避重试，其他失败提供可恢复错误提示。

## 代码结构

- `src/theme.css`：设计令牌与皮肤。颜色 / 圆角 / 阴影 / 字号系数都在这里，`[data-theme]` 与 `[data-font]` 只覆盖变量。
- `src/styles.css`：布局与组件，不写死色值；`src/panels.css`：结果面板。
- `src/main.ts`：输入与交互状态、菜单栏、皮肤 / 字号 / 窗口、历史抽屉与导出；`src/render.ts`：结果与词典渲染。
- `src/settings.ts`、`src/api.ts`：设置与桌面命令访问。
- `src-tauri/src/lib.rs`：Tauri 命令层，把驱动、缓存、历史库与窗口操作暴露给前端。
- `src-tauri/src/settings.rs`：配置文件默认值、校验与读写。
- `src-tauri/src/translate`：驱动抽象（`driver.rs`）、数据模型（`model.rs`）、三个引擎实现、错误模型、TTL/LRU 缓存。
- `src-tauri/src/history.rs`：SQLite 历史库；`src-tauri/src/export.rs`：四种导出格式。
- `src-tauri/src/selection`：划词取词。`mod.rs` 编排、工作线程与退出收摊，`hook.rs` 全局鼠标钩子（含停钩子），`uia.rs` UI Automation 读选区，`clipboard.rs` 剪贴板兜底与还原，`bubble.rs` 悬浮图标窗口的尺寸、位置与外观。
- `src-tauri/src/shell_menu.rs`：资源管理器右键菜单的注册与卸载；`src-tauri/src/file_text.rs`：从菜单传入的文件路径读取文本并猜编码。
- `src/bubble.ts` / `bubble.html` / `src/bubble.css`：悬浮图标页，只渲染，点击由钩子接管。
- `scripts/desktop.ps1`：Windows 工具链检测、开发、测试与构建入口；`scripts/test-ui.mjs`：界面回归测试。
- `concept/meridian`：`meridian` 皮肤的设计稿来源（`DESIGN.md` 为权威色板与排印规范）。
