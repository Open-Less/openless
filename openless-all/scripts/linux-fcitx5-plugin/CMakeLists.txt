cmake_minimum_required(VERSION 3.12)
project(openless-fcitx5-plugin VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)
find_package(Fcitx5Module REQUIRED COMPONENTS DBus)

# FCITX_INSTALL_*DIR comes from Fcitx5Utils
message(STATUS "FCITX_INSTALL_LIBDIR: ${FCITX_INSTALL_LIBDIR}")
message(STATUS "FCITX_INSTALL_PKGDATADIR: ${FCITX_INSTALL_PKGDATADIR}")
message(STATUS "FCITX_INSTALL_ADDONDIR: ${FCITX_INSTALL_ADDONDIR}")

add_library(openless MODULE openless.cpp)
target_link_libraries(openless PRIVATE
    Fcitx5::Core
    Fcitx5::Utils)

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
