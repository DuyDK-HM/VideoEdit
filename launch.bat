@echo off
rem Opens the editor with the fork's settings (FFmpeg, yt-dlp, the sidecar,
rem the sync server, the SFX library) - what the VideoEdit shortcut runs.
rem The optimised build (build-release.bat) when there is one, else run.bat's
rem quick build, whose window code is unoptimised and so slower to use.
setlocal
call "%~dp0vendor\env.cmd"
set "EXE=%~dp0src\target\release\concat.exe"
if not exist "%EXE%" set "EXE=%~dp0src\target\quick\concat.exe"
start "" "%EXE%" %*
