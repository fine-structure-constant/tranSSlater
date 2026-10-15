param(
    [ValidateSet('init', 'dev', 'build', 'test', 'check', 'gradle-check', 'devices', 'package', 'status', 'sign')]
    [string]$Action = 'build',
    [string[]]$Target = @('aarch64'),
    [switch]$Debug
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $projectRoot

try {
    $config = Get-Content -LiteralPath (Join-Path $projectRoot 'tools.local.json') -Raw | ConvertFrom-Json
    foreach ($toolDir in @($config.nodeDir, $config.rustDir)) {
        if ($toolDir) { $env:PATH = "$toolDir;$env:PATH" }
    }
    $sdkRoot = $config.androidSdk
    $jdkRoot = $config.javaHome
    foreach ($dir in @($sdkRoot, $jdkRoot)) {
        if (-not (Test-Path -LiteralPath $dir)) { throw "Tool directory not found: $dir. Edit tools.local.json." }
    }
    $ndkDir = Get-ChildItem -LiteralPath (Join-Path $sdkRoot 'ndk') -Directory |
        Sort-Object { [version]$_.Name } | Select-Object -Last 1
    if (-not $ndkDir) { throw 'Android NDK is missing.' }

    # Host build scripts need MSVC even when compiling for Android.
    $envLines = & $env:ComSpec /d /s /c "`"`"$($config.msvcSetup)`" >nul && set`""
    if ($LASTEXITCODE -ne 0) { throw 'MSVC initialization failed.' }
    foreach ($line in $envLines) {
        if ($line -match '^([^=]+)=(.*)$') {
            [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
        }
    }
    $env:ANDROID_HOME = $sdkRoot
    $env:ANDROID_SDK_ROOT = $sdkRoot
    $env:NDK_HOME = $ndkDir.FullName
    $env:JAVA_HOME = $jdkRoot
    $llvmBin = Join-Path $ndkDir.FullName 'toolchains\llvm\prebuilt\windows-x86_64\bin'
    $env:PATH = "$llvmBin;$($config.rustDir);$($config.nodeDir);$jdkRoot\bin;$env:PATH"
    $env:CARGO_TARGET_DIR = $config.cargoTargetDir
    $env:GRADLE_USER_HOME = $config.gradleHome
    $env:CARGO_BUILD_JOBS = '4'
    $gradleOptions = @('--no-daemon', '--console=plain')
    if ($config.gradleProxyHost -and $config.gradleProxyPort) {
        foreach ($protocol in @('http', 'https')) {
            $gradleOptions += "-D$protocol.proxyHost=$($config.gradleProxyHost)"
            $gradleOptions += "-D$protocol.proxyPort=$($config.gradleProxyPort)"
        }
    }
    Write-Host "SDK: $sdkRoot"
    Write-Host "NDK: $($ndkDir.FullName)"
    Write-Host "JDK: $jdkRoot"
    Write-Host "Cargo target: $env:CARGO_TARGET_DIR"

    $tauri = 'node_modules\@tauri-apps\cli\tauri.js'
    if (-not (Test-Path -LiteralPath $tauri)) { throw 'Run pnpm install first.' }
    switch ($Action) {
        'init'  { & node $tauri android init --ci }
        'dev'   { & node $tauri android dev }
        'test'  { & cargo test --manifest-path src-tauri/Cargo.toml }
        'check' { & cargo check --manifest-path src-tauri/Cargo.toml }
        'devices' { & (Join-Path $sdkRoot 'platform-tools\adb.exe') devices -l }
        'sign' { & (Join-Path $PSScriptRoot 'package-apk.ps1') -SdkRoot $sdkRoot -JavaHome $jdkRoot }
        'status' {
            Get-Process rustc,java -ErrorAction SilentlyContinue | Format-Table Id,ProcessName,CPU,StartTime -AutoSize
            $global:LASTEXITCODE = 0
        }
        'gradle-check' {
            Push-Location 'src-tauri/gen/android'
            try { & .\gradlew.bat @gradleOptions help }
            finally { Pop-Location }
        }
        'package' {
            $jniDir = 'src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a'
            New-Item -ItemType Directory -Force -Path $jniDir | Out-Null
            Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'aarch64-linux-android/release/libtransslater_lib.so') -Destination $jniDir -Force
            Push-Location 'src-tauri/gen/android'
            try { & .\gradlew.bat @gradleOptions assembleArm64Release -x :app:rustBuildArm64Release -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64 }
            finally { Pop-Location }
        }
        'build' {
            if (-not (Test-Path 'src-tauri\gen\android\gradlew.bat')) {
                & node $tauri android init --ci
                if ($LASTEXITCODE -ne 0) { throw 'Android initialization failed.' }
            }
            Copy-Item -Path 'src-tauri/icons/android/*' -Destination 'src-tauri/gen/android/app/src/main/res' -Recurse -Force
            $cliArgs = @('scripts/native-build.mjs')
            foreach ($abi in $Target) { $cliArgs += @('--target', $abi) }
            if ($Debug) { $cliArgs += '--debug' }
            & node @cliArgs
            if ($LASTEXITCODE -ne 0 -and -not $Debug -and $Target.Count -eq 1 -and $Target[0] -eq 'aarch64') {
                $buildLog = Get-Content 'test-results/tauri-build.log' -Raw
                if ($buildLog -match 'Creation symbolic link is not allowed') {
                    Write-Host 'Native compilation succeeded. Using a copied library because Windows does not allow symlinks.'
                    $jniDir = 'src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a'
                    New-Item -ItemType Directory -Force -Path $jniDir | Out-Null
                    Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR 'aarch64-linux-android/release/libtransslater_lib.so') -Destination $jniDir -Force
                    $sdkPath = $sdkRoot.Replace('\', '/')
                    [IO.File]::WriteAllText((Join-Path $projectRoot 'src-tauri/gen/android/local.properties'), "sdk.dir=$sdkPath`n")
                    Push-Location 'src-tauri/gen/android'
                    try {
                        & .\gradlew.bat @gradleOptions assembleArm64Release -x :app:rustBuildArm64Release -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64
                    } finally { Pop-Location }
                }
            }
        }
    }
    if ($LASTEXITCODE -ne 0) { throw "The $Action command failed (exit $LASTEXITCODE)." }
    if ($Action -in @('build', 'package')) {
        Get-ChildItem 'src-tauri\gen\android\app\build\outputs\apk' -Recurse -Filter '*.apk' |
            ForEach-Object { Write-Host "APK: $($_.FullName)" }
        if (-not $Debug -and $Target.Count -eq 1 -and $Target[0] -eq 'aarch64') {
            & (Join-Path $PSScriptRoot 'package-apk.ps1') -SdkRoot $sdkRoot -JavaHome $jdkRoot
        }
    }
    exit 0
} catch {
    Write-Host "[ERROR] $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
