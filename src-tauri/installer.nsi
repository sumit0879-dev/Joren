; Joren Desktop Application NSIS Installer Script
Unicode True
!include "MUI2.nsh"
!include "x64.nsh"

!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.1.0"
!endif

!ifndef STAGING_DIR
  !define STAGING_DIR "staging\joren"
!endif

!ifndef OUTFILE
  !define OUTFILE "staging\joren-setup-x64.exe"
!endif

Name "Joren"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Joren"
InstallDirRegKey HKLM "Software\Joren" "InstallDir"
RequestExecutionLevel admin

!ifndef ICON_PATH
  !define ICON_PATH "src-tauri\icons\icon.ico"
!endif

!define MUI_ABORTWARNING
!define MUI_ICON "${ICON_PATH}"
!define MUI_UNICON "${ICON_PATH}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\joren.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Joren"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "MainSection" SEC01
  SetOutPath "$INSTDIR"
  SetOverwrite on

  ; Copy all staged files and subdirectories
  File /r "${STAGING_DIR}\*.*"

  ; Create uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start Menu Shortcuts
  CreateDirectory "$SMPROGRAMS\Joren"
  CreateShortcut "$SMPROGRAMS\Joren\Joren.lnk" "$INSTDIR\joren.exe" "" "$INSTDIR\joren.exe" 0
  CreateShortcut "$SMPROGRAMS\Joren\Uninstall Joren.lnk" "$INSTDIR\uninstall.exe" "" "$INSTDIR\uninstall.exe" 0

  ; Desktop Shortcut
  CreateShortcut "$DESKTOP\Joren.lnk" "$INSTDIR\joren.exe" "" "$INSTDIR\joren.exe" 0

  ; Registry entries for Add/Remove Programs
  WriteRegStr HKLM "Software\Joren" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "DisplayName" "Joren"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "Publisher" "Joren Project"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "DisplayIcon" "$INSTDIR\joren.exe,0"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren" "NoRepair" 1
SectionEnd

Section "Uninstall"
  RMDir /r "$INSTDIR\app"
  RMDir /r "$INSTDIR\locales"
  RMDir /r "$INSTDIR\Llama"
  Delete "$INSTDIR\*.*"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Joren\Joren.lnk"
  Delete "$SMPROGRAMS\Joren\Uninstall Joren.lnk"
  RMDir "$SMPROGRAMS\Joren"
  Delete "$DESKTOP\Joren.lnk"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Joren"
  DeleteRegKey HKLM "Software\Joren"
SectionEnd
