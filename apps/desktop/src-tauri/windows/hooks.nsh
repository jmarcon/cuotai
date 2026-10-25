; PRD 0018 RF3.2 / PRD 0021 RF8.1: the durable winget ownership marker.
; The winget manifest passes /WINGET=1 (InstallerSwitches.Custom); only then
; does the installer write install-source.txt beside Cuotai.exe, which
; cuotai_core::install_source reads to report "winget" and turn the internal
; updater off. A manual install over it removes the marker, and the
; uninstaller removes it with the application.
!include "FileFunc.nsh"

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  ClearErrors
  ${GetParameters} $0
  ${GetOptions} $0 "/WINGET=" $1
  ${IfNot} ${Errors}
    FileOpen $1 "$INSTDIR\install-source.txt" w
    FileWrite $1 "winget"
    FileClose $1
  ${Else}
    Delete "$INSTDIR\install-source.txt"
  ${EndIf}
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\install-source.txt"
!macroend
