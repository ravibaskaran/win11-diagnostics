; Story 16.3 — Inno Setup script for sidebar v1.0.0
; Builds sidebar-setup.exe: installs sidebar-app.exe + sidebar-monitor-svc.exe +
; sidebar-monitor-host.exe + LHM runtime to %PROGRAMFILES%\sidebar.
; Registers + starts the Windows Service. Creates Start Menu shortcut.
;
; Cited: Story 16.3, T-47, G19.

[Setup]
AppName=sidebar
AppVersion=1.0.0
AppPublisher=ravibaskaran
AppPublisherURL=https://github.com/ravibaskaran/win11-diagnostics
AppSupportURL=https://github.com/ravibaskaran/win11-diagnostics/issues
DefaultDirName={pf}\sidebar
DefaultGroupName=sidebar
UninstallDisplayIcon={app}\sidebar-app.exe
Compression=lzma2
SolidCompression=yes
OutputDir=..
OutputBaseFilename=sidebar-setup
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
LicenseFile=LICENSE.rtf
; winget compatibility: Scope: machine (see winget-cli #254)

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional icons:"

[Files]
; The build artifacts (produced by release.yml build stage)
Source: "..\target\x86_64-pc-windows-msvc\release\sidebar-app.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\x86_64-pc-windows-msvc\release\sidebar-monitor-svc.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\resources\sidebar-monitor-host\sidebar-monitor-host.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\resources\LibreHardwareMonitorLib.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\resources\LibreHardwareMonitor.LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
; The complete LHM runtime (managed DLLs, config, locales)
Source: "..\resources\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "sidebar-monitor-host\*"

[Icons]
Name: "{group}\sidebar"; Filename: "{app}\sidebar-app.exe"
Name: "{commondesktop}\sidebar"; Filename: "{app}\sidebar-app.exe"; Tasks: desktopicon

[Run]
; Register + start the service
Filename: "{sys}\sc.exe"; Parameters: "create sidebar-monitor-svc binPath= ""{app}\sidebar-monitor-svc.exe"" start= auto"; Flags: runhidden
Filename: "{sys}\sc.exe"; Parameters: "start sidebar-monitor-svc"; Flags: runhidden
; Launch sidebar
Filename: "{app}\sidebar-app.exe"; Description: "Launch sidebar"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Stop + delete the service before uninstalling files
Filename: "{sys}\sc.exe"; Parameters: "stop sidebar-monitor-svc"; Flags: runhidden; RunOnceId: "StopService"
Filename: "{sys}\sc.exe"; Parameters: "delete sidebar-monitor-svc"; Flags: runhidden; RunOnceId: "DeleteService"

[Code]
function IsServiceInstalled(const name: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := ShellExec('open', 'sc.exe', 'query ' + name, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0);
end;

function InitializeSetup(): Boolean;
begin
  // If a prior version is installed, stop the service before overwrite.
  if IsServiceInstalled('sidebar-monitor-svc') then
  begin
    ShellExec('open', 'sc.exe', 'stop sidebar-monitor-svc', '', SW_HIDE, ewWaitUntilTerminated, 0);
  end;
  Result := True;
end;
