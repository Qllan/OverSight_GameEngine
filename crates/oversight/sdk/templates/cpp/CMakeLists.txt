cmake_minimum_required(VERSION 3.16)
project(__NAME__ CXX)

# The OverSight SDK this game was made from; -DCMAKE_PREFIX_PATH=... overrides it
list(APPEND CMAKE_PREFIX_PATH "__SDK__")
find_package(oversight REQUIRED)

add_executable(__NAME__ main.cpp)
target_link_libraries(__NAME__ PRIVATE oversight::oversight)
target_compile_features(__NAME__ PRIVATE cxx_std_17)
if(MSVC)
    target_compile_options(__NAME__ PRIVATE /utf-8)   # the sources are UTF-8
endif()
oversight_copy_runtime(__NAME__)

# Pictures, sounds and maps go to assets/ and are copied next to the game
add_custom_command(TARGET __NAME__ POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_directory
        "${CMAKE_CURRENT_SOURCE_DIR}/assets" "$<TARGET_FILE_DIR:__NAME__>/assets")
