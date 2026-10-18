# ORST for CMake: find_package(orst) from the SDK folder.
#     list(APPEND CMAKE_PREFIX_PATH "C:/path/to/orst-sdk")
#     find_package(orst REQUIRED)
#     target_link_libraries(my_game PRIVATE orst::orst)
#     orst_copy_runtime(my_game)
get_filename_component(_orst_root "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)

if(NOT TARGET orst::orst)
    add_library(orst::orst SHARED IMPORTED)
    set_target_properties(orst::orst PROPERTIES
        INTERFACE_INCLUDE_DIRECTORIES "${_orst_root}/include"
        INTERFACE_COMPILE_FEATURES cxx_std_17)
    if(WIN32)
        set_target_properties(orst::orst PROPERTIES
            IMPORTED_LOCATION "${_orst_root}/bin/orst.dll"
            IMPORTED_IMPLIB "${_orst_root}/lib/orst.dll.lib")
    else()
        # The library has no soname: without this the executable would
        # remember the SDK path instead of liborst.so
        set_target_properties(orst::orst PROPERTIES
            IMPORTED_LOCATION "${_orst_root}/lib/liborst.so"
            IMPORTED_NO_SONAME TRUE)
    endif()
endif()

# Copy the library next to an executable so the game runs from its folder
function(orst_copy_runtime target)
    add_custom_command(TARGET ${target} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            $<TARGET_FILE:orst::orst> $<TARGET_FILE_DIR:${target}>)
    if(NOT WIN32)
        set_target_properties(${target} PROPERTIES BUILD_RPATH "$ORIGIN" INSTALL_RPATH "$ORIGIN")
    endif()
endfunction()

unset(_orst_root)
