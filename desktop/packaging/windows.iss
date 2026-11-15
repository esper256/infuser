#ifndef Build
  #define Build "preview"
#endif
[Setup]
AppId=InfuserDesktop
AppName=Infuser
AppVersion=0.1.0
AppPublisher=Infuser
DefaultDirName={localappdata}\Infuser\Desktop
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableWelcomePage=yes
DisableReadyPage=yes
ChangesAssociations=yes
OutputDir=..\..\dist
OutputBaseFilename=Infuser-Setup
Compression=lzma2
SolidCompression=yes
SetupIconFile=..\assets\infuser.ico
UninstallDisplayIcon={app}\{#Build}\Infuser.exe

[Files]
Source: "..\target\release\infuser-desktop.exe"; DestDir: "{app}\{#Build}"; DestName: "Infuser.exe"; Flags: ignoreversion

[Registry]
Root: HKCU; Subkey: "Software\Classes\infuser"; ValueType: string; ValueData: "URL:Infuser Protocol"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\infuser"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\infuser\DefaultIcon"; ValueType: string; ValueData: "{app}\{#Build}\Infuser.exe,0"
Root: HKCU; Subkey: "Software\Classes\infuser\shell\open\command"; ValueType: string; ValueData: """{app}\{#Build}\Infuser.exe"" ""%1"""

[Icons]
Name: "{userprograms}\Infuser"; Filename: "{app}\{#Build}\Infuser.exe"

[Run]
Filename: "{app}\{#Build}\Infuser.exe"; Description: "Open Infuser"; Flags: nowait postinstall skipifsilent

[Messages]
FinishedLabel=Infuser is ready. Return to your Library and press Play.

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
