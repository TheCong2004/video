@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\run-kedi-test.ps1" -UseExistingProfile -RecoverWindowless
if errorlevel 1 pause
