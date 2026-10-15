param(
    [ValidateSet('dev', 'build', 'test', 'check', 'smoke')]
    [string]$Action = 'dev',
    [switch]$NoBundle,
    [ValidatePattern('^[A-Za-z0-9_-]+\.exe$')]
    [string]$ArtifactName = 'tranSSlater.exe'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $projectRoot

try {
    $configPath = Join-Path $projectRoot 'tools.local.json'
    $toolConfig = @{}
    if (Test-Path -LiteralPath $configPath) {
        $toolConfig = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    }
    foreach ($toolDir in @($toolConfig.nodeDir, $toolConfig.rustDir)) {
        if ($toolDir) { $env:PATH = "$toolDir;$env:PATH" }
    }
    if (-not (Get-Command node -ErrorAction SilentlyContinue) -and (Test-Path 'D:\.ENV\nodejs\node.exe')) {
        $env:PATH = "D:\.ENV\nodejs;$env:PATH"
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        foreach ($candidate in @((Join-Path $env:USERPROFILE '.cargo\bin'), 'D:\.ENV\Rust\bin')) {
            if (Test-Path (Join-Path $candidate 'cargo.exe')) { $env:PATH = "$candidate;$env:PATH"; break }
        }
    }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) { throw 'Node.js is missing. Install Node.js 22 LTS or configure nodeDir in tools.local.json.' }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'Rust is missing. Install the MSVC Rust toolchain or configure rustDir in tools.local.json.' }
    if (-not (Test-Path 'node_modules\@tauri-apps\cli\tauri.js')) { throw 'Dependencies are missing. Run pnpm install first.' }
    $msvcSetup = $toolConfig.msvcSetup
    if (-not $msvcSetup -and -not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        if (Test-Path -LiteralPath $vswhere) {
            $vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
            if ($vsPath) { $msvcSetup = Join-Path $vsPath 'VC\Auxiliary\Build\vcvars64.bat' }
        }
        if (-not $msvcSetup -and (Test-Path 'D:\.ENV\MSVC\setup_x64.bat')) { $msvcSetup = 'D:\.ENV\MSVC\setup_x64.bat' }
    }
    if ($msvcSetup) {
        if (-not (Test-Path -LiteralPath $msvcSetup)) { throw 'Configured MSVC setup script does not exist.' }
        $envLines = & $env:ComSpec /d /s /c "`"`"$msvcSetup`" >nul && set`""
        if ($LASTEXITCODE -ne 0) { throw 'MSVC environment initialization failed.' }
        foreach ($line in $envLines) {
            if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) { throw 'MSVC C++ Build Tools are missing. Install Desktop development with C++ or configure msvcSetup.' }
    if ($toolConfig.cargoTargetDir) { $env:CARGO_TARGET_DIR = $toolConfig.cargoTargetDir }
    elseif (-not $env:CARGO_TARGET_DIR -and (Test-Path 'D:\.ENV\cargo-target\translater-msvc')) { $env:CARGO_TARGET_DIR = 'D:\.ENV\cargo-target\translater-msvc' }
    if ($env:CARGO_TARGET_DIR) { Write-Host "Build directory: $env:CARGO_TARGET_DIR" }
    switch ($Action) {
        'test' { & cargo test --manifest-path src-tauri/Cargo.toml }
        'check' { & cargo check --manifest-path src-tauri/Cargo.toml }
        'smoke' { & cargo test --manifest-path src-tauri/Cargo.toml --test live_google -- --ignored --nocapture --test-threads=1 }
        'dev' { & node node_modules/@tauri-apps/cli/tauri.js dev }
        'build' {
            if ($NoBundle) { & node node_modules/@tauri-apps/cli/tauri.js build --no-bundle --features tauri/custom-protocol }
            else { & node node_modules/@tauri-apps/cli/tauri.js build --features tauri/custom-protocol }
        }
    }
    $resultCode = $LASTEXITCODE
    if ($resultCode -ne 0) { throw "The $Action command failed (exit $resultCode). See the error above." }
    if ($Action -eq 'build') {
        $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $projectRoot 'src-tauri\target' }
        Write-Host "Executable: $targetDir\release\transslater.exe"
        Write-Host "Installers: $targetDir\release\bundle"
        $artifactDir = Join-Path $projectRoot 'artifacts'
        New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null
        Copy-Item -LiteralPath (Join-Path $targetDir 'release\transslater.exe') -Destination (Join-Path $artifactDir $ArtifactName) -Force
        Write-Host "Ready to run: $artifactDir\$ArtifactName"
    }
    exit 0
} catch {
    Write-Host "[ERROR] $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
