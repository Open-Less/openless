cmake_minimum_required(VERSION 3.16)
project(openless_rime LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT OPENLESS_RIME_PREFIX)
  message(FATAL_ERROR "Build scripts/build-mobile-rime.py first and set OPENLESS_RIME_PREFIX")
endif()
add_library(openless_rime_engine STATIC rime_engine.cpp)
set_target_properties(openless_rime_engine PROPERTIES POSITION_INDEPENDENT_CODE ON)
target_include_directories(openless_rime_engine PUBLIC ${CMAKE_CURRENT_SOURCE_DIR} ${OPENLESS_RIME_PREFIX}/include)
target_link_libraries(openless_rime_engine PUBLIC
  ${OPENLESS_RIME_PREFIX}/lib/librime.a
  ${OPENLESS_RIME_PREFIX}/lib/libyaml-cpp.a
  ${OPENLESS_RIME_PREFIX}/lib/libleveldb.a
  ${OPENLESS_RIME_PREFIX}/lib/libopencc.a
  ${OPENLESS_RIME_PREFIX}/lib/libmarisa.a)
if(ANDROID)
  add_library(openless_rime SHARED android_jni.cpp)
  target_link_libraries(openless_rime PRIVATE openless_rime_engine)
endif()
