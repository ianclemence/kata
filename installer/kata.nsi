; Kata installer: per-user, no administrator password, any folder (D:\Kata works).
Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef DIST
  !define DIST "..\dist\kata"
!endif
!ifndef OUTFILE
  !define OUTFILE "..\dist\kata-setup.exe"
!endif

!define APPNAME "Kata"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kata"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Kata"
InstallDirRegKey HKCU "Software\Kata" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Kata"
VIAddVersionKey "FileDescription" "Kata installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Ian Clemence. MIT OR Apache-2.0"

!define MUI_ICON "${DIST}\kata.ico"
!define MUI_UNICON "${DIST}\kata.ico"
!define MUI_ABORTWARNING
BrandingText "Kata ${VERSION}"

!define MUI_WELCOMEPAGE_TITLE "Welcome to Kata"
!define MUI_WELCOMEPAGE_TEXT "Cut people and things out of photos, print them in your brand's colours and lay out posters, right on this computer.$\r$\n$\r$\nKata installs for you alone, so no administrator password is needed. It adds Kata to the Start menu, the desktop and your PATH."
!insertmacro MUI_PAGE_WELCOME

!define MUI_DIRECTORYPAGE_TEXT_TOP "Choose where to install Kata. Any folder you own works, for example D:\Kata.$\r$\n$\r$\nYour downloads and settings are kept in $LOCALAPPDATA\kata, so they survive upgrades."
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_TITLE "Kata is ready"
!define MUI_FINISHPAGE_TEXT "One more thing for sharp cutouts: Kata needs its cutout engine, a one-time 214 MB download. Tick the box below to get it now (a window shows the progress)."
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Download the cutout engine now (214 MB)"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION DownloadModel
!define MUI_FINISHPAGE_RUN "$INSTDIR\kata-app.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Kata"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function DownloadModel
  ExecWait '"$SYSDIR\cmd.exe" /c title Kata: downloading the cutout engine && "$INSTDIR\kata.exe" models download birefnet-lite-fp32 && timeout /t 3'
FunctionEnd

Function .onInit
  nsExec::Exec 'taskkill /F /IM kata-web.exe'
FunctionEnd

Section "Kata" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${DIST}\kata.exe"
  File "${DIST}\kata-web.exe"
  File "${DIST}\kata-app.exe"
  File "${DIST}\onnxruntime.dll"
  File /nonfatal "${DIST}\onnxruntime_providers_shared.dll"
  File "${DIST}\kata.ico"
  File "${DIST}\README.md"
  File "${DIST}\CHANGELOG.md"
  File "${DIST}\LICENSE-MIT"
  File "${DIST}\LICENSE-APACHE"
  File /r "${DIST}\skills"
  File /r "${DIST}\examples"
  File /r "${DIST}\docs"
  File /r "${DIST}\assets"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; kata on the user PATH
  nsExec::Exec 'powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d=''$INSTDIR''; $$p=[Environment]::GetEnvironmentVariable(''Path'',''User''); $$parts=@($$p -split '';'' | Where-Object { $$_ -ne '''' }); if ($$parts -notcontains $$d) { [Environment]::SetEnvironmentVariable(''Path'', ((@($$parts) + $$d) -join '';''), ''User'') }"'

  CreateShortcut "$SMPROGRAMS\Kata.lnk" "$INSTDIR\kata-app.exe" "" "$INSTDIR\kata.ico" 0
  CreateShortcut "$DESKTOP\Kata.lnk" "$INSTDIR\kata-app.exe" "" "$INSTDIR\kata.ico" 0

  WriteRegStr HKCU "Software\Kata" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "Kata"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "Ian Clemence"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\kata.ico"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://github.com/ianclemence/kata"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" 60000
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM kata-web.exe'
  Sleep 400
  nsExec::Exec 'powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d=''$INSTDIR''; $$p=[Environment]::GetEnvironmentVariable(''Path'',''User''); $$parts=@($$p -split '';'' | Where-Object { $$_ -ne '''' -and $$_ -ne $$d }); [Environment]::SetEnvironmentVariable(''Path'', ($$parts -join '';''), ''User'')"'
  Delete "$SMPROGRAMS\Kata.lnk"
  Delete "$DESKTOP\Kata.lnk"
  RMDir /r "$INSTDIR\skills"
  RMDir /r "$INSTDIR\examples"
  RMDir /r "$INSTDIR\docs"
  RMDir /r "$INSTDIR\assets"
  Delete "$INSTDIR\*.exe"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\*.ico"
  Delete "$INSTDIR\*.md"
  Delete "$INSTDIR\LICENSE-*"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "Software\Kata"
SectionEnd
