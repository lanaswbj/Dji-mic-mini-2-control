//! Guided WinUSB driver install for the DJI receiver's control interface.
//!
//! Windows never auto-binds a user-mode driver to interface 6 (see
//! `protocol::models::mic_mini`'s `interface()`), so `nusb` fails to claim it
//! (`ErrorKind::Unsupported`) even though the device's audio interface works
//! fine and shows up as a normal microphone.
//!
//! A hand-rolled INF (a previous version of this file) can generate the
//! right driver package, but `pnputil /add-driver` refuses to install it on
//! any system with Code Integrity enforced, because the package has no
//! signed catalog file — and self-signing one isn't practical to do safely
//! from this app. Zadig solves exactly this problem and already ships an
//! Authenticode-signed release build, so instead of reinventing its
//! catalog-signing machinery, this downloads the official Zadig release,
//! verifies its signature, pre-configures it (via `zadig.ini`, a feature
//! Zadig itself reads on startup) to already list all devices in advanced
//! mode, launches it elevated for the user to pick the interface and click
//! Install, then deletes the downloaded copy once the window closes.

#[cfg(windows)]
const INSTALL_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:TEMP 'djimic-zadig'
New-Item -ItemType Directory -Force -Path $dir | Out-Null

try {
    $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/pbatard/libwdi/releases/latest' `
        -UseBasicParsing -Headers @{ 'User-Agent' = 'DJI-Mic-Control' }
    $asset = $release.assets | Where-Object { $_.name -match '^zadig-\d.*\.exe$' } | Select-Object -First 1
    if (-not $asset) {
        Write-Output 'NO_ASSET'
        exit 10
    }
    $exePath = Join-Path $dir $asset.name
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $exePath -UseBasicParsing
} catch {
    Write-Output "DOWNLOAD_FAILED: $_"
    exit 11
}

$sig = Get-AuthenticodeSignature -FilePath $exePath
if ($sig.Status -ne 'Valid') {
    Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
    Write-Output "BAD_SIGNATURE: $($sig.Status)"
    exit 12
}

# Zadig reads zadig.ini from its working directory on startup. This puts it
# straight into the state the user needs (advanced mode, so the interface
# number is visible; list-all, so our driverless interface shows up at all)
# instead of making them find those two menu toggles themselves.
$ini = @"
[general]
advanced_mode = true
exit_on_success = true
log_level = 1

[device]
list_all = true
include_hubs = false
trim_whitespaces = true

[driver]
default_driver = 0
"@
Set-Content -Path (Join-Path $dir 'zadig.ini') -Value $ini -Encoding ASCII

try {
    $p = Start-Process -FilePath $exePath -WorkingDirectory $dir -Verb RunAs -Wait -PassThru
    $code = $p.ExitCode
} catch {
    $code = 1223
}

Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
exit $code
"#;

#[cfg(windows)]
pub fn install() -> Result<(), String> {
    use std::process::Command;

    let script_path = std::env::temp_dir().join("djimic-install-driver.ps1");
    std::fs::write(&script_path, INSTALL_SCRIPT).map_err(|e| format!("无法写入安装脚本：{e}"))?;

    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script_path)
        .status();

    let _ = std::fs::remove_file(&script_path);

    let status = status.map_err(|e| format!("无法启动驱动安装向导：{e}"))?;

    match status.code() {
        Some(0) => Ok(()),
        Some(1223) => Err("已取消权限提升，驱动未安装。".into()),
        Some(10) => Err("无法从 GitHub 获取 Zadig 下载地址，请检查网络连接。".into()),
        Some(11) => Err("下载 Zadig 失败，请检查网络连接后重试。".into()),
        Some(12) => Err("下载的安装程序签名验证失败，出于安全考虑已取消安装。".into()),
        Some(code) => Err(format!("驱动安装向导异常退出（代码 {code}）。")),
        None => Err("安装进程被终止。".into()),
    }
}

#[cfg(not(windows))]
pub fn install() -> Result<(), String> {
    Err("此功能仅支持 Windows。".into())
}

#[tauri::command]
pub fn install_usb_driver() -> Result<(), String> {
    install()
}
