@echo off
rem Static zlib and libqrencode for the window, built with Qt's MinGW (toolchain.sh unpacks this next to qt\).
setlocal
for %%i in ("%~dp0..") do set "ROOT=%%~fi"
set "PATH=%ROOT%\qt\Tools\mingw1310_64\bin;%ROOT%\qt\Tools\CMake_64\bin;%ROOT%\qt\Tools\Ninja;%SystemRoot%\system32;%SystemRoot%"
set "S=%ROOT%\deps-src"
set "D=%ROOT%\deps"
cmake -S "%S%\zlib" -B "%S%\zlib\b" -G Ninja -DCMAKE_BUILD_TYPE=Release "-DCMAKE_INSTALL_PREFIX=%D%" ^
    -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_STATIC=ON -DZLIB_BUILD_TESTING=OFF >nul || exit /b 1
cmake --build "%S%\zlib\b" >nul || exit /b 1
cmake --install "%S%\zlib\b" >nul || exit /b 1
cmake -S "%S%\qrencode" -B "%S%\qrencode\b" -G Ninja -DCMAKE_BUILD_TYPE=Release "-DCMAKE_INSTALL_PREFIX=%D%" ^
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DBUILD_SHARED_LIBS=OFF -DWITH_TOOLS=NO -DWITH_TESTS=NO >nul || exit /b 1
cmake --build "%S%\qrencode\b" >nul || exit /b 1
cmake --install "%S%\qrencode\b" >nul || exit /b 1
rem Only the static archives: the window must not pick up a DLL import library.
if exist "%D%\bin" rmdir /s /q "%D%\bin"
del /q "%D%\lib\*.dll.a" 2>nul
rem zlib names its static archive libzs.a; FindZLIB looks for libz.a.
if exist "%D%\lib\libzs.a" move /y "%D%\lib\libzs.a" "%D%\lib\libz.a" >nul
dir /b "%D%\lib"
