//! Give portable builds the same taskbar relaunch information as installed builds.
use std::os::windows::ffi::OsStrExt;
use tauri::Manager;
use windows::{
    Win32::{
        Foundation::PROPERTYKEY,
        Storage::EnhancedStorage::{
            PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
            PKEY_AppUserModel_RelaunchDisplayNameResource, PKEY_AppUserModel_RelaunchIconResource,
        },
        System::{Com::StructuredStorage::PROPVARIANT, Variant::VT_LPWSTR},
        UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
    },
    core::PWSTR,
};

pub fn configure(window: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let mut command: Vec<u16> = vec![b'"' as u16];
    command.extend(executable.as_os_str().encode_wide());
    command.push(b'"' as u16);
    let mut icon: Vec<u16> = executable.as_os_str().encode_wide().collect();
    icon.extend(",0".encode_utf16());
    let id: Vec<u16> = window
        .app_handle()
        .config()
        .identifier
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "tranSSlater".encode_utf16().collect();

    // Only the main window is pinnable. The selection bubble remains a tool window.
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(window.hwnd()?)?;
        set_string(&store, &PKEY_AppUserModel_RelaunchCommand, command)?;
        set_string(&store, &PKEY_AppUserModel_RelaunchDisplayNameResource, name)?;
        set_string(&store, &PKEY_AppUserModel_RelaunchIconResource, icon)?;
        // Set identity last, after the complete relaunch metadata is available.
        set_string(&store, &PKEY_AppUserModel_ID, id)?;
        store.Commit()?;
    }
    Ok(())
}

unsafe fn set_string(
    store: &IPropertyStore,
    key: &PROPERTYKEY,
    mut value: Vec<u16>,
) -> windows::core::Result<()> {
    value.push(0);
    let mut variant = std::mem::ManuallyDrop::new(PROPVARIANT::default());
    // SetValue copies the string. This variant borrows the Vec and must not be cleared
    // with PropVariantClear, which would try to free memory owned by Rust.
    unsafe {
        (*variant.Anonymous.Anonymous).vt = VT_LPWSTR;
        (*variant.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(value.as_mut_ptr());
        store.SetValue(key, &*variant)
    }
}
