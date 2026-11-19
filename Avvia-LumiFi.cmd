@echo off
rem Doppio clic: scarica le ultime modifiche, ricompila LumiFi e lo apre.
cd /d "%~dp0"
echo Scarico gli aggiornamenti...
git pull --ff-only
if errorlevel 1 (
  echo.
  echo Non riesco ad aggiornare: hai modificato qualche file? Chiedi a Claude.
  pause
  exit /b 1
)
echo Compilo LumiFi (la prima volta ci vuole un po^)...
cargo run --release -p lumifi
if errorlevel 1 pause
