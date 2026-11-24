@echo off
rem Windows client: the window, its Qt, then MSI, setup.exe and portable zip.
rem Layout under C:\ryNet-build: qt\ (Qt for MinGW, MinGW, CMake, Ninja), deps\ (static zlib, libqrencode),
rem tools\wix (WiX 3), tools\inno, core\rynet-core.exe, src\desktop-qt; build\ is kept between runs, out\ gets the result.
setlocal
for %%i in ("%~dp0..\..\..\..") do set "ROOT=%%~fi"
set "QT=%ROOT%\qt\6.11.2\mingw_64"
set "PATH=%ROOT%\qt\Tools\mingw1310_64\bin;%ROOT%\qt\Tools\CMake_64\bin;%ROOT%\qt\Tools\Ninja;%QT%\bin;%ROOT%\tools\wix;%ROOT%\tools\inno;%SystemRoot%\system32;%SystemRoot%;%SystemRoot%\System32\WindowsPowerShell\v1.0"
set "SRC=%ROOT%\src\desktop-qt"
set "B=%ROOT%\build"
set "OUT=%ROOT%\out"
for /f "delims=" %%v in ('type "%SRC%\version.txt"') do set "VER=%%v"
if not exist "%OUT%" mkdir "%OUT%"

cmake -S "%SRC%" -B "%B%\app" -G Ninja -DCMAKE_BUILD_TYPE=Release "-DCMAKE_PREFIX_PATH=%QT%;%ROOT%\deps" >"%B%-cmake.log" 2>&1 || (type "%B%-cmake.log" & exit /b 1)
cmake --build "%B%\app" || exit /b 1

set "APP=%B%\stage\ryNet"
if exist "%B%\stage" rmdir /s /q "%B%\stage"
mkdir "%APP%"
copy /y "%B%\app\rynet.exe" "%APP%\" >nul
rem Only what the window loads: QtQuick and its dependencies, Windows platform, Schannel TLS; no translations,
rem software OpenGL or shader compilers (the shaders ship precompiled).
windeployqt --release --qmldir "%SRC%\qml" --no-translations --no-system-d3d-compiler --no-system-dxc-compiler --no-opengl-sw ^
    --skip-plugin-types imageformats,iconengines,generic,networkinformation,qmltooling ^
    --exclude-plugins qopensslbackend,qcertonlybackend,qmodernwindowsstyle "%APP%\rynet.exe" >"%B%-deploy.log" 2>&1 || (type "%B%-deploy.log" & exit /b 1)
for /r "%APP%\qml" %%f in (*.qmltypes) do del "%%f"
rem windeployqt leaves the folders of QML modules it didn't copy behind, empty.
powershell -NoProfile -Command "Get-ChildItem -LiteralPath '%APP%' -Recurse -Directory | Sort-Object { $_.FullName.Length } -Descending | Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } | Remove-Item"
copy /y "%ROOT%\core\rynet-core.exe" "%APP%\" >nul || exit /b 1
copy /y "%SRC%\packaging\windows\wintun.dll" "%APP%\" >nul
copy /y "%SRC%\packaging\windows\wintun-LICENSE.txt" "%APP%\" >nul

rem MSI: heat lists the files, rynet.wxs is the product around them.
heat dir "%APP%" -nologo -cg AppFiles -dr INSTALLDIR -gg -g1 -srd -sreg -scom -sfrag -var var.Src -out "%B%\files.wxs" || exit /b 1
candle -nologo -arch x64 -ext WixUtilExtension "-dVersion=%VER%" "-dSrc=%APP%" "-dIco=%SRC%\packaging\windows\rynet.ico" ^
    -out "%B%\\" "%SRC%\packaging\windows\rynet.wxs" "%B%\files.wxs" || exit /b 1
light -nologo -sval -spdb -ext WixUtilExtension -ext WixUIExtension -cultures:en-us -out "%OUT%\ryNet-%VER%.msi" "%B%\rynet.wixobj" "%B%\files.wixobj" || exit /b 1

ISCC /Q "/DAppVersion=%VER%" "/DSrc=%APP%" "/DOut=%OUT%" "%SRC%\packaging\windows\rynet.iss" || exit /b 1

set "P=%B%\portable\ryNet"
if exist "%B%\portable" rmdir /s /q "%B%\portable"
mkdir "%P%\data"
xcopy /e /q /y "%APP%\*" "%P%\" >nul
> "%P%\data\README.txt" echo Portable ryNet keeps its settings, subscriptions and logs in this folder.
if exist "%OUT%\ryNet-%VER%-portable.zip" del "%OUT%\ryNet-%VER%-portable.zip"
tar -a -c -f "%OUT%\ryNet-%VER%-portable.zip" -C "%B%\portable" ryNet || exit /b 1
dir /b "%OUT%\ryNet-%VER%*"
