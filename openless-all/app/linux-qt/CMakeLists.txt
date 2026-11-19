cmake_minimum_required(VERSION 3.20)
project(openless-qt LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets Network WebEngineWidgets)
find_package(LayerShellQt REQUIRED)

add_executable(openless-qt src/main.cpp)
target_link_libraries(openless-qt PRIVATE
  Qt6::Core Qt6::Gui Qt6::Widgets Qt6::Network Qt6::WebEngineWidgets
  LayerShellQt::Interface
)
install(TARGETS openless-qt RUNTIME DESTINATION lib/openless)
