@echo off
setlocal

pushd "%~dp0" >nul
set "REPO_ROOT=%CD%"
set "BINARY=%REPO_ROOT%\target\release\harmony-mcp.exe"
set "DB_PATH=%REPO_ROOT%\.harmony\memory.db"

if not exist "%BINARY%" (
  echo [Harmony] Building the release sidecar...
  cargo build --release -p harmony-mcp
  if errorlevel 1 (
    echo [Harmony] Build failed. Fix the Cargo errors above and run this script again.
    popd >nul
    exit /b 1
  )
)

echo [Harmony] Verifying the local Harmony setup...
"%BINARY%" doctor --db-path "%DB_PATH%"
if errorlevel 1 (
  echo [Harmony] Verification failed. Check the output above.
  popd >nul
  exit /b 1
)

if /I "%~1"=="--check-only" (
  echo [Harmony] Check finished. No server was started.
  popd >nul
  exit /b 0
)

if /I "%~1"=="--foreground" (
  echo [Harmony] Starting harmony-mcp in this window...
  "%BINARY%" --db-path "%DB_PATH%"
  set "EXITCODE=%ERRORLEVEL%"
  popd >nul
  exit /b %EXITCODE%
)

echo [Harmony] Starting harmony-mcp in a new terminal window...
start "Harmony MCP" cmd /k ""%BINARY%" --db-path "%DB_PATH%""

echo.
echo Next steps:
echo 1. Open Zed and use "Install Dev Extension" on this folder:
echo    %REPO_ROOT%
echo 2. Open a project and enable the Harmony context server if Zed prompts you.
echo 3. Run /harmony-pulse in the Assistant to confirm Harmony can read the project database.
echo 4. Keep the "Harmony MCP" terminal window open while you test overlaps.
echo.
echo Tip: run start-harmony-mcp.bat --foreground to keep the server in the current terminal.

popd >nul
exit /b 0
