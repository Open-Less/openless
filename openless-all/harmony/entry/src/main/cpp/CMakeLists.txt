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

# `bridge.cpp` links the Rust core as an imported shared library, so the HAP must
# ship `libopenless_harmony_host.so` next to `libopenless_harmony.so` or the NAPI
# module fails to load at runtime ("Error loading shared library
# libopenless_harmony_host.so ... load failed"). hvigor only packages what the
# native build emits, so stage the Rust library into the CMake output directory;
# `entry/src/main/libs` on its own is never packaged.
add_custom_command(TARGET openless_harmony POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
        "${CMAKE_CURRENT_SOURCE_DIR}/../libs/arm64-v8a/libopenless_harmony_host.so"
        "$<TARGET_FILE_DIR:openless_harmony>/libopenless_harmony_host.so"
    COMMENT "Staging libopenless_harmony_host.so for the HAP")
