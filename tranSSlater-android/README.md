# tranSSlater · Android

本目录是独立 Android 工程，使用 Tauri 2、Rust、SQLite 和 TypeScript。桌面工程在上一级目录。

## 保留的功能

- Google、有道词典、有道翻译三个引擎，以及词典、例句、音频、翻译缓存。
- 无上限本地历史库：保存完整翻译结果，不按日期或条数自动清理。分页、搜索、历史回填、单条删除和主动清空保留。
- CSV、Markdown、Anki、完整 JSON 全量导出；导出不受当前搜索条件或已加载分页影响。
- 简约与远点两个皮肤、四档字号、设置和单词礼炮。

不包含桌面划词、悬浮窗口、资源管理器右键菜单、窗口尺寸预设或最大化。旧窗口设置字段仅用于兼容数据，不调用窗口 API。

## 构建

首次安装依赖、复制本机工具配置：

```powershell
npm ci
Copy-Item tools.example.json tools.local.json
# 在 tools.local.json 中配置 SDK、JDK、Rust、Node、MSVC 和缓存位置
```

构建 arm64 Release 安装包：

```powershell
npm run android:build
```

主入口 `scripts/android.ps1` 兼容 Windows PowerShell 5.1，并初始化宿主 MSVC 与目标 NDK 环境。使用本机已安装的 Android Rust 标准库，无需 rustup。

Gradle 不会自动采用 Windows 系统代理。需要代理访问依赖仓库时，在 `tools.local.json` 设置 `gradleProxyHost` 和 `gradleProxyPort`；它们仅用于构建。项目使用本机缓存的 Gradle 9.7.0。

Windows 未开放符号链接时，脚本只在原生编译成功、明确遇到符号链接权限错误后复制生成的 `.so`，再用 Gradle 组装 arm64 APK。编译错误不会被这个分支忽略。默认构建 arm64；其他 ABI 需安装对应工具链并允许 Tauri 所需的符号链接。

安装包输出：`artifacts/tranSSlater-arm64.apk`。要求 Android 7.0 / API 24 及以上，arm64-v8a。

脚本使用本地发布证书签名并调用 `apksigner verify`。`signing/` 中的密钥和密码不纳入版本控制；请保留并备份，后续使用同一证书才能覆盖升级并保留应用数据。

```powershell
adb install -r artifacts/tranSSlater-arm64.apk
```

## 历史与导出

SQLite `history.db` 和 `settings.json` 位于 Android 应用私有目录，实际位置可从「说明 → 数据位置」查看。覆盖升级保留数据。主动清空、系统清除应用数据或卸载会移除历史。

点击「历史 → 导出」，通过 Android 系统文件选择器选择保存位置。Rust 使用 Android `content://` 文档描述符流式写入全部记录，无需读取整个数据库到内存，也无需广泛存储权限。取消文件选择不会写入导出文件。JSON 保存完整词典与翻译结果，适合备份。

桌面端重命名保持原应用标识，因此既有 Windows 设置和历史仍使用原数据目录。两个平台的数据库各自独立。

## 图标与界面

`app-icon.png` 是 1024px 母版：深底、A／文字形与蓝、橄榄、金、橙四段彩虹条。Android 启动器使用安全区内的前景、纯色背景及圆形资源，生成 Gradle 工程后脚本自动同步图标。

`src/mobile.css` 提供安全区、动态视口、44px 触控区域、窄屏菜单、历史抽屉和设置布局。设计及验证记录见 `design/index.html`。

## 验证

```powershell
npm run build
npm run test:rust
```

父工程 `scripts/test-android.mjs` 使用 Edge Chromium 触屏模拟与 IPC 测试数据，覆盖 320px、390px、横屏与平板视口。它验证布局和前端交互，不替代实际 Android 设备验证。Rust 历史测试额外验证 1,205 条记录在关闭并重开数据库后完整保留，并可全量遍历导出。

Google 在大陆通常需要代理，有道引擎支持直连。引擎仍使用原项目的公开接口。
