# Copies the engine's assets/ folder next to an example: cmake -DSRC=<dir> -DDST=<dir> -P copy-assets.cmake
# Every example target copies into the same folder; a parallel build runs these
# commands at the same time, so they take turns on a lock file, held until this
# script ends (GUARD FILE crashes CMake on arm64 Linux with glibc 2.28).
file(MAKE_DIRECTORY "${DST}")
file(LOCK "${DST}.lock" GUARD PROCESS TIMEOUT 300 RESULT_VARIABLE locked)
if(NOT locked STREQUAL "0")
    message(FATAL_ERROR "could not lock ${DST}.lock: ${locked}")
endif()
file(COPY "${SRC}/" DESTINATION "${DST}")
