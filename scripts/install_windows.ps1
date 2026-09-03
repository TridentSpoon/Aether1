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

# The per-user Start Menu, not the machine-wide one: this needs no administrator rights,
# and an install that demands elevation for a shortcut is an install people abandon.
$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Aether1.lnk'
New-AetherShortcut -Path $startMenu
Write-Host "  Start Menu:  $startMenu"

$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Aether1.lnk'
New-AetherShortcut -Path $desktop
Write-Host "  Desktop:     $desktop"
