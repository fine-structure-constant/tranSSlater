param([string]$SdkRoot, [string]$JavaHome)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$signing = Join-Path $root 'signing'
$artifacts = Join-Path $root 'artifacts'
New-Item -ItemType Directory -Force -Path $signing, $artifacts | Out-Null
$keystore = Join-Path $signing 'transslater.jks'
$passwordFile = Join-Path $signing 'password.txt'
if (-not (Test-Path -LiteralPath $keystore)) {
    if (-not (Test-Path -LiteralPath $passwordFile)) {
        $bytes = New-Object byte[] 32
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        $rng.GetBytes($bytes)
        $rng.Dispose()
        [IO.File]::WriteAllText($passwordFile, [Convert]::ToBase64String($bytes))
    }
    & (Join-Path $JavaHome 'bin\keytool.exe') -genkeypair -keystore $keystore -storetype JKS -alias transslater -keyalg RSA -keysize 3072 -validity 10000 -dname 'CN=tranSSlater Local, O=tranSSlater, C=CN' -storepass:file $passwordFile -keypass:file $passwordFile
    if ($LASTEXITCODE -ne 0) { throw 'APK signing key creation failed.' }
}
$buildTools = Get-ChildItem (Join-Path $SdkRoot 'build-tools') -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
$unsigned = Get-ChildItem (Join-Path $root 'src-tauri\gen\android\app\build\outputs\apk') -Recurse -Filter '*-unsigned.apk' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $unsigned) { throw 'No unsigned release APK found.' }
$aligned = Join-Path $artifacts 'transslater-aligned.apk'
$output = Join-Path $artifacts 'tranSSlater-arm64.apk'
& (Join-Path $buildTools.FullName 'zipalign.exe') -f -P 16 4 $unsigned.FullName $aligned
if ($LASTEXITCODE -ne 0) { throw 'APK alignment failed.' }
& (Join-Path $buildTools.FullName 'apksigner.bat') sign --ks $keystore --ks-key-alias transslater --ks-pass "file:$passwordFile" --out $output $aligned
if ($LASTEXITCODE -ne 0) { throw 'APK signing failed.' }
& (Join-Path $buildTools.FullName 'apksigner.bat') verify --verbose $output
if ($LASTEXITCODE -ne 0) { throw 'APK signature verification failed.' }
Remove-Item -LiteralPath $aligned
Write-Host "Installable APK: $output"
Get-FileHash -LiteralPath $output -Algorithm SHA256 | Format-List
