; sidebar Inno Setup script (v0.1.0)
; Builds sidebar-setup.exe: installs sidebar-app.exe + sidebar-monitor-svc.exe +
; sidebar-monitor-host.exe + LHM runtime to %PROGRAMFILES%\Sidebar.
; Service registration is DISABLED (commented out in [Run]) until the
; named-pipe consumer lands. Creates Start Menu + optional desktop shortcut.
;
; Build locally:
;   ISCC.exe installer\sidebar.iss
; Output lands at:
;   dist\sidebar-setup.exe
;
; The release workflow (.github/workflows/release.yml) invokes this with
; /DAppVersion=<version> so the Add/Remove Programs entry reflects the
; release tag. Without /D, the literal AppVersion below is used.

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppName=Sidebar
AppVersion={#AppVersion}
AppPublisher=ravibaskaran
AppPublisherURL=https://github.com/ravibaskaran/win11-diagnostics
AppSupportURL=https://github.com/ravibaskaran/win11-diagnostics/issues
DefaultDirName={commonpf}\Sidebar
DefaultGroupName=Sidebar
UninstallDisplayIcon={app}\sidebar-app.exe
Compression=lzma2
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename=sidebar-setup
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
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
; The complete LHM runtime (managed DLLs, config, locales).
; Excludes: debug symbols (.pdb) and XML docs — not needed by end users.
Source: "..\resources\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "sidebar-monitor-host\*, *.pdb, *.xml"

[Icons]
Name: "{group}\Sidebar"; Filename: "{app}\sidebar-app.exe"
Name: "{commondesktop}\Sidebar"; Filename: "{app}\sidebar-app.exe"; Tasks: desktopicon

[Run]
; Service registration is DISABLED until Epic 15/16 pipe path is wired end-to-end.
; The app uses the HTTP-to-LHM path (OhmSupervisor::launch_elevated) which works
; without a service. Re-enable these lines when the named-pipe consumer lands.
; Filename: "{sys}\sc.exe"; Parameters: "create sidebar-monitor-svc binPath= ""{app}\sidebar-monitor-svc.exe"" start= auto"; Flags: runhidden
; Filename: "{sys}\sc.exe"; Parameters: "start sidebar-monitor-svc"; Flags: runhidden
; Launch sidebar
Filename: "{app}\sidebar-app.exe"; Description: "Launch Sidebar"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Service cleanup — disabled until service registration is re-enabled.
; Filename: "{sys}\sc.exe"; Parameters: "stop sidebar-monitor-svc"; Flags: runhidden; RunOnceId: "StopService"
; Filename: "{sys}\sc.exe"; Parameters: "delete sidebar-monitor-svc"; Flags: runhidden; RunOnceId: "DeleteService"

[Code]
function IsServiceInstalled(const name: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := ShellExec('open', 'sc.exe', 'query ' + name, '', SW_HIDE, ewWaitUntilTerminated, ResultCode) and (ResultCode = 0);
end;

function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
begin
  // If a prior version is installed, stop the service before overwrite.
  if IsServiceInstalled('sidebar-monitor-svc') then
  begin
    ShellExec('open', 'sc.exe', 'stop sidebar-monitor-svc', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  end;
  Result := True;
end;
