# Creates Start Menu and Desktop shortcuts for the built Aether1 executable.
#
# The Linux counterpart of this is scripts/install_desktop_app.sh, which writes a .desktop
# file. Windows had no equivalent at all, which is why the app built and ran but never
# appeared in the Start Menu.
#
# Called from setup.bat. Kept as a .ps1 rather than inlined into the batch file because
# quoting a COM call through cmd.exe is a reliable way to produce a shortcut that silently
# points at nothing.

param(
    [Parameter(Mandatory = $true)][string]$RepoRoot
)

$ErrorActionPreference = 'Stop'

$exe = Join-Path $RepoRoot 'src-tauri\target\release\aether1.exe'
$icon = Join-Path $RepoRoot 'src-tauri\icons\icon.ico'

if (-not (Test-Path $exe)) {
    Write-Host "  The executable is not there yet ($exe) -- build it first." -ForegroundColor Yellow
    exit 1
}

function New-AetherShortcut {
    param([string]$Path)

    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    $shortcut.TargetPath = $exe
    # So the app starts in its own checkout, which is where its backend/ data directory and
    # the frontend it serves both live.
    $shortcut.WorkingDirectory = $RepoRoot
    $shortcut.Description = 'Aether1 -- holographic AI companion'
    if (Test-Path $icon) {
        $shortcut.IconLocation = $icon
    }
    $shortcut.Save()
}

# Each shortcut is attempted on its own. GetFolderPath returns an empty string for a
# special folder that isn't there, and a profile with a redirected or missing Desktop
# should still get a Start Menu entry -- so one failure reports itself and the other is
# still tried, rather than the first one aborting the script.
$made = 0
foreach ($target in @(
    @{ Label = 'Start Menu'; Folder = 'Programs' },
    @{ Label = 'Desktop';    Folder = 'Desktop'  }
)) {
    # The per-user folders, not the machine-wide ones: these need no administrator
    # rights, and an install that demands elevation for a shortcut is one people abandon.
    $dir = [Environment]::GetFolderPath($target.Folder)
    if ([string]::IsNullOrWhiteSpace($dir) -or -not (Test-Path $dir)) {
        Write-Host "  $($target.Label): skipped -- Windows reports no $($target.Folder) folder for this profile." -ForegroundColor Yellow
        continue
    }

    $path = Join-Path $dir 'Aether1.lnk'
    try {
        New-AetherShortcut -Path $path
        Write-Host "  $($target.Label): $path"
        $made++
    } catch {
        Write-Host "  $($target.Label): could not be created -- $($_.Exception.Message)" -ForegroundColor Yellow
    }
}

if ($made -eq 0) {
    Write-Host "  No shortcuts were created." -ForegroundColor Yellow
    exit 1
}
