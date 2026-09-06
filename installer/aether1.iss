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
AppId={{9B7B6C6E-7B3E-4A9B-9C7A-8A1E8B6A6F1E}
AppName=Aether1 Platform
AppVersion={#AppVersion}
AppPublisher=TridentSpoon
DefaultDirName={localappdata}\Aether1
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutDir}
OutputBaseFilename=Aether1-Setup
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
; (see src-tauri\src\llm\tts.rs / stt.rs) -- {%USERPROFILE%} is what home_dir() resolves to
; on Windows (paths.rs), so no code on the app side needs to know this installer exists.
Source: "{#StageDir}\models\{#PiperVoice}.onnx"; DestDir: "{%USERPROFILE%}\.local\share\piper\voices"; Flags: ignoreversion
Source: "{#StageDir}\models\{#PiperVoice}.onnx.json"; DestDir: "{%USERPROFILE%}\.local\share\piper\voices"; Flags: ignoreversion
Source: "{#StageDir}\models\ggml-{#WhisperModel}.bin"; DestDir: "{%USERPROFILE%}\.local\share\whisper"; Flags: ignoreversion

[Icons]
Name: "{group}\Aether1 Platform"; Filename: "{app}\aether1.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Aether1 Platform"; Filename: "{app}\aether1.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\aether1.exe"; Description: "Launch Aether1 now"; Flags: nowait postinstall skipifsilent

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
