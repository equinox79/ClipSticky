; Windows 用のインストーラー(Inno Setup)。
; ビルド: iscc /DAppVersion=0.2.4 /DSourceExe=target\release\clipsticky.exe /DOutputDir=target\installer packaging\windows\clipsticky.iss
; 管理者権限は不要で、ユーザーごとに %LOCALAPPDATA%\Programs\ClipSticky に入る。

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\clipsticky.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\installer"
#endif

[Setup]
AppId={{17938838-41D2-4A51-A5F6-930497F0CE7F}
AppName=ClipSticky
AppVersion={#AppVersion}
AppPublisher=equinox79
AppPublisherURL=https://equinox79.github.io/ClipSticky/
AppSupportURL=https://github.com/equinox79/ClipSticky/issues
DefaultDirName={autopf}\ClipSticky
DefaultGroupName=ClipSticky
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir={#OutputDir}
OutputBaseFilename=ClipSticky-v{#AppVersion}-windows-x64-setup
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName=ClipSticky
WizardStyle=modern

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
japanese.AutoStart=Windows にログインしたときに、自動で起動する
english.AutoStart=Start automatically when I sign in to Windows

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"; Flags: unchecked
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "clipsticky.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\ClipSticky"; Filename: "{app}\clipsticky.exe"
Name: "{group}\{cm:UninstallProgram,ClipSticky}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\ClipSticky"; Filename: "{app}\clipsticky.exe"; Tasks: desktopicon
Name: "{userstartup}\ClipSticky"; Filename: "{app}\clipsticky.exe"; Tasks: autostart

[Run]
Filename: "{app}\clipsticky.exe"; Description: "{cm:LaunchProgram,ClipSticky}"; Flags: nowait postinstall skipifsilent

[Code]
// 起動中のまま上書き・削除できるように、先に終了させる(タスクバーに窓が無いので、再起動マネージャーに頼らない)
procedure StopApp();
var
  code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM clipsticky.exe', '', SW_HIDE, ewWaitUntilTerminated, code);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopApp();
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopApp();
end;
