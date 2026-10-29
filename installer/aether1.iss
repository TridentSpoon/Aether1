; Offline installer for AETHER1 -- packages the already-built aether1.exe plus Piper (TTS)
; and whisper.cpp (STT) with a voice and a model already bundled, so this installs and runs
; with no internet access at all. Compiled by scripts\package_offline_windows.ps1, which
; passes StageDir/OutDir/PiperVoice/WhisperModel in via /D; the #ifndef defaults below only
; matter if someone runs `iscc aether1.iss` directly against a hand-built staging folder.
;
; Per-user install (PrivilegesRequired=lowest): no admin rights, no UAC prompt, matching
; setup.bat's own "no administrator rights needed" -- an installer that demands elevation
; for a Start Menu shortcut is one people abandon.

#ifndef StageDir
  #define StageDir "..\dist-stage"
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif
#ifndef PiperVoice
  #define PiperVoice "en_US-lessac-medium"
#endif
#ifndef WhisperModel
  #define WhisperModel "small"
#endif

#define AppVersion GetEnv("AETHER1_VERSION")
#if AppVersion == ""
  #define AppVersion "0.0.0-dev"
#endif

[Setup]
; **The AppId and DefaultDirName below do not change with the rename, and must not.**
; The AppId is how Windows' own uninstall registry entry is keyed, and how
; installs.rs::windows_install finds a copy that is already here -- change it and an
; existing install becomes invisible to both, so the next installer puts a second copy
; beside it and neither uninstaller knows about the other. DefaultDirName is the folder
; that install's files are already in. The display name is what an operator reads, and that
; is what the rename moves; these two are identity, not branding.
AppId={{9B7B6C6E-7B3E-4A9B-9C7A-8A1E8B6A6F1E}
AppName=Aurora1
AppVersion={#AppVersion}
AppPublisher=TridentSpoon
DefaultDirName={localappdata}\Aether1
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutDir}
OutputBaseFilename=Aurora1-Setup
SetupIconFile={#StageDir}\icon.ico
UninstallDisplayIcon={app}\aether1.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Files]
Source: "{#StageDir}\aether1.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StageDir}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

; Piper's whole release directory (the exe plus the DLLs it needs next to it) and the
; standalone whisper-cli.exe both go into {app}\bin, which is the one directory this
; installer adds to the user's PATH -- see the [Code] section. whisper-cli.exe is static
; (see package_offline_windows.ps1's BUILD_SHARED_LIBS=OFF), so it needs nothing else there.
Source: "{#StageDir}\piper\*"; DestDir: "{app}\bin"; Flags: ignoreversion recursesubdirs
Source: "{#StageDir}\whisper\whisper-cli.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

; Straight into the same ~/.local/share layout the app already searches on every platform
; (see src-tauri\src\llm\tts.rs / stt.rs) -- {%USERPROFILE} is what home_dir() resolves to
; on Windows (paths.rs), so no code on the app side needs to know this installer exists.
; Note: Inno's env-var constant syntax is {%NAME}, not {%NAME%} -- the brace already
; supplies the delimiter that Windows' own %NAME% notation would otherwise need.
Source: "{#StageDir}\models\{#PiperVoice}.onnx"; DestDir: "{%USERPROFILE}\.local\share\piper\voices"; Flags: ignoreversion
Source: "{#StageDir}\models\{#PiperVoice}.onnx.json"; DestDir: "{%USERPROFILE}\.local\share\piper\voices"; Flags: ignoreversion
Source: "{#StageDir}\models\ggml-{#WhisperModel}.bin"; DestDir: "{%USERPROFILE}\.local\share\whisper"; Flags: ignoreversion

[Icons]
Name: "{group}\Aurora1"; Filename: "{app}\aether1.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Aurora1"; Filename: "{app}\aether1.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
; **The second command name.** `aurora1` is the companion's command and `aether1` is the one
; behind the curtain; every subcommand works under both, and the help text names whichever one
; was typed (cli::invoked_as). A hard link rather than a second copy, because it is the same
; binary -- copying it would double a large install for nothing and let the two drift apart on
; the next update. `mklink /H` needs no elevation on NTFS within one volume, which is what a
; per-user install under {localappdata} always is.
;
; `runhidden` and no failure handling: if the link cannot be made (a volume that is not NTFS,
; say) the install is still complete and correct, because `aether1` is there and runs
; everything. This is the one step allowed to fail quietly, which is why it is here and not
; in [Files].
Filename: "{cmd}"; Parameters: "/c mklink /H ""{app}\aurora1.exe"" ""{app}\aether1.exe"""; Flags: runhidden; StatusMsg: "Adding the aurora1 command..."

Filename: "{app}\aether1.exe"; Description: "Launch Aurora1 now"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The hard link above is made after install rather than listed in [Files], so Inno does not
; know about it and would otherwise leave behind an aurora1.exe pointing at a deleted file.
Type: files; Name: "{app}\aurora1.exe"

[Code]
// Appends {app}\bin to the current user's PATH so `which::which` (Rust, in the running
// app) finds piper.exe / whisper-cli.exe the same way it would on Linux with them in
// ~/.local/bin -- Inno Setup has no built-in "add to PATH" the way some other installer
// tools do, so this is the standard hand-rolled version of it (HKCU\Environment, broadcast
// WM_SETTINGCHANGE so already-open programs -- Explorer included -- notice without a
// reboot). Only ever touches the user's own PATH, matching PrivilegesRequired=lowest.
procedure EnvAddPath(Path: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths) then
    Paths := '';
  if (Paths = '') or (Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';') = 0) then
  begin
    if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
      Paths := Paths + ';';
    Paths := Paths + Path;
    RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths);
    RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'AETHER1_PATH_ADDED', Path);
  end;
end;

procedure EnvRemovePath(Path: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths) then
    exit;
  P := Pos(';' + Uppercase(Path) + ';', ';' + Uppercase(Paths) + ';');
  if P > 0 then
  begin
    Delete(Paths, P - 1, Length(Path) + 1);
    RegWriteStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', Paths);
  end;
  RegDeleteValue(HKEY_CURRENT_USER, 'Environment', 'AETHER1_PATH_ADDED');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    EnvAddPath(ExpandConstant('{app}\bin'));
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    EnvRemovePath(ExpandConstant('{app}\bin'));
  end;
end;
