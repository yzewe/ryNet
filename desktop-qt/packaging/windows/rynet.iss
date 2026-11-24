#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Src
  #define Src "..\..\..\..\build\stage\ryNet"
#endif
#ifndef Out
  #define Out "..\..\..\..\out"
#endif

[Setup]
AppId={{6D3CF723-5E4D-4F5F-B44D-FB1F5669DC88}
AppName=ryNet
AppVersion={#AppVersion}
AppVerName=ryNet {#AppVersion}
AppPublisher=yzewe
AppPublisherURL=https://github.com/yzewe/ryNet
VersionInfoVersion={#AppVersion}
VersionInfoProductName=ryNet
DefaultDirName={autopf}\ryNet
DefaultGroupName=ryNet
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Out}
OutputBaseFilename=ryNet-{#AppVersion}-setup
SetupIconFile=rynet.ico
UninstallDisplayIcon={app}\rynet.exe
UninstallDisplayName=ryNet
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern dynamic
WizardSizePercent=100
WizardImageFile=wizard-100.bmp,wizard-125.bmp,wizard-150.bmp,wizard-175.bmp,wizard-200.bmp,wizard-250.bmp
WizardSmallImageFile=small-100.bmp,small-125.bmp,small-150.bmp,small-175.bmp,small-200.bmp,small-250.bmp
CloseApplications=force
RestartApplications=no
ShowLanguageDialog=auto
LanguageDetectionMethod=uilanguage

[Languages]
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
ru.RemoveData=Удалить и настройки ryNet (подписки, журналы)?
en.RemoveData=Also remove ryNet settings (subscriptions, logs)?
ru.WelcomeText=Установка ryNet {#AppVersion} — VPN-клиента с режимом «Весь трафик», правилами и подписками.%n%nНажмите «Далее», чтобы продолжить.
en.WelcomeText=This will install ryNet {#AppVersion} — a VPN client with full-tunnel mode, rules and subscriptions.%n%nClick Next to continue.

[Messages]
ru.WelcomeLabel2={cm:WelcomeText}
en.WelcomeLabel2={cm:WelcomeText}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[InstallDelete]
Type: filesandordirs; Name: "{app}\app"
Type: filesandordirs; Name: "{app}\runtime"
Type: filesandordirs; Name: "{app}\qml"
Type: filesandordirs; Name: "{app}\platforms"
Type: filesandordirs; Name: "{app}\tls"
Type: filesandordirs; Name: "{app}\styles"
Type: filesandordirs; Name: "{app}\imageformats"
Type: filesandordirs; Name: "{app}\generic"
Type: filesandordirs; Name: "{app}\networkinformation"
Type: filesandordirs; Name: "{app}\iconengines"

[Files]
Source: "{#Src}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\ryNet"; Filename: "{app}\rynet.exe"
Name: "{autodesktop}\ryNet"; Filename: "{app}\rynet.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\rynet"; ValueType: string; ValueData: "URL:ryNet"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\rynet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\rynet\DefaultIcon"; ValueType: string; ValueData: "{app}\rynet.exe,0"
Root: HKCU; Subkey: "Software\Classes\rynet\shell\open\command"; ValueType: string; ValueData: """{app}\rynet.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\.rnp"; ValueType: string; ValueData: "ryNet.Plugin"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.rnp"; ValueType: string; ValueName: "Content Type"; ValueData: "application/vnd.rynet.plugin"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\ryNet.Plugin"; ValueType: string; ValueData: "ryNet plugin"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\ryNet.Plugin\DefaultIcon"; ValueType: string; ValueData: "{app}\rynet.exe,0"
Root: HKCU; Subkey: "Software\Classes\ryNet.Plugin\shell\open\command"; ValueType: string; ValueData: """{app}\rynet.exe"" ""%1"""

[Run]
Filename: "{app}\rynet.exe"; Description: "{cm:LaunchProgram,ryNet}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
procedure StopRyNet();
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /T /IM rynet.exe', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(1500);
end;

{ Копия из MSI (прежняя jpackage — «rynet», нынешняя — «ryNet», обе в %LOCALAPPDATA%\rynet) — удалить тихо,
  чтобы не было двух копий. }
procedure RemoveOldMsi(Root: Integer);
var
  Keys: TArrayOfString;
  I, Code: Integer;
  Name, Base: String;
begin
  Base := 'Software\Microsoft\Windows\CurrentVersion\Uninstall';
  if not RegGetSubkeyNames(Root, Base, Keys) then Exit;
  for I := 0 to GetArrayLength(Keys) - 1 do
    if (Length(Keys[I]) = 38) and (Keys[I][1] = '{') and
       RegQueryStringValue(Root, Base + '\' + Keys[I], 'DisplayName', Name) and (CompareText(Name, 'rynet') = 0) then
      if Root = HKCU then
        Exec(ExpandConstant('{sys}\msiexec.exe'), '/x ' + Keys[I] + ' /qn /norestart', '', SW_HIDE, ewWaitUntilTerminated, Code)
      else
        ShellExec('runas', ExpandConstant('{sys}\msiexec.exe'), '/x ' + Keys[I] + ' /qn /norestart', '', SW_HIDE, ewWaitUntilTerminated, Code);
  if FileExists(ExpandConstant('{localappdata}\rynet\rynet.exe')) then
  begin
    DelTree(ExpandConstant('{localappdata}\rynet\app'), True, True, True);
    DelTree(ExpandConstant('{localappdata}\rynet\runtime'), True, True, True);
    DeleteFile(ExpandConstant('{localappdata}\rynet\rynet.exe'));
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRyNet();
  RemoveOldMsi(HKCU);
  RemoveOldMsi(HKLM);
  Result := '';
end;

{ Автозапуск, включённый в прежней версии, — на новый путь программы. }
procedure CurStepChanged(CurStep: TSetupStep);
var
  Key: String;
begin
  Key := 'Software\Microsoft\Windows\CurrentVersion\Run';
  if (CurStep = ssPostInstall) and RegValueExists(HKCU, Key, 'ryNet') then
    RegWriteStringValue(HKCU, Key, 'ryNet', '"' + ExpandConstant('{app}\rynet.exe') + '" --autostart');
end;

{ «Весь трафик»: ядро в Program Files и задача планировщика ставятся с правами — снять их
  тоже с правами (один запрос UAC), только если они есть. }
procedure RemoveElevated();
var
  Code: Integer;
  Cmd: String;
begin
  if not DirExists(ExpandConstant('{commonpf64}\ryNet')) then Exit;
  Cmd := '-NoProfile -WindowStyle Hidden -Command "Get-Process rynet-core -ErrorAction SilentlyContinue | Stop-Process -Force; ' +
    'Unregister-ScheduledTask -TaskPath ''\ryNet\'' -TaskName Core -Confirm:$false -ErrorAction SilentlyContinue; ' +
    'netsh advfirewall firewall delete rule name=''ryNet core'' | Out-Null; ' +
    'Remove-Item -Recurse -Force ''' + ExpandConstant('{commonpf64}\ryNet') + '''"';
  ShellExec('runas', 'powershell.exe', Cmd, '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    StopRyNet();
    RemoveElevated();
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    if (not UninstallSilent) and (MsgBox(CustomMessage('RemoveData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      DelTree(ExpandConstant('{userappdata}\rynet'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\rynet\geo'), True, True, True);
    end;
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'ryNet');
  end;
end;
