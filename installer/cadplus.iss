; Установщик Cad+ для Windows 10/11 (Inno Setup 6).
; Сборка: iscc /DAppVersion=0.1.0 installer\cadplus.iss  — после cargo build --release -p cadplus-gui.
; Обычно его собирает GitHub Actions (.github/workflows/release.yml).

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{8F3C2A51-6D2E-4B7A-9C1E-5A4D3B2C1F0E}
AppName=Cad+
AppVersion={#AppVersion}
AppVerName=Cad+ {#AppVersion}
AppPublisher=wardjonefscek
AppPublisherURL=https://github.com/wardjonefscek/Yadryshko-
AppComments=Расчёт крыла, лонжеронов и нервюр с топооптимизацией
DefaultDirName={autopf}\Cad+
DefaultGroupName=Cad+
DisableProgramGroupPage=yes
; Ставится без прав администратора — в папку программ пользователя.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=Cad+-{#AppVersion}-setup
SetupIconFile=..\assets\icon\cadplus.ico
UninstallDisplayIcon={app}\cadplus.exe
UninstallDisplayName=Cad+
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes

[Languages]
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\cadplus.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\icon\cadplus.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\airfoils\clarky.dat"; DestDir: "{app}\airfoils"; Flags: ignoreversion
Source: "..\docs\app.md"; DestDir: "{app}\docs"; DestName: "Руководство.md"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Cad+\Cad+"; Filename: "{app}\cadplus.exe"; IconFilename: "{app}\cadplus.ico"; Comment: "Расчёт крыла и нервюр"
Name: "{autoprograms}\Cad+\Cad+ (совместимый режим OpenGL)"; Filename: "{app}\cadplus.exe"; Parameters: "--glow"; IconFilename: "{app}\cadplus.ico"; Comment: "Если обычный запуск не открывает окно"
Name: "{autoprograms}\Cad+\Руководство"; Filename: "{app}\docs\Руководство.md"
Name: "{autoprograms}\Cad+\Удалить Cad+"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Cad+"; Filename: "{app}\cadplus.exe"; IconFilename: "{app}\cadplus.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\cadplus.exe"; Description: "{cm:LaunchProgram,Cad+}"; Flags: nowait postinstall skipifsilent
