cmake_minimum_required(VERSION 3.12)
# C 是必需的：wayland-scanner 生成的协议胶水是 .c 文件，项目只声明 CXX 时
# CMake 会把它当成头文件跳过（体现为链接期 undefined reference）。
project(openless-fcitx5-plugin VERSION 1.0.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)
find_package(Fcitx5Module REQUIRED COMPONENTS DBus Clipboard)
find_package(PkgConfig REQUIRED)
pkg_check_modules(WAYLAND_CLIENT REQUIRED wayland-client)

# FCITX_INSTALL_*DIR comes from Fcitx5Utils
message(STATUS "FCITX_INSTALL_LIBDIR: ${FCITX_INSTALL_LIBDIR}")
message(STATUS "FCITX_INSTALL_PKGDATADIR: ${FCITX_INSTALL_PKGDATADIR}")
message(STATUS "FCITX_INSTALL_ADDONDIR: ${FCITX_INSTALL_ADDONDIR}")

# 选区探针（primary_selection.cpp）需要 ext-data-control 的客户端绑定：
# 插件必须自己看一眼合成器上的 PRIMARY，才能分辨“选中的不是文本”与“没有选区”
# （fcitx5 的 clipboard addon 没有这种通知，且会在新选区没有 text mime 时静默
# 保留旧值——见 primary_selection.h）。协议 XML 内置在 protocols/ 下。
find_program(WAYLAND_SCANNER NAMES wayland-scanner REQUIRED)
set(EXT_DATA_CONTROL_XML
    "${CMAKE_CURRENT_SOURCE_DIR}/protocols/ext-data-control-v1.xml")
set(EXT_DATA_CONTROL_DIR "${CMAKE_CURRENT_BINARY_DIR}/protocols")
set(EXT_DATA_CONTROL_HEADER
    "${EXT_DATA_CONTROL_DIR}/ext-data-control-v1-client-protocol.h")
set(EXT_DATA_CONTROL_CODE
    "${EXT_DATA_CONTROL_DIR}/ext-data-control-v1-protocol.c")
file(MAKE_DIRECTORY "${EXT_DATA_CONTROL_DIR}")
add_custom_command(
    OUTPUT "${EXT_DATA_CONTROL_HEADER}"
    COMMAND "${WAYLAND_SCANNER}" client-header "${EXT_DATA_CONTROL_XML}"
            "${EXT_DATA_CONTROL_HEADER}"
    DEPENDS "${EXT_DATA_CONTROL_XML}"
    COMMENT "Generating ext-data-control-v1 client header")
add_custom_command(
    OUTPUT "${EXT_DATA_CONTROL_CODE}"
    COMMAND "${WAYLAND_SCANNER}" private-code "${EXT_DATA_CONTROL_XML}"
            "${EXT_DATA_CONTROL_CODE}"
    DEPENDS "${EXT_DATA_CONTROL_XML}"
    COMMENT "Generating ext-data-control-v1 protocol glue")

add_library(openless MODULE
    openless.cpp
    primary_selection.cpp
    "${EXT_DATA_CONTROL_CODE}"
    "${EXT_DATA_CONTROL_HEADER}")
target_include_directories(openless PRIVATE
    "${EXT_DATA_CONTROL_DIR}"
    ${WAYLAND_CLIENT_INCLUDE_DIRS})
target_link_libraries(openless PRIVATE
    Fcitx5::Core
    Fcitx5::Utils
    Fcitx5::Module::Clipboard
    ${WAYLAND_CLIENT_LIBRARIES})

# Locate fcitx5 module headers (e.g. fcitx-module/dbus/dbus_public.h)
find_path(FCITX5_MODULE_INCLUDE_DIR
    NAMES "fcitx-module/dbus/dbus_public.h"
    HINTS "/usr/include/Fcitx5/Module"
    PATH_SUFFIXES "include/Fcitx5/Module"
)
if(FCITX5_MODULE_INCLUDE_DIR)
    target_include_directories(openless PRIVATE "${FCITX5_MODULE_INCLUDE_DIR}")
    message(STATUS "FCITX5_MODULE_INCLUDE_DIR: ${FCITX5_MODULE_INCLUDE_DIR}")
else()
    message(FATAL_ERROR "Cannot find fcitx5 module headers (fcitx-module/dbus/dbus_public.h)")
endif()

include(CTest)
if(BUILD_TESTING)
    add_executable(input_target_contract
        input_target_contract.cpp
        primary_selection.cpp
        "${EXT_DATA_CONTROL_CODE}"
        "${EXT_DATA_CONTROL_HEADER}")
    # Release packaging runs this executable too; assertions are its contract,
    # so NDEBUG must never turn the validation into a no-op.
    target_compile_options(input_target_contract PRIVATE -UNDEBUG)
    target_include_directories(input_target_contract PRIVATE
        "${FCITX5_MODULE_INCLUDE_DIR}"
        "${EXT_DATA_CONTROL_DIR}"
        ${WAYLAND_CLIENT_INCLUDE_DIRS})
    target_link_libraries(input_target_contract PRIVATE
        Fcitx5::Core Fcitx5::Utils Fcitx5::Module::Clipboard
        ${WAYLAND_CLIENT_LIBRARIES})
    add_test(NAME input_target_contract COMMAND input_target_contract)

    # Pure hotkey-matching contract: Shift+symbol / letter folding and the
    # "never consume a bare modifier" rule (the Ctrl+Shift+; regression).
    add_executable(hotkey_match_contract hotkey_match_contract.cpp)
    target_compile_options(hotkey_match_contract PRIVATE -UNDEBUG)
    target_link_libraries(hotkey_match_contract PRIVATE Fcitx5::Utils)
    add_test(NAME hotkey_match_contract COMMAND hotkey_match_contract)
endif()

# Install the plugin .so to fcitx5 addon dir
install(TARGETS openless
    LIBRARY DESTINATION "${FCITX_INSTALL_ADDONDIR}")

# Generate and install addon config
configure_file(
    openless.conf.in
    openless.conf
    @ONLY)
install(
    FILES "${CMAKE_CURRENT_BINARY_DIR}/openless.conf"
    DESTINATION "${FCITX_INSTALL_PKGDATADIR}/addon")
