# Checks the packaged SDK the way a game uses it: find_package, link,
# run the C and C++ smoke tests. CI configures it with
#     cmake -S crates/oversight/sdk/test -B build/sdk -DCMAKE_PREFIX_PATH=<sdk>
cmake_minimum_required(VERSION 3.16)
project(oversight_sdk_test LANGUAGES C CXX)

find_package(oversight 0.1 REQUIRED)

enable_testing()
set(tests "${CMAKE_CURRENT_LIST_DIR}/../../../../bindings/cpp/tests")
add_executable(sdk_smoke_c "${tests}/smoke.c")
set_target_properties(sdk_smoke_c PROPERTIES C_STANDARD 99)
add_executable(sdk_smoke_cpp "${tests}/smoke.cpp")
foreach(t sdk_smoke_c sdk_smoke_cpp)
    target_link_libraries(${t} PRIVATE oversight::oversight)
    oversight_copy_runtime(${t})
    add_test(NAME ${t} COMMAND ${t})
endforeach()
