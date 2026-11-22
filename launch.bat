@echo off
rem Opens the last build with the fork's settings (FFmpeg, yt-dlp, the sidecar,
rem the sync server, the SFX library) - what the VideoEdit shortcut runs.
rem run.bat builds first; this does not.
setlocal
call "%~dp0vendor\env.cmd"
start "" "%~dp0src\target\quick\concat.exe" %*
