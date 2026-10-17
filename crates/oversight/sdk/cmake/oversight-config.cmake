# OverSight 2D for CMake: find_package(oversight) from the SDK folder.
#     list(APPEND CMAKE_PREFIX_PATH "C:/path/to/oversight-sdk")
#     find_package(oversight REQUIRED)
#     target_link_libraries(my_game PRIVATE oversight::oversight)
#     oversight_copy_runtime(my_game)
get_filename_component(_oversight_root "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)

if(NOT TARGET oversight::oversight)
    add_library(oversight::oversight SHARED IMPORTED)
    set_target_properties(oversight::oversight PROPERTIES
        INTERFACE_INCLUDE_DIRECTORIES "${_oversight_root}/include"
        INTERFACE_COMPILE_FEATURES cxx_std_17)
    if(WIN32)
        set_target_properties(oversight::oversight PROPERTIES
            IMPORTED_LOCATION "${_oversight_root}/bin/oversight.dll"
            IMPORTED_IMPLIB "${_oversight_root}/lib/oversight.dll.lib")
    else()
        # The library has no soname: without this the executable would
        # remember the SDK path instead of liboversight.so
        set_target_properties(oversight::oversight PROPERTIES
            IMPORTED_LOCATION "${_oversight_root}/lib/liboversight.so"
            IMPORTED_NO_SONAME TRUE)
    endif()
endif()

# Copy the library next to an executable so the game runs from its folder
function(oversight_copy_runtime target)
    add_custom_command(TARGET ${target} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            $<TARGET_FILE:oversight::oversight> $<TARGET_FILE_DIR:${target}>)
    if(NOT WIN32)
        set_target_properties(${target} PROPERTIES BUILD_RPATH "$ORIGIN" INSTALL_RPATH "$ORIGIN")
    endif()
endfunction()

unset(_oversight_root)
