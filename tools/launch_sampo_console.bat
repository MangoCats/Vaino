@echo off
REM Launches the Sampo console (tools/console.py) against the desktop's own
REM working library, and opens it in the default browser.
REM
REM This is NOT the same database vainopi plays from -- see
REM docs/spec/SPEC006-data-flow-and-portability.md. Edits here reach the
REM appliance only via the bundle exporter/importer, never automatically.
setlocal
cd /d "%~dp0.."
start "Sampo Console" python tools\console.py data\vaino_new.db --root "C:\Users\Mango Cat\Music"
timeout /t 2 /nobreak >nul
start "" http://127.0.0.1:5730/
