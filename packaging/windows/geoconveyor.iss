; Inno Setup 6 script. Build with:  iscc /DAppVersion=0.3.1 packaging\windows\geoconveyor.iss
; Expects target\release\geoconveyor.exe (cargo build --release on Windows).
#ifndef AppVersion
  #define AppVersion "0.3.1"
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

// terrio's Windows builds (pip wheel and standalone zip) need the Visual C++ runtime DLLs.
// GeoConveyor itself does not (it links the C runtime statically).
function VCRuntimeInstalled: Boolean;
var
  Installed: Cardinal;
begin
  Result := RegQueryDWordValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64', 'Installed', Installed) and (Installed = 1);
  if not Result then
    Result := FileExists(ExpandConstant('{sys}\vcruntime140.dll'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if (CurStep <> ssPostInstall) or WizardSilent then
    Exit;
  if not TerrioFound then
    MsgBox('GeoConveyor needs the terrio command-line tool, which was not found on this PC.' + #13#10 + #13#10 +
           'Install Python, then run:  pip install terrio' + #13#10 + #13#10 +
           'GeoConveyor will find it automatically, or you can point to it in Settings.', mbInformation, MB_OK);
  if not VCRuntimeInstalled then
    if MsgBox('terrio needs the Microsoft Visual C++ Redistributable (x64), which is not installed on this PC.' + #13#10 + #13#10 +
              'Download it from Microsoft now? Run the downloaded vc_redist.x64.exe, then start GeoConveyor.',
              mbConfirmation, MB_YESNO) = IDYES then
      ShellExec('open', 'https://aka.ms/vs/17/release/vc_redist.x64.exe', '', '', SW_SHOWNORMAL, ewNoWait, Code);
end;
