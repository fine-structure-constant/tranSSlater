# Translater 桌面翻译器

Windows 桌面应用，使用 Tauri 2、Rust 与 TypeScript。支持自动检测语言、中英日韩法德俄西翻译、词典释义、同义词、例句、朗读和内存缓存。

## 直接运行

双击 `artifacts/Translater.exe`。这个文件包含界面资源，不需要启动 Vite、Node.js 或 Rust。

运行环境需要 Microsoft Edge WebView2 Runtime（Windows 10/11 通常已安装）。翻译需要联网访问 Google，若连接失败，可在右上角「设置」中填写已有代理，如 `http://127.0.0.1:7890` 或 `socks5h://127.0.0.1:1080`。代理端口必须与自己的代理软件一致。

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
- `Ctrl+Enter` 立即翻译，`Escape` 清空；中文输入法组词期间不触发翻译。
- 点击互换按钮可将译文放入输入框并反向翻译。自动检测模式需要先取得一次结果。
- 点击译词或同义词继续查询；词典、释义、同义词和例句按接口返回的实际内容显示。
- 复制按钮复制完整译文。朗读使用系统 SpeechSynthesis，语音可用性取决于安装的语言语音包。
- 一次最多翻译 5000 个 Unicode 字符；长文请分段。
- 缓存按原文及语言对区分，最长 24 小时，最多 2000 条，程序退出后清空。

## 设置文件

桌面设置保存在应用配置目录的 `settings.json`，Windows 通常为 `%APPDATA%\com.tauri-app.translater\settings.json`。启动时读取该文件，界面保存前会验证代理和语言对。设置无效时保留上一份配置并显示原因；文件损坏时应用仍可打开，可通过设置重新保存。

```json
{
  "proxy": "",
  "autoTranslate": true,
  "autoSpeak": false,
  "langPair": "auto|zh-CN"
}
```

留空代理使用 reqwest 的系统/环境代理检测；显式地址覆盖自动检测。支持 HTTP、HTTPS、SOCKS5、SOCKS5H。此文件可能包含代理凭据，请勿分享。浏览器预览的设置仅使用浏览器本地存储。

## 验证

运行 Rust 测试前请先关闭开发模式窗口与 `dev.bat` 进程，避免 Windows 锁定正在运行的 EXE。

```powershell
# TypeScript 类型检查及前端生产构建
pnpm build

# 配置读写、Google 响应解析与缓存回归测试
pnpm test:rust

# 真实网络测试（使用公开示例文本，无需密钥）
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/desktop.ps1 -Action smoke

# 启动 Vite 后，在另一终端执行界面回归测试
pnpm dev
pnpm test:ui
```

界面测试使用已安装的 Microsoft Edge 与模拟 Tauri IPC，验证隐藏状态、标签重复切换、异步竞态、错误恢复、设置校验、语言互换、清空计时器、输入限制和键盘焦点。截图保存在 `test-results`。这些测试与真实 Google 网络测试分开执行。

设计与验证记录见 `design/index.html`。Google 使用公开的非官方接口，服务可用性及词典字段由上游决定；HTTP 429 使用有限次数的退避重试，其他失败提供可恢复错误提示。

## 代码结构

- `src/main.ts`：输入与交互状态；`src/render.ts`：结果与词典渲染。
- `src/settings.ts`、`src/api.ts`：设置与桌面命令访问。
- `src-tauri/src/settings.rs`：配置文件默认值、校验与读写。
- `src-tauri/src/translate`：Google 协议客户端、解析、错误模型、TTL/LRU 缓存。
- `scripts/desktop.ps1`：Windows 工具链检测、开发、测试与构建入口。
