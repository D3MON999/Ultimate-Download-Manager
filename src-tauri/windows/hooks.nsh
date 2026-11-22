; Ultimate Download Manager's installer hooks (tauri.conf.json → bundle.windows.nsis.installerHooks).
;
; Before installing: a running Ultimate Download Manager is asked to quit (udm.exe --quit reaches
; the running copy wherever it was started from, through its single-instance channel), then any
; copy still running and the browser helper are ended, so files aren't in use and the new
; version isn't handed over to an old running one. Downloads resume at the next start.

!macro UDM_CLOSE_RUNNING
  ; The installed copy (an update), else whatever udm.exe is on PATH: a graceful quit.
  IfFileExists "$INSTDIR\udm.exe" 0 udm_no_installed
    nsExec::Exec '"$INSTDIR\udm.exe" --quit'
    Pop $0
    Sleep 2500
  udm_no_installed:
  ; Anything still running (a copy started from another folder too): ended.
  nsExec::Exec 'taskkill /F /T /IM udm.exe'
  Pop $0
  nsExec::Exec 'taskkill /F /IM udm-nmhost.exe'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UDM_CLOSE_RUNNING
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UDM_CLOSE_RUNNING
!macroend
