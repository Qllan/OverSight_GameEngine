# The C++ samples. Build the engine first (cargo build --release -p orst),
# then:
#     cmake -S samples/cpp -B build/samples
#     cmake --build build/samples --config Release
# Run with --headless 200 --screenshot shot.png to play a scripted run
# without a window (used by CI).
cmake_minimum_required(VERSION 3.16)
project(orst_samples LANGUAGES C CXX)

add_subdirectory(../../bindings/cpp orst)

foreach(sample topdown platformer isometric)
    add_executable(${sample} ${sample}.cpp common.hpp)
    target_link_libraries(${sample} PRIVATE orst)
    target_compile_definitions(${sample} PRIVATE ORST_SAMPLE_ASSETS="${CMAKE_CURRENT_LIST_DIR}/../assets")
    if(MSVC)
        # The sources are UTF-8 (Cyrillic strings)
        target_compile_options(${sample} PRIVATE /utf-8 /W4)
    else()
        target_compile_options(${sample} PRIVATE -Wall -Wextra)
    endif()
    orst_copy_runtime(${sample})
endforeach()
