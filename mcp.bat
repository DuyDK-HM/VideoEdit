@echo off
rem Concat for AI agents: the MCP server on stdin/stdout, with the fork's
rem environment (FFmpeg, yt-dlp, the sidecar, the sync server) loaded.
rem   claude mcp add concat -- F:\programfiles\VideoEdit\mcp.bat
rem Build first with run.bat; nothing may print to stdout before the server.
setlocal
call "%~dp0vendor\env.cmd" >nul 2>nul
"%~dp0src\target\quick\concat.exe" --mcp %*
