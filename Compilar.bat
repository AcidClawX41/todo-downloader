@echo off
rem Compila Todo Downloader (Rust) y deja el .exe junto a este script
cd /d "%~dp0"
where cargo >nul 2>&1 || (echo Rust no encontrado. Instala desde https://rustup.rs y reabre esta ventana. & pause & exit /b 1)
echo Compilando en modo release (la primera vez tarda unos minutos)...
cargo build --release
if errorlevel 1 (echo. & echo ERROR de compilacion. Revisa los mensajes anteriores. & pause & exit /b 1)
copy /y "target\release\todo-downloader.exe" "Todo Downloader.exe" >nul
echo.
echo Listo: "%~dp0Todo Downloader.exe"
pause
