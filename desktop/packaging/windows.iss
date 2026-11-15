#ifndef Build
  #define Build "preview"
#endif
[Setup]
AppId=InfuserDesktop
AppName=Infuser Play
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
OutputBaseFilename=Infuser-Play-Setup
Compression=lzma2
SolidCompression=yes
SetupIconFile=..\assets\infuser.ico
UninstallDisplayIcon={app}\{#Build}\InfuserPlay.exe

[Files]
Source: "..\target\release\infuser-desktop.exe"; DestDir: "{app}\{#Build}"; DestName: "InfuserPlay.exe"; Flags: ignoreversion
Source: "..\assets\fonts\OFL.txt"; DestDir: "{app}\{#Build}\licenses"; DestName: "NotoSans-OFL.txt"; Flags: ignoreversion

[Registry]
Root: HKCU; Subkey: "Software\Classes\infuser"; ValueType: string; ValueData: "URL:Infuser Play Protocol"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\infuser"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\infuser\DefaultIcon"; ValueType: string; ValueData: "{app}\{#Build}\InfuserPlay.exe,0"
Root: HKCU; Subkey: "Software\Classes\infuser\shell\open\command"; ValueType: string; ValueData: """{app}\{#Build}\InfuserPlay.exe"" ""%1"""

[Icons]
Name: "{userprograms}\Infuser Play"; Filename: "{app}\{#Build}\InfuserPlay.exe"

[InstallDelete]
Type: files; Name: "{userprograms}\Infuser.lnk"

[Run]
Filename: "{app}\{#Build}\InfuserPlay.exe"; Description: "Open Infuser Play"; Flags: nowait postinstall skipifsilent

[Messages]
FinishedLabel=Infuser Play is ready. Return to Infuser in your browser and press Play.

[UninstallDelete]
Type: filesandordirs; Name: "{app}"
