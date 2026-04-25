cmake_minimum_required(VERSION 3.16)
project(dolphin CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# SFML を自動ダウンロード & ビルド (git不要、tarball取得)
include(FetchContent)
FetchContent_Declare(
    SFML
    URL      https://github.com/SFML/SFML/archive/refs/tags/2.6.2.tar.gz
    URL_HASH SHA256=15ff4d608a018f287c6a885db0a2da86ea389e516d2323629e4d4407a7ce047f
)
set(SFML_BUILD_EXAMPLES   OFF CACHE BOOL "" FORCE)
set(SFML_BUILD_DOC        OFF CACHE BOOL "" FORCE)
set(SFML_BUILD_TEST_SUITE OFF CACHE BOOL "" FORCE)
FetchContent_MakeAvailable(SFML)

add_executable(dolphin
    src/main.cpp
    src/interpreter/DolphinInterpreter.cpp
    src/interpreter/Builtins.cpp
    src/interpreter/GraphicsBuiltins.cpp
)

target_include_directories(dolphin PRIVATE src/interpreter)
target_link_libraries(dolphin PRIVATE sfml-graphics sfml-window sfml-system)

# Windows: DLL を実行ファイルと同じ場所にコピー
if(WIN32)
    add_custom_command(TARGET dolphin POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            $<TARGET_RUNTIME_DLLS:dolphin>
            $<TARGET_FILE_DIR:dolphin>
        COMMAND_EXPAND_LISTS
    )
endif()
