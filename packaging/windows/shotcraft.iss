; ShotCraft for Windows: the installer (Inno Setup 6). scripts\windows\make-installer.ps1 builds it.
;
; It installs for the current user without administrator rights (/ALLUSERS on the command line
; installs for everyone), lists ShotCraft under Open with for pictures, and checks the one thing
; ShotCraft needs that a PC may lack: Windows' own text recognition for the user's language. When
; that is missing it asks, and installs it on a yes.
;   /TEXTRECOGNITION=install   installs it without asking (unattended installs)
;   /TEXTRECOGNITION=skip      never asks

#ifndef AppVersion
  #error Pass the version: ISCC /DAppVersion=1.2.3 shotcraft.iss
#endif
#define Root "..\.."

[Setup]
; Names this app to Windows for upgrades and uninstalling: never change it.
AppId={{90999C30-F67A-4490-AE8B-82287BE89D1B}
AppName=ShotCraft
AppVersion={#AppVersion}
AppVerName=ShotCraft {#AppVersion}
AppPublisher=ShotCraft
AppPublisherURL=https://github.com/DailyXplorer/ShotCraft
AppSupportURL=https://github.com/DailyXplorer/ShotCraft/issues
AppUpdatesURL=https://github.com/DailyXplorer/ShotCraft/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\ShotCraft
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
DisableWelcomePage=no
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
; ShotCraft hides its window when asked to close, so Setup stops it itself (PrepareToInstall).
CloseApplications=no
ChangesAssociations=yes
WizardStyle=modern
SetupIconFile={#Root}\assets\shotcraft.ico
UninstallDisplayIcon={app}\shotcraft.exe
UninstallDisplayName=ShotCraft
Compression=lzma2/max
SolidCompression=yes
OutputDir={#Root}\target\dist
OutputBaseFilename=ShotCraft-{#AppVersion}-Setup

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"

[Messages]
en.FinishedLabel=ShotCraft is installed.%n%nIt runs in the notification area, next to the clock. On Windows 11 its icon may be behind the ^ arrow: drag it onto the taskbar to keep it in sight.%n%nCtrl+Shift+2 captures an area, Ctrl+Shift+1 the whole screen.
en.FinishedLabelNoIcons=ShotCraft is installed.%n%nIt runs in the notification area, next to the clock. On Windows 11 its icon may be behind the ^ arrow: drag it onto the taskbar to keep it in sight.%n%nCtrl+Shift+2 captures an area, Ctrl+Shift+1 the whole screen.
fr.FinishedLabel=ShotCraft est installé.%n%nIl se trouve dans la zone de notification, à côté de l'horloge. Sous Windows 11, son icône peut être cachée derrière la flèche ^ : faites-la glisser sur la barre des tâches pour la garder sous les yeux.%n%nCtrl+Maj+2 capture une zone, Ctrl+Maj+1 tout l'écran.
fr.FinishedLabelNoIcons=ShotCraft est installé.%n%nIl se trouve dans la zone de notification, à côté de l'horloge. Sous Windows 11, son icône peut être cachée derrière la flèche ^ : faites-la glisser sur la barre des tâches pour la garder sous les yeux.%n%nCtrl+Maj+2 capture une zone, Ctrl+Maj+1 tout l'écran.

[CustomMessages]
en.StillRunning=ShotCraft is running. Setup closes it to update it: a picture you are editing and have not saved will be lost.%n%nContinue?
fr.StillRunning=ShotCraft est en cours d'exécution. L'installation le ferme pour le mettre à jour : une image en cours de modification et non enregistrée sera perdue.%n%nContinuer ?
en.NotClosed=ShotCraft is still running. Quit it from its icon next to the clock, then run Setup again.
fr.NotClosed=ShotCraft est toujours en cours d'exécution. Quittez-le depuis son icône à côté de l'horloge, puis relancez l'installation.
en.OcrMissing=Windows has no text recognition installed for your language.%n%nShotCraft uses it to read the text in a picture (Recognize Text/QR, Blur Text). Install it now?%n%nWindows asks for administrator rights and downloads it. This can take a few minutes.
fr.OcrMissing=Windows n'a pas de reconnaissance de texte installée pour votre langue.%n%nShotCraft s'en sert pour lire le texte d'une image (Recognize Text/QR, Blur Text). L'installer maintenant ?%n%nWindows demande les droits d'administrateur et la télécharge. Cela peut prendre quelques minutes.
en.OcrInstalling=Installing Windows' text recognition. This can take a few minutes...
fr.OcrInstalling=Installation de la reconnaissance de texte de Windows. Cela peut prendre quelques minutes...
en.OcrInstalled=Text recognition is installed.
fr.OcrInstalled=La reconnaissance de texte est installée.
en.OcrFailed=Windows could not install text recognition. Everything else in ShotCraft works.%n%nTo add it later: Settings > Time & language > Language & region, choose your language > Language options > Optical character recognition.
fr.OcrFailed=Windows n'a pas pu installer la reconnaissance de texte. Tout le reste de ShotCraft fonctionne.%n%nPour l'ajouter plus tard : Paramètres > Heure et langue > Langue et région, choisissez votre langue > Options linguistiques > Reconnaissance optique de caractères.
en.RemoveSettings=Remove your ShotCraft settings too?
fr.RemoveSettings=Supprimer aussi vos réglages ShotCraft ?

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Root}\target\release\shotcraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#Root}\assets\fonts\OFL.txt"; DestDir: "{app}\licenses"; DestName: "DM Sans (OFL).txt"; Flags: ignoreversion
Source: "{#Root}\assets\fonts\OFL-Noto.txt"; DestDir: "{app}\licenses"; DestName: "Noto Sans Symbols (OFL).txt"; Flags: ignoreversion
Source: "{#Root}\assets\icons\LICENSE-hugeicons.md"; DestDir: "{app}\licenses"; DestName: "Hugeicons (MIT).md"; Flags: ignoreversion
Source: "ocr.ps1"; Flags: dontcopy

[Icons]
Name: "{autoprograms}\ShotCraft"; Filename: "{app}\shotcraft.exe"
Name: "{autodesktop}\ShotCraft"; Filename: "{app}\shotcraft.exe"; Tasks: desktopicon

[Registry]
; Open with: Explorer lists ShotCraft for these pictures. It does not become their default app.
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "ShotCraft"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\shell\open\command"; ValueType: string; ValueData: """{app}\shotcraft.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".png"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".jpg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".jpeg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".gif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".bmp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".tif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".tiff"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\shotcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".webp"; ValueData: ""

[Run]
Filename: "{app}\shotcraft.exe"; Description: "{cm:LaunchProgram,ShotCraft}"; Flags: nowait postinstall skipifsilent
; An unattended update of a ShotCraft that was running starts it again, in the tray.
Filename: "{app}\shotcraft.exe"; Parameters: "--background"; Flags: nowait runasoriginaluser; Check: RestartsAfterSilentUpdate

[Code]
var
  WasRunning: Boolean;

// The copies of ShotCraft that run from Exe, counted, and ended when Stop is set.
function RunningCopies(const Exe: String; Stop: Boolean): Integer;
var
  Locator, Services, Processes, Process: Variant;
  I: Integer;
begin
  Result := 0;
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Services := Locator.ConnectServer('.', 'root\CIMV2');
    Processes := Services.ExecQuery('SELECT * FROM Win32_Process WHERE Name = "shotcraft.exe"');
    for I := 0 to Processes.Count - 1 do
    begin
      Process := Processes.ItemIndex(I);
      // Windows gives no path for another user's process: that copy is not ours to stop.
      if not VarIsNull(Process.ExecutablePath) and (CompareText(Process.ExecutablePath, Exe) = 0) then
      begin
        Result := Result + 1;
        if Stop then
          Process.Terminate();
      end;
    end;
  except
    // No answer from Windows about its processes: Setup goes on, and says so if a file is in use.
  end;
end;

procedure StopRunningCopies(const Exe: String);
var
  Tries: Integer;
begin
  RunningCopies(Exe, True);
  // Windows frees shotcraft.exe a moment after its process ends.
  Tries := 0;
  while (RunningCopies(Exe, False) > 0) and (Tries < 50) do
  begin
    Sleep(100);
    Tries := Tries + 1;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Exe: String;
begin
  Result := '';
  Exe := ExpandConstant('{app}\shotcraft.exe');
  WasRunning := RunningCopies(Exe, False) > 0;
  if not WasRunning then
    Exit;
  if SuppressibleMsgBox(CustomMessage('StillRunning'), mbConfirmation, MB_OKCANCEL, IDOK) <> IDOK then
  begin
    Result := CustomMessage('NotClosed');
    Exit;
  end;
  StopRunningCopies(Exe);
end;

function RestartsAfterSilentUpdate(): Boolean;
begin
  Result := WasRunning and WizardSilent;
end;

// Asks Windows whether it can read text in the user's language, and offers to install that when
// it cannot. Only a clear "no" is acted on: a PC where the check does not run is left alone.
procedure OfferTextRecognition();
var
  Choice, PowerShell, Script, Params, ListFile: String;
  Languages: AnsiString;
  Code: Integer;
begin
  Choice := Lowercase(ExpandConstant('{param:TEXTRECOGNITION|ask}'));
  if Choice = 'skip' then
    Exit;
  PowerShell := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
  ExtractTemporaryFile('ocr.ps1');
  Script := ExpandConstant('{tmp}\ocr.ps1');
  ListFile := ExpandConstant('{tmp}\languages.txt');
  Params := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + Script + '"';
  if not Exec(PowerShell, Params + ' -Check -LanguagesFile "' + ListFile + '"', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 3) then
    Exit;
  if not LoadStringFromFile(ListFile, Languages) or (Trim(String(Languages)) = '') then
    Exit;
  if (Choice <> 'install') and (SuppressibleMsgBox(CustomMessage('OcrMissing'), mbConfirmation, MB_YESNO, IDNO) <> IDYES) then
    Exit;
  WizardForm.StatusLabel.Caption := CustomMessage('OcrInstalling');
  WizardForm.Refresh;
  if ShellExec('runas', PowerShell, Params + ' -Install -Languages "' + Trim(String(Languages)) + '"', '', SW_HIDE, ewWaitUntilTerminated, Code) and (Code = 0) then
    SuppressibleMsgBox(CustomMessage('OcrInstalled'), mbInformation, MB_OK, IDOK)
  else
    SuppressibleMsgBox(CustomMessage('OcrFailed'), mbInformation, MB_OK, IDOK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    OfferTextRecognition();
end;

function InitializeUninstall(): Boolean;
begin
  StopRunningCopies(ExpandConstant('{app}\shotcraft.exe'));
  Result := True;
end;

// Whether a command line Windows keeps starts the copy of ShotCraft being removed.
function StartsThisCopy(const Command: String): Boolean;
begin
  Result := Pos(Lowercase(ExpandConstant('{app}\shotcraft.exe')), Lowercase(Command)) > 0;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command, Settings: String;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  // What ShotCraft registers for the user by itself while it runs: its login item (Launch at
  // Startup) and the shotcraft:// link type (Preferences, Advanced). Another program's stay.
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'ShotCraft', Command) and StartsThisCopy(Command) then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'ShotCraft');
  if RegQueryStringValue(HKCU, 'Software\Classes\shotcraft\shell\open\command', '', Command) and StartsThisCopy(Command) then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\shotcraft');
  Settings := ExpandConstant('{userappdata}\ShotCraft');
  if not UninstallSilent and DirExists(Settings) and (MsgBox(CustomMessage('RemoveSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    DelTree(Settings, True, True, True);
end;
