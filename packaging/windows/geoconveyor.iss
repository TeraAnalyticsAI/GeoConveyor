; Inno Setup 6 script. Build with:  iscc /DAppVersion=0.3.0 packaging\windows\geoconveyor.iss
; Expects target\release\geoconveyor.exe (cargo build --release on Windows).
#ifndef AppVersion
  #define AppVersion "0.3.0"
#endif

[Setup]
AppId={{6F0B2C54-7D3E-4C1A-9B6E-2A5D8E1F4C73}
AppName=GeoConveyor
AppVersion={#AppVersion}
AppPublisher=Tera Analytics
DefaultDirName={autopf}\GeoConveyor
DefaultGroupName=GeoConveyor
UninstallDisplayIcon={app}\geoconveyor.ico
OutputDir=..\..\dist
OutputBaseFilename=GeoConveyor-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern
DisableProgramGroupPage=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\..\target\release\geoconveyor.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\geoconveyor.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\GeoConveyor"; Filename: "{app}\geoconveyor.exe"; IconFilename: "{app}\geoconveyor.ico"
Name: "{autodesktop}\GeoConveyor"; Filename: "{app}\geoconveyor.exe"; IconFilename: "{app}\geoconveyor.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\geoconveyor.exe"; Description: "Launch GeoConveyor"; Flags: nowait postinstall skipifsilent

[Code]
function TerrioFound: Boolean;
var
  Code: Integer;
begin
  Result := Exec(ExpandConstant('{cmd}'), '/C where terrio', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0);
  if not Result then
    Result := FileExists(ExpandConstant('{userappdata}\Python\Scripts\terrio.exe'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and (not WizardSilent) and (not TerrioFound) then
    MsgBox('GeoConveyor needs the terrio command-line tool, which was not found on this PC.' + #13#10 + #13#10 +
           'Install Python, then run:  pip install terrio' + #13#10 + #13#10 +
           'GeoConveyor will find it automatically, or you can point to it in Settings.', mbInformation, MB_OK);
end;
