# OverSight 2D for C and C++: an imported target for the Rust-built
# library plus the headers. Build the library first:
#     cargo build --release -p oversight
# then use this directory from your project:
#     add_subdirectory(path/to/bindings/cpp oversight)
#     target_link_libraries(my_game PRIVATE oversight)
# OVERSIGHT_LIB_DIR points at the folder with oversight.dll /
# liboversight.so (default: the workspace's target/release).
cmake_minimum_required(VERSION 3.16)
project(oversight_cpp LANGUAGES C CXX)

get_filename_component(OVERSIGHT_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
set(OVERSIGHT_LIB_DIR "${OVERSIGHT_ROOT}/target/release" CACHE PATH "Folder with the built oversight library")

add_library(oversight SHARED IMPORTED GLOBAL)
target_include_directories(oversight INTERFACE "${OVERSIGHT_ROOT}/crates/oversight/include")
target_compile_features(oversight INTERFACE cxx_std_17)
if(WIN32)
    set_target_properties(oversight PROPERTIES
        IMPORTED_LOCATION "${OVERSIGHT_LIB_DIR}/oversight.dll"
        IMPORTED_IMPLIB "${OVERSIGHT_LIB_DIR}/oversight.dll.lib")
elseif(APPLE)
    set_target_properties(oversight PROPERTIES IMPORTED_LOCATION "${OVERSIGHT_LIB_DIR}/liboversight.dylib")
else()
    set_target_properties(oversight PROPERTIES IMPORTED_LOCATION "${OVERSIGHT_LIB_DIR}/liboversight.so")
endif()

# Copy the library next to an executable so it runs without PATH tweaks
function(oversight_copy_runtime target)
    add_custom_command(TARGET ${target} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            $<TARGET_FILE:oversight> $<TARGET_FILE_DIR:${target}>)
endfunction()

option(OVERSIGHT_BUILD_TESTS "Build the C and C++ smoke tests" ON)
if(OVERSIGHT_BUILD_TESTS AND PROJECT_IS_TOP_LEVEL)
    enable_testing()
    add_executable(smoke_c tests/smoke.c)
    set_target_properties(smoke_c PROPERTIES C_STANDARD 99)
    target_link_libraries(smoke_c PRIVATE oversight)
    oversight_copy_runtime(smoke_c)
    add_test(NAME smoke_c COMMAND smoke_c)

    add_executable(smoke_cpp tests/smoke.cpp)
    target_link_libraries(smoke_cpp PRIVATE oversight)
    oversight_copy_runtime(smoke_cpp)
    add_test(NAME smoke_cpp COMMAND smoke_cpp)
    if(NOT MSVC)
        target_compile_options(smoke_c PRIVATE -Wall -Wextra -Werror)
        target_compile_options(smoke_cpp PRIVATE -Wall -Wextra -Werror)
    endif()
endif()
