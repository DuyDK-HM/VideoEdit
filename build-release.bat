@echo off
rem Builds the optimised editor that launch.bat (the VideoEdit shortcut) opens.
rem Slow - the window's code is optimised too - so for everyday use, not for
rem trying a change (run.bat). Close VideoEdit and any agent using its MCP first.
setlocal
cd /d "%~dp0"
call vendor\env.cmd
pushd src
cargo build --release -p concat --bin concat
set "RESULT=%errorlevel%"
popd
if not "%RESULT%"=="0" ( echo build failed & pause & exit /b 1 )
echo Built src\target\release\concat.exe
