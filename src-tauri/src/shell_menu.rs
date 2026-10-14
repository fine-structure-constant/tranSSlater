//! Windows 资源管理器右键菜单项。
//!
//! 浏览器自己画的右键菜单第三方改不了（只能靠浏览器扩展），但资源管理器的可以。
//! 键写在 `HKCU\Software\Classes` 下，**只影响当前用户、不需要管理员权限**，关掉
//! 开关或用卸载程序就能整棵删掉。
//!
//! 注意 Windows 11 会把经典菜单折叠进「显示更多选项」，这是系统行为，不是没注册成功。

use std::path::Path;

/// 菜单上显示的文字。加后缀是为了和系统自带项区分开。
pub const LABEL: &str = "翻译（translater）";

const FILE_BASE: &str = r"Software\Classes\*\shell\Translater";
const DIR_BASE: &str = r"Software\Classes\Directory\shell\Translater";

/// 传进来的参数名，`lib.rs` 里解析。
pub const ARG: &str = "--translate-file";

/// 注册（或刷新）菜单项。每次启动都会调一次，这样程序被挪了位置也能自动纠正路径。
pub fn register(exe: &Path) -> Result<(), String> {
    let exe = exe.to_string_lossy();
    let command = format!("\"{exe}\" {ARG} \"%1\"");
    let icon = format!("\"{exe}\",0");

    for base in [FILE_BASE, DIR_BASE] {
        let key = windows_registry::CURRENT_USER
            .create(base)
            .map_err(|e| format!("写入注册表失败（{base}）：{e}"))?;
        key.set_string("", LABEL)
            .map_err(|e| format!("写入菜单标题失败：{e}"))?;
        key.set_string("Icon", icon.as_str())
            .map_err(|e| format!("写入菜单图标失败：{e}"))?;
        // 多选时只调一次，否则选中 10 个文件会弹出 10 个窗口
        let _ = key.set_string("MultiSelectModel", "Single");

        let command_key = windows_registry::CURRENT_USER
            .create(format!("{base}\\command"))
            .map_err(|e| format!("写入注册表失败（{base}\\command）：{e}"))?;
        command_key
            .set_string("", command.as_str())
            .map_err(|e| format!("写入菜单命令失败：{e}"))?;
    }

    Ok(())
}

/// 删除菜单项。本来就不存在时也算成功。
pub fn unregister() -> Result<(), String> {
    for base in [FILE_BASE, DIR_BASE] {
        if windows_registry::CURRENT_USER.open(base).is_err() {
            continue;
        }
        windows_registry::CURRENT_USER
            .remove_tree(base)
            .map_err(|e| format!("删除注册表项失败（{base}）：{e}"))?;
    }
    Ok(())
}

/// 菜单项当前是否已经注册。
pub fn is_registered() -> bool {
    windows_registry::CURRENT_USER.open(FILE_BASE).is_ok()
        && windows_registry::CURRENT_USER.open(DIR_BASE).is_ok()
}

/// 按设置同步：开就注册，关就删除。
pub fn apply(enabled: bool) -> Result<(), String> {
    if !enabled {
        return unregister();
    }
    let exe = std::env::current_exe().map_err(|e| format!("取不到程序路径：{e}"))?;
    register(&exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_is_distinguishable_from_system_entries() {
        assert!(LABEL.contains("translater"));
        assert_ne!(LABEL, "翻译");
    }

    #[test]
    fn arg_is_a_recognizable_flag() {
        assert!(ARG.starts_with("--"));
        assert_eq!(ARG, "--translate-file");
    }
}
