//! Finds Xbox App / PC Game Pass games registered for the current Windows user.
//!
//! Xbox games are not exposed through a Steam-like library file. The Windows
//! shell already has the authoritative app registration, so the Windows
//! implementation asks PowerShell for Start-menu AUMIDs and joins them with
//! their AppX package locations. `MicrosoftGame.config` keeps ordinary Store
//! apps out of this source while retaining packaged Xbox titles.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct XboxGame {
    /// Stable Wervik id. The AUMID is the Windows identity used to launch it.
    pub id: String,
    pub name: String,
    pub aumid: String,
    pub install_dir: PathBuf,
    pub logo_path: Option<PathBuf>,
}

#[cfg(windows)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PowerShellXboxGame {
    name: String,
    aumid: String,
    install_location: String,
    logo_path: Option<String>,
}

#[cfg(windows)]
const POWERSHELL_SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# These are Xbox infrastructure and Store UI packages, not games. The
# MicrosoftGame.config check below does the main classification work.
$excludedNames = @(
  'Microsoft.GamingApp',
  'Microsoft.GamingServices',
  'Microsoft.XboxApp',
  'Microsoft.XboxIdentityProvider',
  'Microsoft.Xbox.TCUI',
  'Microsoft.XboxGamingOverlay',
  'Microsoft.XboxGameOverlay',
  'Microsoft.WindowsStore',
  'Microsoft.StorePurchaseApp'
)

$packages = @{}
Get-AppxPackage -PackageTypeFilter Main | Where-Object {
  -not $_.IsFramework -and $_.InstallLocation -and (Test-Path -LiteralPath $_.InstallLocation)
} | ForEach-Object {
  $packages[[string]$_.PackageFamilyName] = $_
}

$rows = @()
foreach ($entry in @(Get-StartApps)) {
  $aumid = [string]$entry.AppID
  if ([string]::IsNullOrWhiteSpace($aumid) -or $aumid -notmatch '!') { continue }

  $family = $aumid.Split('!')[0]
  if (-not $packages.ContainsKey($family)) { continue }
  $package = $packages[$family]
  if ($excludedNames -contains [string]$package.Name) { continue }

  # GDK titles distributed by the Xbox app carry this file. Check Content as
  # well because some Xbox installations keep the title package one level
  # below the registered package location.
  $configPath = $null
  foreach ($candidate in @(
    (Join-Path $package.InstallLocation 'MicrosoftGame.config'),
    (Join-Path $package.InstallLocation 'Content\MicrosoftGame.config')
  )) {
    if (Test-Path -LiteralPath $candidate) {
      $configPath = $candidate
      break
    }
  }
  if (-not $configPath) { continue }

  $logoPath = $null
  $manifestPath = Join-Path $package.InstallLocation 'AppxManifest.xml'
  if (Test-Path -LiteralPath $manifestPath) {
    try {
      [xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
      $appId = $aumid.Substring($aumid.IndexOf('!') + 1)
      $app = @($manifest.Package.Applications.Application |
        Where-Object { [string]$_.Id -eq $appId }) | Select-Object -First 1
      if ($null -eq $app) {
        $app = @($manifest.Package.Applications.Application) | Select-Object -First 1
      }
      $visual = @($app.ChildNodes |
        Where-Object { $_.LocalName -eq 'VisualElements' }) | Select-Object -First 1
      $relativeLogo = [string]$visual.Square150x150Logo
      if ([string]::IsNullOrWhiteSpace($relativeLogo)) {
        $relativeLogo = [string]$visual.Logo
      }
      if (-not [string]::IsNullOrWhiteSpace($relativeLogo)) {
        $candidateLogo = Join-Path $package.InstallLocation $relativeLogo
        if (Test-Path -LiteralPath $candidateLogo) {
          $logoPath = (Resolve-Path -LiteralPath $candidateLogo).Path
        }
      }
    } catch {
      # A missing or unusual manifest must not hide an otherwise launchable
      # game. It simply appears with Wervik's normal placeholder art.
    }
  }

  $rows += [pscustomobject]@{
    name = [string]$entry.Name
    aumid = $aumid
    installLocation = [string]$package.InstallLocation
    logoPath = $logoPath
  }
}

$jsonRows = @($rows | Sort-Object -Property name, aumid -Unique)
if ($jsonRows.Count -eq 0) {
  '[]'
} else {
  ConvertTo-Json -InputObject $jsonRows -Compress
}
"#;

#[cfg(windows)]
pub fn scan() -> Vec<XboxGame> {
    let output = run_powershell(POWERSHELL_SCRIPT);
    let Ok(output) = output else {
        return Vec::new();
    };

    let Ok(records) = serde_json::from_str::<Vec<PowerShellXboxGame>>(&output) else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    records
        .into_iter()
        .filter_map(|record| {
            let name = record.name.trim().to_string();
            let aumid = record.aumid.trim().to_string();
            let install_dir = PathBuf::from(record.install_location);
            if name.is_empty() || aumid.is_empty() || !install_dir.is_dir() {
                return None;
            }

            let id = format!("xbox:{}", aumid.to_ascii_lowercase());
            if !seen.insert(id.clone()) {
                return None;
            }

            Some(XboxGame {
                id,
                name,
                aumid,
                install_dir,
                logo_path: record.logo_path.map(PathBuf::from),
            })
        })
        .collect()
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<String, String> {
    use std::process::Command;

    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .or_else(|_| {
            Command::new("pwsh.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    script,
                ])
                .output()
        })
        .map_err(|e| format!("Could not run PowerShell: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| format!("PowerShell returned invalid UTF-8: {e}"))
}

#[cfg(not(windows))]
pub fn scan() -> Vec<XboxGame> {
    Vec::new()
}
