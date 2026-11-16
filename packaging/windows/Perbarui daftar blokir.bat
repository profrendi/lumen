@echo off
rem Mengunduh daftar filter iklan/pelacak terbaru (EasyList, EasyPrivacy, ABPindo, uBlock)
rem ke Data\filters. Tutup Lumen dulu; daftar dibaca saat browser dibuka.
setlocal EnableDelayedExpansion
set "DIR=%~dp0Data\filters"
if not exist "%DIR%" mkdir "%DIR%"
set GAGAL=0
for /f "usebackq eol=# tokens=1,2" %%A in ("%~dp0filters.list") do (
  curl.exe -fsSL --retry 3 -o "%DIR%\%%A.tmp" "%%B"
  if !errorlevel! equ 0 (
    move /Y "%DIR%\%%A.tmp" "%DIR%\%%A" >nul
    echo OK     %%A
  ) else (
    del "%DIR%\%%A.tmp" 2>nul
    echo GAGAL  %%A ^(berkas lama tetap dipakai^)
    set GAGAL=1
  )
)
pause
exit /b %GAGAL%
