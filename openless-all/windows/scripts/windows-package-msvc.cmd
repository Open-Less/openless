@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
set "SUPPLIED_ARGS=%*"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%windows-package-msvc.ps1" %SUPPLIED_ARGS%
exit /b %ERRORLEVEL%
