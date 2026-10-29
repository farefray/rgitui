@echo off
rem Launch the newest local build. Optional first argument: repository directory.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\run-windows.ps1" %*
if errorlevel 1 (
    pause
    exit /b 1
)
