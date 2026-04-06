@echo off
setlocal

set "BINARY=%~1"
set "DB_PATH=%~2"

if "%BINARY%"=="" exit /b 2
if "%DB_PATH%"=="" exit /b 3

for %%I in ("%DB_PATH%") do set "HARMONY_DIR=%%~dpI"
set "TRACE_LOG=%HARMONY_DIR%mcp-debug.log"
set "LAUNCH_LOG=%HARMONY_DIR%context-server-launch.log"

>>"%LAUNCH_LOG%" echo [%date% %time%] launch "%BINARY%" --db-path "%DB_PATH%"
set "HARMONY_MCP_DEBUG_LOG=%TRACE_LOG%"
set "HARMONY_MCP_TRACE_STDERR=0"

"%BINARY%" --db-path "%DB_PATH%"
set "EXIT_CODE=%ERRORLEVEL%"

>>"%LAUNCH_LOG%" echo [%date% %time%] exit %EXIT_CODE%
exit /b %EXIT_CODE%
