cmake_minimum_required(VERSION 3.21)

file(READ ${CMAKE_CURRENT_SOURCE_DIR}/version.txt RYNET_VERSION)
string(STRIP "${RYNET_VERSION}" RYNET_VERSION)
project(rynet VERSION ${RYNET_VERSION} LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.8 REQUIRED COMPONENTS Core Gui Network Qml Quick ShaderTools)
set(QT_NO_PRIVATE_MODULE_WARNING ON)
find_package(Qt6 QUIET COMPONENTS GuiPrivate)
if(UNIX AND NOT APPLE)
    find_package(Qt6 REQUIRED COMPONENTS DBus)
endif()
find_package(ZLIB REQUIRED)
find_path(QRENCODE_INCLUDE_DIR qrencode.h REQUIRED)
find_library(QRENCODE_LIBRARY NAMES qrencode libqrencode REQUIRED)

qt_standard_project_setup(REQUIRES 6.8)

option(RYNET_TESTS "Build the unit tests" OFF)

set(RYNET_CORE_SOURCES
    src/app.cpp src/app_subs.cpp src/app_connect.cpp src/app_auto.cpp src/app_rules.cpp src/app_misc.cpp src/app_detail.h src/app.h
    src/core.cpp src/core.h
    src/fields.cpp
    src/filepick.cpp src/filepick.h
    src/grid.cpp src/grid.h
    src/cards.cpp src/cards.h
    src/gridcell.cpp src/gridcell.h
    src/segments.cpp src/segments.h
    src/setorder.cpp src/setorder.h
    src/hct.h
    src/items.cpp src/items.h
    src/logmodel.cpp src/logmodel.h
    src/model.cpp src/model_rules.cpp src/model_local.cpp src/model_auto.cpp src/model_backup.cpp src/model_detail.h src/model.h src/endpoints.h
    src/platform.cpp src/platform.h
    src/press.cpp src/press.h
    src/reports.cpp src/reports.h
    src/shapes_data.h
    src/spring.cpp src/spring.h
    src/statelayer.cpp src/statelayer.h
    src/store.cpp src/store.h
    src/theme.cpp src/theme.h
    src/ui.cpp src/ui.h
    src/update.cpp src/update.h
    src/tray.cpp src/tray.h
)

qt_add_executable(rynet WIN32 MACOSX_BUNDLE src/main.cpp)

file(GLOB RYNET_QML RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} CONFIGURE_DEPENDS qml/*.qml)
foreach(f ${RYNET_QML})
    get_filename_component(name ${f} NAME)
    set_source_files_properties(${f} PROPERTIES QT_RESOURCE_ALIAS ${name} QT_DISCARD_FILE_CONTENTS TRUE)
endforeach()
foreach(singleton qml/M.qml qml/Icons.qml qml/SetLook.qml)
    set_source_files_properties(${singleton} PROPERTIES QT_QML_SINGLETON_TYPE TRUE)
endforeach()

qt_add_qml_module(rynet
    URI Rynet
    VERSION 1.0
    QML_FILES ${RYNET_QML}
    SOURCES ${RYNET_CORE_SOURCES}
)
set_target_properties(rynet PROPERTIES QT_QMLCACHEGEN_ARGUMENTS "--only-bytecode")

file(GLOB RYNET_ASSETS RELATIVE ${CMAKE_CURRENT_SOURCE_DIR} CONFIGURE_DEPENDS
    assets/flags/*.png assets/icons/*.png assets/fonts/*.ttf)
qt_add_resources(rynet assets
    PREFIX /
    BASE assets
    FILES ${RYNET_ASSETS}
)

if(UNIX AND NOT APPLE)
    set(RYNET_POLKIT packaging/linux/polkit/49-rynet-resolved.rules packaging/linux/polkit/49-rynet-resolved.pkla)
    qt_add_resources(rynet polkit PREFIX /polkit BASE packaging/linux/polkit FILES ${RYNET_POLKIT})
endif()

qt_add_shaders(rynet shaders
    BATCHABLE
    PREFIX /
    BASE shaders
    FILES shaders/state.vert shaders/state.frag shaders/round.frag
)

target_include_directories(rynet PRIVATE src ${QRENCODE_INCLUDE_DIR})
target_compile_definitions(rynet PRIVATE RYNET_VERSION="${RYNET_VERSION}" QT_NO_CAST_FROM_BYTEARRAY QT_NO_URL_CAST_FROM_STRING)
target_link_libraries(rynet PRIVATE Qt6::Core Qt6::Gui Qt6::GuiPrivate Qt6::Network Qt6::Qml Qt6::Quick ZLIB::ZLIB ${QRENCODE_LIBRARY})
if(UNIX AND NOT APPLE)
    target_compile_definitions(rynet PRIVATE RYNET_DBUS)
    target_link_libraries(rynet PRIVATE Qt6::DBus)
endif()
if(WIN32)
    target_compile_definitions(rynet PRIVATE _USE_MATH_DEFINES NOMINMAX WIN32_LEAN_AND_MEAN UNICODE _UNICODE)
    target_link_libraries(rynet PRIVATE dwmapi ole32 shell32 uuid advapi32 wininet user32)
    file(CONFIGURE OUTPUT ${CMAKE_CURRENT_BINARY_DIR}/rynet_version.h CONTENT
        "#define RYNET_VERSION_MAJOR @PROJECT_VERSION_MAJOR@\n#define RYNET_VERSION_MINOR @PROJECT_VERSION_MINOR@\n#define RYNET_VERSION_PATCH @PROJECT_VERSION_PATCH@\n#define RYNET_VERSION_STR \"@PROJECT_VERSION@\"\n")
    target_include_directories(rynet PRIVATE ${CMAKE_CURRENT_BINARY_DIR} ${CMAKE_CURRENT_SOURCE_DIR}/packaging/windows)
    target_sources(rynet PRIVATE packaging/windows/rynet.rc)
    set_property(SOURCE packaging/windows/rynet.rc APPEND PROPERTY OBJECT_DEPENDS
        ${CMAKE_CURRENT_SOURCE_DIR}/packaging/windows/rynet.ico ${CMAKE_CURRENT_SOURCE_DIR}/packaging/windows/rynet.manifest)
endif()

if(APPLE)
    set_target_properties(rynet PROPERTIES
        OUTPUT_NAME ryNet
        MACOSX_BUNDLE_GUI_IDENTIFIER dev.yzewe.rynet
        MACOSX_BUNDLE_BUNDLE_NAME ryNet
        MACOSX_BUNDLE_BUNDLE_VERSION ${RYNET_VERSION}
        MACOSX_BUNDLE_SHORT_VERSION_STRING ${RYNET_VERSION}
        MACOSX_BUNDLE_ICON_FILE rynet.icns
        MACOSX_BUNDLE_INFO_PLIST ${CMAKE_CURRENT_SOURCE_DIR}/packaging/macos/Info.plist.in)
    if(EXISTS ${CMAKE_CURRENT_SOURCE_DIR}/packaging/macos/rynet.icns)
        target_sources(rynet PRIVATE packaging/macos/rynet.icns)
        set_source_files_properties(packaging/macos/rynet.icns PROPERTIES MACOSX_PACKAGE_LOCATION Resources)
    endif()
    target_link_options(rynet PRIVATE -Wl,-dead_strip)
endif()

if(NOT MSVC AND NOT APPLE)
    target_compile_options(rynet PRIVATE -ffunction-sections -fdata-sections)
    target_link_options(rynet PRIVATE -Wl,--gc-sections $<$<CONFIG:Release,MinSizeRel>:-s>)
endif()

install(TARGETS rynet RUNTIME DESTINATION . BUNDLE DESTINATION .)

if(RYNET_TESTS)
    enable_testing()
    find_package(Qt6 REQUIRED COMPONENTS Test)
    qt_add_executable(rynet_tests tests/tests.cpp src/core.cpp src/core.h src/model.cpp src/model_rules.cpp src/model_local.cpp src/model_auto.cpp src/model_backup.cpp src/model.h src/store.cpp src/store.h src/reports.cpp src/reports.h src/platform.cpp src/platform.h src/update.cpp src/update.h)
    target_include_directories(rynet_tests PRIVATE src ${QRENCODE_INCLUDE_DIR})
    target_compile_definitions(rynet_tests PRIVATE RYNET_VERSION="${RYNET_VERSION}")
    target_link_libraries(rynet_tests PRIVATE Qt6::Core Qt6::Gui Qt6::Network Qt6::Test ZLIB::ZLIB)
    if(UNIX AND NOT APPLE)
        target_compile_definitions(rynet_tests PRIVATE RYNET_DBUS)
        target_link_libraries(rynet_tests PRIVATE Qt6::DBus)
    endif()
    if(UNIX AND NOT APPLE)
        qt_add_resources(rynet_tests polkit PREFIX /polkit BASE packaging/linux/polkit FILES ${RYNET_POLKIT})
    endif()
    add_test(NAME rynet_tests COMMAND rynet_tests)

    qt_add_executable(rynet_grid_tests tests/gridtests.cpp ${RYNET_CORE_SOURCES})
    target_include_directories(rynet_grid_tests PRIVATE src ${QRENCODE_INCLUDE_DIR})
    target_compile_definitions(rynet_grid_tests PRIVATE RYNET_VERSION="${RYNET_VERSION}")
    target_link_libraries(rynet_grid_tests PRIVATE Qt6::Core Qt6::Gui Qt6::Network Qt6::Qml Qt6::Quick Qt6::Test ZLIB::ZLIB ${QRENCODE_LIBRARY})
    if(UNIX AND NOT APPLE)
        target_compile_definitions(rynet_grid_tests PRIVATE RYNET_DBUS)
        target_link_libraries(rynet_grid_tests PRIVATE Qt6::DBus)
    endif()
    add_test(NAME rynet_grid_tests COMMAND rynet_grid_tests)
endif()
