# ORST for C and C++: an imported target for the Rust-built
# library plus the headers. Build the library first:
#     cargo build --release -p orst
# then use this directory from your project:
#     add_subdirectory(path/to/bindings/cpp orst)
#     target_link_libraries(my_game PRIVATE orst)
# ORST_LIB_DIR points at the folder with orst.dll /
# liborst.so (default: the workspace's target/release).
cmake_minimum_required(VERSION 3.16)
project(orst_cpp LANGUAGES C CXX)

get_filename_component(ORST_ROOT "${CMAKE_CURRENT_LIST_DIR}/../.." ABSOLUTE)
set(ORST_LIB_DIR "${ORST_ROOT}/target/release" CACHE PATH "Folder with the built orst library")

add_library(orst SHARED IMPORTED GLOBAL)
target_include_directories(orst INTERFACE "${ORST_ROOT}/crates/orst/include")
target_compile_features(orst INTERFACE cxx_std_17)
if(WIN32)
    set_target_properties(orst PROPERTIES
        IMPORTED_LOCATION "${ORST_LIB_DIR}/orst.dll"
        IMPORTED_IMPLIB "${ORST_LIB_DIR}/orst.dll.lib")
elseif(APPLE)
    set_target_properties(orst PROPERTIES IMPORTED_LOCATION "${ORST_LIB_DIR}/liborst.dylib")
else()
    set_target_properties(orst PROPERTIES IMPORTED_LOCATION "${ORST_LIB_DIR}/liborst.so")
endif()

# Copy the library next to an executable so it runs without PATH tweaks
function(orst_copy_runtime target)
    add_custom_command(TARGET ${target} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            $<TARGET_FILE:orst> $<TARGET_FILE_DIR:${target}>)
endfunction()

option(ORST_BUILD_TESTS "Build the C and C++ smoke tests" ON)
if(ORST_BUILD_TESTS AND PROJECT_IS_TOP_LEVEL)
    enable_testing()
    add_executable(smoke_c tests/smoke.c)
    set_target_properties(smoke_c PROPERTIES C_STANDARD 99)
    target_link_libraries(smoke_c PRIVATE orst)
    orst_copy_runtime(smoke_c)
    add_test(NAME smoke_c COMMAND smoke_c)

    add_executable(smoke_cpp tests/smoke.cpp)
    target_link_libraries(smoke_cpp PRIVATE orst)
    orst_copy_runtime(smoke_cpp)
    add_test(NAME smoke_cpp COMMAND smoke_cpp)
    if(NOT MSVC)
        target_compile_options(smoke_c PRIVATE -Wall -Wextra -Werror)
        target_compile_options(smoke_cpp PRIVATE -Wall -Wextra -Werror)
    endif()
endif()
