@echo off
setlocal enabledelayedexpansion

set "ROOT_DIR=%~dp0.."
cd /d "%ROOT_DIR%"

echo [1/3] Building Windows release binary (100%% Safe Rust)...
cargo build --release
if errorlevel 1 exit /b 1

set "DIST_DIR=%ROOT_DIR%\dist\NanikaAi-win64"
if exist "%DIST_DIR%" rmdir /s /q "%DIST_DIR%"
mkdir "%DIST_DIR%"

echo [2/3] Copying executable and CC0 default assets...
copy /Y "%ROOT_DIR%\target\release\nanikaai.exe" "%DIST_DIR%\nanikaai.exe" >nul
xcopy /E /I /Y "%ROOT_DIR%\assets" "%DIST_DIR%\assets" >nul
copy /Y "%ROOT_DIR%\LICENSE" "%DIST_DIR%\LICENSE" >nul

echo [3/3] Windows portable build ready: %DIST_DIR%
endlocal
