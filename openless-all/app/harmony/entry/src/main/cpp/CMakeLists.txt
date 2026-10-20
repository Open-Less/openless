cmake_minimum_required(VERSION 3.5.0)
project(openless_harmony)
set(OPENLESS_RIME_PREFIX "$ENV{OPENLESS_RIME_PREFIX}")
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../../../../mobile-ime" "${CMAKE_BINARY_DIR}/mobile-ime")
add_library(openless_harmony_host SHARED IMPORTED)
set_target_properties(openless_harmony_host PROPERTIES
    IMPORTED_LOCATION "${CMAKE_CURRENT_SOURCE_DIR}/../libs/arm64-v8a/libopenless_harmony_host.so"
    IMPORTED_NO_SONAME TRUE)
add_library(openless_harmony SHARED bridge.cpp)
target_link_libraries(openless_harmony PUBLIC openless_harmony_host openless_rime_engine libace_napi.z.so)
