# The C++ samples. Build the engine first (cargo build --release -p oversight),
# then:
#     cmake -S samples/cpp -B build/samples
#     cmake --build build/samples --config Release
# Run with --headless 200 --screenshot shot.png to play a scripted run
# without a window (used by CI).
cmake_minimum_required(VERSION 3.16)
project(oversight_samples LANGUAGES C CXX)

add_subdirectory(../../bindings/cpp oversight)

foreach(sample topdown platformer isometric)
    add_executable(${sample} ${sample}.cpp common.hpp)
    target_link_libraries(${sample} PRIVATE oversight)
    target_compile_definitions(${sample} PRIVATE OV_SAMPLE_ASSETS="${CMAKE_CURRENT_LIST_DIR}/../assets")
    if(MSVC)
        # The sources are UTF-8 (Cyrillic strings)
        target_compile_options(${sample} PRIVATE /utf-8 /W4)
    else()
        target_compile_options(${sample} PRIVATE -Wall -Wextra)
    endif()
    oversight_copy_runtime(${sample})
endforeach()
