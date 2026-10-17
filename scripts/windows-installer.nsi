Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma
Name "Concat Studio"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\ConcatStudio"
InstallDirRegKey HKCU "Software\ConcatStudio" "InstallDir"
!include "MUI2.nsh"
!include "FileFunc.nsh"
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onVerifyInstDir
  ${GetFileName} "$INSTDIR" $0
  StrCmp $0 "ConcatStudio" allowed
  Abort
  allowed:
FunctionEnd

Section "Concat Studio with local runtimes"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${PACKAGE_DIR}\*.*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\Concat Studio.lnk" "$INSTDIR\Concat.exe"
  CreateDirectory "$SMPROGRAMS\Concat Studio"
  CreateShortcut "$SMPROGRAMS\Concat Studio\Concat Studio.lnk" "$INSTDIR\Concat.exe"
  CreateShortcut "$SMPROGRAMS\Concat Studio\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\ConcatStudio" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio" "DisplayName" "Concat Studio"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio" "DisplayIcon" "$INSTDIR\Concat.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  ; Exact packaged-file manifest; never recursively delete an arbitrary chosen
  ; installation directory or Concat/Nexora user data.
  !include "${REMOVE_MANIFEST}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$DESKTOP\Concat Studio.lnk"
  Delete "$SMPROGRAMS\Concat Studio\Concat Studio.lnk"
  Delete "$SMPROGRAMS\Concat Studio\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Concat Studio"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ConcatStudio"
  DeleteRegKey HKCU "Software\ConcatStudio"
SectionEnd
