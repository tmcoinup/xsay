; NSIS installer for xsay (Windows).
;
; Invoked from .github/workflows/release.yml as:
;   makensis /DAPP_VERSION=0.1.0 windows/installer.nsi
;
; Produces windows/xsay-<version>-setup.exe — a per-machine installer
; that copies xsay.exe into Program Files, creates Start Menu +
; (optionally) desktop shortcuts, and registers an Uninstall entry in
; Control Panel → Programs.
;
; The installer does NOT launch xsay on install or reboot. Users start
; it from Start Menu; it self-registers its tray icon.

!ifndef APP_VERSION
  !define APP_VERSION "0.0.0-dev"
!endif
!define APP_NAME "xsay"
!define APP_PUBLISHER "mason"
!define APP_URL "https://github.com/tmcoinup/xsay"
!define APP_EXE "xsay.exe"
!define REG_UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel admin

Name "${APP_NAME} ${APP_VERSION}"
OutFile "xsay-${APP_VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\${APP_NAME}"
InstallDirRegKey HKLM "${REG_UNINST}" "InstallLocation"
ShowInstDetails show
ShowUninstDetails show

; --- Pages -----------------------------------------------------------
!include "MUI2.nsh"
!define MUI_ABORTWARNING
!define MUI_ICON   "xsay.ico"
!define MUI_UNICON "xsay.ico"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch xsay now"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Install ---------------------------------------------------------
Section "Install"
  SetOutPath "$INSTDIR"
  File "xsay.exe"
  ; Licensing + docs stay discoverable post-install.
  File "..\LICENSE"
  File "..\README.md"

  ; Start Menu + Desktop shortcut.
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortCut  "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" \
                  "$INSTDIR\${APP_EXE}"
  CreateShortCut  "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk" \
                  "$INSTDIR\uninstall.exe"

  ; Uninstall metadata — shows up in Control Panel → Programs.
  WriteRegStr   HKLM "${REG_UNINST}" "DisplayName"     "${APP_NAME}"
  WriteRegStr   HKLM "${REG_UNINST}" "DisplayVersion"  "${APP_VERSION}"
  WriteRegStr   HKLM "${REG_UNINST}" "Publisher"       "${APP_PUBLISHER}"
  WriteRegStr   HKLM "${REG_UNINST}" "URLInfoAbout"    "${APP_URL}"
  WriteRegStr   HKLM "${REG_UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr   HKLM "${REG_UNINST}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegDWORD HKLM "${REG_UNINST}" "NoModify" 1
  WriteRegDWORD HKLM "${REG_UNINST}" "NoRepair" 1

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

; --- Uninstall -------------------------------------------------------
Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir  "$INSTDIR"

  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Uninstall.lnk"
  RMDir  "$SMPROGRAMS\${APP_NAME}"

  DeleteRegKey HKLM "${REG_UNINST}"
SectionEnd
