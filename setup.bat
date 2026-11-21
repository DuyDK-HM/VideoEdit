@echo off
rem Installs what building Concat needs on this machine. See scripts\vn\setup.ps1.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\vn\setup.ps1"
if errorlevel 1 ( echo setup failed & pause & exit /b 1 )
