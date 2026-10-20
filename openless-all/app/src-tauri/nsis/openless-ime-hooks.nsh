!define OPENLESS_IME_BUILD_TAG "${__DATE__}_${__TIME__}"
!searchreplace OPENLESS_IME_BUILD_TAG "${OPENLESS_IME_BUILD_TAG}" ":" "-"
!searchreplace OPENLESS_IME_BUILD_TAG "${OPENLESS_IME_BUILD_TAG}" "/" "-"
!searchreplace OPENLESS_IME_BUILD_TAG "${OPENLESS_IME_BUILD_TAG}" " " "_"
!define OPENLESS_IME_FILENAME "OpenLessIme-${VERSION}-${OPENLESS_IME_BUILD_TAG}.dll"

!define OPENLESS_IME_DIAGNOSTIC_FILE "$TEMP\openless-ime-install-diagnostic.ndjson"

; Diagnostic writes are best-effort and never change the installer outcome.
!macro OPENLESS_IME_DIAGNOSTIC_EVENT STAGE
  Push $0
  Push $R9
  StrCpy $R9 $0
  ClearErrors
  FileOpen $0 "${OPENLESS_IME_DIAGNOSTIC_FILE}" a
  ${IfNot} ${Errors}
    FileWrite $0 "{$\"schema$\":$\"openless.windows-installer.v1$\",$\"probe_id$\":$\"P4$\",$\"stage$\":$\"${STAGE}$\",$\"code$\":$\"$R9$\"}$\r$\n"
    FileClose $0
  ${EndIf}
  Pop $R9
  Pop $0
!macroend

!macro OPENLESS_IME_ABORT_IF_FAILED EXIT_CODE LABEL
  ${If} ${EXIT_CODE} != 0
    DetailPrint "OpenLess TSF IME ${LABEL} failed with exit code ${EXIT_CODE}"
    Abort
  ${EndIf}
!macroend

!macro OPENLESS_IME_REGISTER_X64
  ${If} ${RunningX64}
    DetailPrint "Registering OpenLess x64 TSF IME"
    ExecWait '"$WINDIR\Sysnative\regsvr32.exe" /s "$INSTDIR\windows-ime\x64\${OPENLESS_IME_FILENAME}"' $0
    ${If} $0 != 0
      ${DisableX64FSRedirection}
      ExecWait '"$WINDIR\System32\regsvr32.exe" /s "$INSTDIR\windows-ime\x64\${OPENLESS_IME_FILENAME}"' $0
      ${EnableX64FSRedirection}
    ${EndIf}
    !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT register_x64_result
    !insertmacro OPENLESS_IME_ABORT_IF_FAILED $0 "x64 registration"
  ${EndIf}
!macroend

!macro OPENLESS_IME_UNREGISTER_X64
  ${If} ${RunningX64}
    DetailPrint "Unregistering OpenLess x64 TSF IME"
    SetRegView 64
    StrCpy $R0 ""
    ReadRegStr $R0 HKLM "Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32" ""
    ${If} $R0 == ""
      StrCpy $R0 "$INSTDIR\windows-ime\x64\${OPENLESS_IME_FILENAME}"
    ${EndIf}
    ExecWait '"$WINDIR\Sysnative\regsvr32.exe" /s /u "$R0"' $0
    ${If} $0 != 0
      ${DisableX64FSRedirection}
      ExecWait '"$WINDIR\System32\regsvr32.exe" /s /u "$R0"' $0
      ${EnableX64FSRedirection}
    ${EndIf}
    DetailPrint "OpenLess x64 TSF IME unregister exit code $0"
  ${EndIf}
!macroend

!macro OPENLESS_IME_UNREGISTER_X86
  DetailPrint "Unregistering OpenLess x86 TSF IME"
  SetRegView 32
  StrCpy $R1 ""
  ReadRegStr $R1 HKLM "Software\Classes\CLSID\{6B9F3F4F-5EE7-42D6-9C61-9F80B03A5D7D}\InprocServer32" ""
  ${If} $R1 == ""
    StrCpy $R1 "$INSTDIR\windows-ime\x86\${OPENLESS_IME_FILENAME}"
  ${EndIf}
  ${If} ${RunningX64}
    ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s /u "$R1"' $0
  ${Else}
    ExecWait '"$WINDIR\System32\regsvr32.exe" /s /u "$R1"' $0
  ${EndIf}
  DetailPrint "OpenLess x86 TSF IME unregister exit code $0"
!macroend

!macro OPENLESS_IME_REGISTER_X86
  DetailPrint "Registering OpenLess x86 TSF IME"
  ${If} ${RunningX64}
    ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s "$INSTDIR\windows-ime\x86\${OPENLESS_IME_FILENAME}"' $0
  ${Else}
    ExecWait '"$WINDIR\System32\regsvr32.exe" /s "$INSTDIR\windows-ime\x86\${OPENLESS_IME_FILENAME}"' $0
  ${EndIf}
  ${If} $0 != 0
    StrCpy $1 $0
    !insertmacro OPENLESS_IME_UNREGISTER_X64
    StrCpy $0 $1
  ${EndIf}
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT register_x86_result
  !insertmacro OPENLESS_IME_ABORT_IF_FAILED $0 "x86 registration"
!macroend

!macro OPENLESS_IME_DELETE_LEGACY PLATFORM_DIR
  Delete /REBOOTOK "$INSTDIR\windows-ime\${PLATFORM_DIR}\OpenLessIme.dll"
  ${If} ${FileExists} "$INSTDIR\windows-ime\${PLATFORM_DIR}\OpenLessIme.dll"
    !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT legacy_${PLATFORM_DIR}_pending_reboot
    DetailPrint "OpenLess TSF IME ${PLATFORM_DIR} legacy DLL deletion deferred until reboot"
    SetRebootFlag true
  ${EndIf}
!macroend

!macro OPENLESS_IME_STAGE_AND_REPLACE PLATFORM_DIR ENV_VAR
  ; Install each build under a new name. TSF client processes can keep an old
  ; DLL mapped for their whole lifetime, so reusing a release path can leave the
  ; installer unable to replace the file or the registry pointing at old code.
  SetOutPath "$INSTDIR\windows-ime\${PLATFORM_DIR}"
  File /oname=${OPENLESS_IME_FILENAME} "$%${ENV_VAR}%"
  ${IfNot} ${FileExists} "$INSTDIR\windows-ime\${PLATFORM_DIR}\${OPENLESS_IME_FILENAME}"
    !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT stage_${PLATFORM_DIR}_missing
    DetailPrint "OpenLess TSF IME ${PLATFORM_DIR} DLL staging failed"
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Upgrade path: release the COM/TSF registration so new client processes won't
  ; bind to the old DLL while we replace it. Fresh install: regsvr32 /u against
  ; a missing DLL is harmless — the existing UNREGISTER macros log the exit code
  ; and continue without aborting.
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT preinstall_begin
  !insertmacro OPENLESS_IME_UNREGISTER_X86
  !insertmacro OPENLESS_IME_UNREGISTER_X64
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT preinstall_unregistration_complete

  ; Do not leave the legacy basename available for a new TSF client to load.
  ; A running TSF client can keep the file mapped, so NSIS schedules deletion
  ; at reboot and continues with the versioned registration.
  !insertmacro OPENLESS_IME_DELETE_LEGACY "x64"
  !insertmacro OPENLESS_IME_DELETE_LEGACY "x86"

  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT stage_x64_begin
  !insertmacro OPENLESS_IME_STAGE_AND_REPLACE "x64" "OPENLESS_IME_DLL_X64"
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT stage_x64_complete
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT stage_x86_begin
  !insertmacro OPENLESS_IME_STAGE_AND_REPLACE "x86" "OPENLESS_IME_DLL_X86"
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT stage_x86_complete

  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro OPENLESS_IME_DIAGNOSTIC_EVENT postinstall_begin
  !insertmacro OPENLESS_IME_REGISTER_X64
  !insertmacro OPENLESS_IME_REGISTER_X86
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro OPENLESS_IME_UNREGISTER_X86
  !insertmacro OPENLESS_IME_UNREGISTER_X64
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\windows-ime\x64\OpenLessIme-*.dll"
  Delete "$INSTDIR\windows-ime\x64\OpenLessIme.dll"
  Delete "$INSTDIR\windows-ime\x86\OpenLessIme-*.dll"
  Delete "$INSTDIR\windows-ime\x86\OpenLessIme.dll"
  RMDir "$INSTDIR\windows-ime\x64"
  RMDir "$INSTDIR\windows-ime\x86"
  RMDir "$INSTDIR\windows-ime"
!macroend
