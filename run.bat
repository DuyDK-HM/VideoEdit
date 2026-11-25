@echo off
rem Builds Concat if the sources changed (cargo decides; a no-op build takes seconds), then opens it.
rem Run setup.bat once first.
setlocal
cd /d "%~dp0"
if not exist vendor\env.cmd ( echo Run setup.bat first. & pause & exit /b 1 )
call vendor\env.cmd
set "EXE=%~dp0src\target\quick\concat.exe"

where cargo >nul 2>nul
if errorlevel 1 (
  if not exist "%EXE%" ( echo cargo not found and no built editor at %EXE%. & pause & exit /b 1 )
  echo cargo not found, opening the last build.
) else (
  rem The build's id is its commit; see build-release.bat.
  for /f %%i in ('git rev-parse HEAD 2^>nul') do set "GITHUB_SHA=%%i"
  pushd src
  cargo build --profile quick -p concat --bin concat
  if errorlevel 1 ( popd & echo build failed & pause & exit /b 1 )
  popd
)
start "" "%EXE%" %*
