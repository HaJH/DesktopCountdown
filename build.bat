@echo off
REM ============================================================
REM  Build DesktopCountdown in release mode.
REM  Double-click to build, or run from a terminal: build.bat
REM  Output: target\release\desktop-countdown.exe
REM    - Renderer:  desktop-countdown.exe
REM    - Settings:  desktop-countdown.exe --settings
REM ============================================================

setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
    echo ERROR: cargo not found on PATH. Install Rust from https://rustup.rs
    pause
    exit /b 1
)

echo Building DesktopCountdown ^(release^)...
echo.
cargo build --release
if errorlevel 1 (
    echo.
    echo *** BUILD FAILED ***
    pause
    exit /b 1
)

echo.
echo *** BUILD SUCCEEDED ***
echo   Executable:    "%~dp0target\release\desktop-countdown.exe"
echo   Run renderer:  desktop-countdown.exe
echo   Open settings: desktop-countdown.exe --settings
echo.
pause
