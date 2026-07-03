; Futureboard Studio Installer
; Generated for Inno Setup 6.x
; Source build output:
;   ..\..\target\release\*.exe
;   ..\..\target\release\*.dll
;
; Install targets:
;   Per-user:  %LOCALAPPDATA%\Programs\Futureboard Studio
;   All-users: %ProgramFiles%\Futureboard Studio

#define MyAppName "Futureboard Studio"
#define MyAppPublisher "Futureboard"
#define MyAppExeName "FutureboardNative.exe"
#define MyAppIcon "..\..\packages\shared\app\icons\icon.ico"
#define MySourceDir "..\..\target\release"

#ifndef MyAppVersion
#define MyAppVersion "2026.6.17"
#endif

[Setup]
AppId={{9A56EFD0-B65D-4A48-9B0F-F6214A69F001}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={code:GetDefaultDir}
DefaultGroupName={#MyAppName}
SetupIconFile={#MyAppIcon}
UninstallDisplayIcon={app}\{#MyAppExeName}
OutputDir=..\..\target\installer
OutputBaseFilename=FutureboardStudioSetup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
UsePreviousSetupType=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
AllowNoIcons=yes
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
Name: "fileassoc_apak"; Description: "Associate .apak packages with APAK Installer"; GroupDescription: "File associations:"; Flags: checkedonce

[Files]
; Main app + helper executables
Source: "{#MySourceDir}\*.exe"; DestDir: "{app}"; Flags: ignoreversion

; Runtime / engine / bridge DLLs
Source: "{#MySourceDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"

Name: "{group}\APAK Installer"; Filename: "{app}\apakinstaller.exe"; WorkingDir: "{app}"; IconFilename: "{app}\apakinstaller.exe"; Check: FileExists(ExpandConstant('{app}\apakinstaller.exe'))

Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"

Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; .apak file association.
; This is per-user when running non-elevated and HKLM when elevated by install mode.
Root: HKA; Subkey: "Software\Classes\.apak"; ValueType: string; ValueName: ""; ValueData: "Futureboard.APAK"; Flags: uninsdeletevalue; Tasks: fileassoc_apak
Root: HKA; Subkey: "Software\Classes\Futureboard.APAK"; ValueType: string; ValueName: ""; ValueData: "Futureboard Audio Package"; Flags: uninsdeletekey; Tasks: fileassoc_apak
Root: HKA; Subkey: "Software\Classes\Futureboard.APAK\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\apakinstaller.exe,0"; Tasks: fileassoc_apak
Root: HKA; Subkey: "Software\Classes\Futureboard.APAK\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\apakinstaller.exe"" ""%1"""; Tasks: fileassoc_apak

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallDelete]
Type: filesandordirs; Name: "{app}\logs"

[Code]
function GetDefaultDir(Param: string): string;
begin
  if IsAdminInstallMode then
    Result := ExpandConstant('{autopf}\Futureboard Studio')
  else
    Result := ExpandConstant('{localappdata}\Programs\Futureboard Studio');
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
end;
