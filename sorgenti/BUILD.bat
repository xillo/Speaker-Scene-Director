@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>&1
if errorlevel 1 (
  echo Rust Cargo was not found.
  echo Install Rust from https://rustup.rs/ then restart your terminal.
  pause
  exit /b 1
)
echo Building the PUBLIC release...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\build.ps1" %*
if errorlevel 1 (
  echo Build failed. Read the error messages above.
  pause
  exit /b 1
)
echo Find your finished public package in dist.
pause
