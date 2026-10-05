; rexenv's NSIS installer hooks (tauri.conf.json -> bundle.windows.nsis.installerHooks).
; Tauri !includes this file BEFORE any Section, so a top-level attribute here applies to the
; whole installer, and the NSIS_HOOK_* macros run inside Tauri's Install / Uninstall sections
; (PREINSTALL before Tauri's own running-app check, POSTINSTALL after the shortcuts,
; PREUNINSTALL before the uninstaller's running-app check, POSTUNINSTALL after the cleanup).
;
; The two failures these exist for (docs/TODO.md, Win11 VM):
;   * `setup.exe /S` over a RUNNING rexenv returned 0 and left the old rexenv.exe in place
;     (30 Sep 2026). The DNS agent is rexenv.exe too (`\rexenv\dns-agent`, a logon task that is
;     always running); the silent path shows no dialog; and NSIS's default `AllowSkipFiles on`
;     lets a locked file be SKIPPED silently in silent mode -- a half-replaced install with a
;     green exit code (`rex 0.8.11 ... app rexenv 0.8.10`).
;   * `uninstall.exe` from Apps & Features removed everything but rexenv.exe (SMOKE run 6,
;     21 Sep 2026): the per-user task, left registered, re-ran the agent from the file within
;     the minute and held it -- an uninstalled app whose resolver answers 127.0.0.1:53 forever.
;
; The task name must match platform/windows/logon_task.rs DNS_AGENT_TASK; a test holds them
; together. NO BOM, ASCII only (the English.nsh rule).

; A file that cannot be written ABORTS the install with a non-zero exit code -- never a
; silent skip. The interactive installer shows NSIS's retry dialog as before.
AllowSkipFiles off

!define REXENV_DNS_TASK "\rexenv\dns-agent"
!define REXENV_EXE "rexenv.exe"

; Stop the agent's task (so nothing puts the agent back), then kill every rexenv.exe of this
; user and WAIT until none is left: a kill is asynchronous and the file stays held for a
; moment, and a File step that lands in that moment fails. Bounded: 20 x 500 ms.
!macro REXENV_STOP_AGENT_AND_WAIT
  nsExec::ExecToLog 'schtasks /End /TN "${REXENV_DNS_TASK}"'
  Pop $0
  StrCpy $1 0
  ${Do}
    nsis_tauri_utils::FindProcessCurrentUser "${REXENV_EXE}"
    Pop $0
    ${If} $0 <> 0
      ${ExitDo}
    ${EndIf}
    nsis_tauri_utils::KillProcessCurrentUser "${REXENV_EXE}"
    Pop $0
    Sleep 500
    IntOp $1 $1 + 1
  ${LoopUntil} $1 >= 20
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Silent only: the interactive installer shows Tauri's "rexenv is still running" dialog and the
  ; person decides. Here nobody is asked, so the agent is stopped for the swap (and restarted
  ; after it, below); the app itself is closed as the silent install always meant to.
  ${If} ${Silent}
    !insertmacro REXENV_STOP_AGENT_AND_WAIT
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; The resolver comes back on the NEW binary. The task is the user's own, so no UAC; on a
  ; first install there is no task yet and the error is harmless. The app is NOT launched:
  ; silent means silent.
  ${If} ${Silent}
    nsExec::ExecToLog 'schtasks /Run /TN "${REXENV_DNS_TASK}"'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; The task goes WITH the app: ended (which stops the agent it started) and deleted, before
  ; Tauri's running-app check handles the app itself (a dialog when interactive, a kill when
  ; silent) -- so nothing re-runs rexenv.exe while the uninstaller removes it. No UAC: the
  ; task is the user's own. NRPT and the certificate are NOT touched here (they need the
  ; elevated step inside the app); the note after the uninstall says so.
  nsExec::ExecToLog 'schtasks /End /TN "${REXENV_DNS_TASK}"'
  Pop $0
  nsExec::ExecToLog 'schtasks /Delete /TN "${REXENV_DNS_TASK}" /F'
  Pop $0
  Sleep 1500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${IfNot} ${Silent}
    MessageBox MB_OK|MB_ICONINFORMATION "rexenv is uninstalled. Two system settings it made are still on this PC and are removed only from inside rexenv (Settings > Services > Uninstall): the .rex DNS rule and the rexenv certificate. To remove them, reinstall rexenv and use that button."
  ${EndIf}
!macroend
