@echo off
rem Builds (if needed) and runs OpenPax. Any arguments are passed on:
rem   run.bat                         open the last pack
rem   run.bat C:\packs\my_pack        open that pack
rem   run.bat --debug ...             use a debug build instead of release
setlocal
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

set "PROFILE=--release"
if /i "%~1"=="--debug" (
    set "PROFILE="
    shift
)

set "ARGS="
:collect
if "%~1"=="" goto run
set ARGS=%ARGS% %1
shift
goto collect

:run
cargo run %PROFILE% -p openpax-app --%ARGS%
if errorlevel 1 (
    echo.
    echo OpenPax failed to build or exited with an error.
    pause
)
endlocal
