; ============================================================================
;  Todo Downloader — Windows installer (Inno Setup)
; ============================================================================
;  Built in CI by .github/workflows/build.yml. To build it by hand:
;
;      iscc /DAppVersion=1.8.5 /DBinary=..\target\release\todo-downloader.exe ^
;           installer\todo-downloader.iss
;
;  WHY AN INSTALLER AT ALL. The portable .exe stays the primary download and
;  always will: it needs nothing, writes nothing outside its own folder and
;  can live on a USB stick. But "download an .exe and put it somewhere" is not
;  how most people expect to install a program, and the request for a Setup is
;  a fair one. Both are published side by side.
;
;  WHAT THIS DOES NOT DO, on purpose:
;
;    - It does NOT bundle yt-dlp, gallery-dl or ffmpeg. Those are downloaded
;      from Settings when you want them, and they update far more often than
;      this application does. Freezing a copy inside the installer would ship
;      a stale yt-dlp that breaks on YouTube within weeks.
;    - It does NOT register the magnet: protocol. The application does that
;      from Settings, with the user's explicit consent, because it modifies a
;      protocol association and that is not a decision an installer should
;      take silently.
;    - It does NOT require administrator rights. See PrivilegesRequired below.
;    - It does NOT touch settings or downloads on uninstall.
; ============================================================================

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Binary
  #define Binary "..\target\release\todo-downloader.exe"
#endif

#define AppName    "Todo Downloader"
#define Publisher  "Eric Valls Gramunt"
#define AppURL     "https://github.com/AcidClawX41/todo-downloader"
#define ExeName    "todo-downloader.exe"

[Setup]
AppId={{8E5C1F42-9A3D-4B27-B6E1-7D0C4A9F2E33}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
VersionInfoVersion={#AppVersion}

; INSTALL WITHOUT ADMINISTRATOR RIGHTS.
;
; `lowest` installs into the user's own profile, so Windows never shows a UAC
; prompt. That matters more than usual here: the binaries are not code-signed,
; so a UAC dialog would read "Unknown publisher" in red and ask for elevation
; — a combination that reasonably scares people off.
;
; `dialog` lets anyone who prefers a machine-wide install under Program Files
; choose it at the start, and only then does Windows ask for elevation.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes

; 64-bit only: that is the only architecture the project builds for.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

OutputDir=..\dist
OutputBaseFilename=todo-downloader-setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#ExeName}
UninstallDisplayName={#AppName}

; LZMA2/max: the binary is ~9 MB of already-optimised code, and the installer
; is downloaded far more often than it is built.
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; THE ACCEPTANCE SCREEN.
;
; Inno Setup's licence page is the only one with an "I accept / I do not
; accept" choice, so it shows the TERMS OF USE — which is what the user is
; being asked to accept. The GPL is deliberately NOT put there: its section 9
; states plainly that you are not required to accept it in order to receive or
; run a copy, only to redistribute or modify. Gating the program behind an
; "I accept" on the GPL would assert a condition the licence itself denies.
; It is installed alongside the program as LICENSE, and the terms point to it.
;
; One document per language: the files go on the [Languages] entries rather
; than in [Setup], because Inno Setup allows one of each per language.

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"; \
  LicenseFile: "terms-en.txt"; InfoAfterFile: "post-install-en.txt"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"; \
  LicenseFile: "terms-es.txt"; InfoAfterFile: "post-install-es.txt"

[CustomMessages]
english.CreateDesktopIcon=Create a &desktop shortcut
spanish.CreateDesktopIcon=Crear un acceso directo en el &escritorio
english.LaunchApp=Launch {#AppName}
spanish.LaunchApp=Abrir {#AppName}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Binary}"; DestDir: "{app}"; DestName: "{#ExeName}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
; Section 9 of the terms states that releases up to v1.4.0 remain available
; under MIT. That claim should be verifiable without an internet connection.
Source: "..\LICENSE-HISTORY.md"; DestDir: "{app}"; Flags: ignoreversion

; The accepted terms are installed too: someone who accepts them today must
; be able to read them again tomorrow without re-running the installer.
Source: "terms-en.txt"; DestDir: "{app}"; DestName: "TERMS.txt"; Flags: ignoreversion; Languages: english
Source: "terms-es.txt"; DestDir: "{app}"; DestName: "CONDICIONES.txt"; Flags: ignoreversion; Languages: spanish

; The `tips` folder is where the optional animated GIFs for the support tab
; go. Only its README ships: the GIFs themselves are third-party material and
; are deliberately not distributed. Creating the folder makes the feature
; discoverable instead of a documented secret.
Source: "..\tips\README.txt"; DestDir: "{app}\tips"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#ExeName}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#ExeName}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; WHAT IS NOT REMOVED, AND WHY.
;
; Settings, downloads and the helper engines all live in the user profile, not
; here, and none of them are deleted. Settings and downloads are the user's own
; files. The helpers (yt-dlp, gallery-dl, ffmpeg) sit in
; %LOCALAPPDATA%\TodoDownloader\bin, which is shared with any portable copy of
; the application: removing them on uninstall would silently break a portable
; install the user still wanted. They are ordinary files in a documented folder
; and can be deleted by hand.
;
; Only what the installer itself put here is removed. `tips` is emptied only if
; the user never added a GIF to it.
Type: dirifempty; Name: "{app}\tips"
Type: dirifempty; Name: "{app}"
