param([string]$Executable = "$PSScriptRoot\..\artifacts\tranSSlater.exe")
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TaskbarProperties {
    [StructLayout(LayoutKind.Sequential)] public struct Key { public Guid fmtid; public uint pid; }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Store {
        void GetCount(out uint count);
        void GetAt(uint index, out Key key);
        void GetValue(ref Key key, IntPtr value);
        void SetValue(ref Key key, IntPtr value);
        void Commit();
    }
    [DllImport("shell32.dll", PreserveSig=false)] static extern void SHGetPropertyStoreForWindow(IntPtr hwnd, ref Guid iid, out Store store);
    [DllImport("propsys.dll", PreserveSig=false)] static extern void PropVariantToStringAlloc(IntPtr value, out IntPtr text);
    [DllImport("ole32.dll")] static extern int PropVariantClear(IntPtr value);
    public static string Read(IntPtr hwnd, uint pid) {
        Guid iid = typeof(Store).GUID;
        Store store;
        SHGetPropertyStoreForWindow(hwnd, ref iid, out store);
        IntPtr value = Marshal.AllocCoTaskMem(24);
        for (int i = 0; i < 24; i++) Marshal.WriteByte(value, i, 0);
        try {
            Key key = new Key { fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = pid };
            store.GetValue(ref key, value);
            IntPtr text;
            PropVariantToStringAlloc(value, out text);
            try { return Marshal.PtrToStringUni(text); }
            finally { Marshal.FreeCoTaskMem(text); }
        } finally { PropVariantClear(value); Marshal.FreeCoTaskMem(value); Marshal.ReleaseComObject(store); }
    }
}
'@
$path = (Resolve-Path -LiteralPath $Executable).Path
if (Get-Process transslater -ErrorAction SilentlyContinue) { throw 'Close the running application before this test.' }
$process = Start-Process -FilePath $path -WindowStyle Hidden -PassThru
try {
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 200
        $process.Refresh()
        if ($process.HasExited) { throw 'Application exited before its main window appeared.' }
    } while ($process.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
    if ($process.MainWindowHandle -eq 0) { throw 'Main window did not appear.' }
    # Tauri creates the native window before running the setup callback.
    while ([TaskbarProperties]::Read($process.MainWindowHandle, 5) -eq '' -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
        if ($process.HasExited) { throw "Application exited during setup: $($process.ExitCode)" }
    }
    $expected = @{ 2 = '"' + $path + '"'; 3 = $path + ',0'; 4 = 'tranSSlater'; 5 = 'com.tauri-app.translater' }
    foreach ($pidValue in $expected.Keys) {
        $actual = [TaskbarProperties]::Read($process.MainWindowHandle, $pidValue)
        if ($actual -cne $expected[$pidValue]) { throw "Property $pidValue mismatch: $actual" }
        Write-Host "PASS: taskbar property $pidValue = $actual"
    }
    $second = Start-Process -FilePath $path -WindowStyle Hidden -PassThru
    if (-not $second.WaitForExit(10000)) { Stop-Process -Id $second.Id; throw 'Second launch did not forward to the existing instance.' }
    $process.Refresh()
    if ($process.HasExited) { throw 'Existing instance exited after relaunch.' }
    Write-Host 'PASS: relaunch keeps the existing instance running'
    $null = $process.CloseMainWindow()
    if (-not $process.WaitForExit(10000)) { throw 'Closing the main window did not exit the application.' }
    Write-Host 'PASS: closing the main window exits the application'
} finally {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id }
}
