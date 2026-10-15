# Optional system/local OCCT SDK. Link only modeling, mesh and STEP toolkits.
# Avoid full OpenCASCADE config: some distributions make it require VTK/GUI.
set(OCCT_ROOT "" CACHE PATH "Optional OCCT prefix (e.g. extracted Debian SDK/usr)")
find_path(OCCT_INCLUDE_DIR Standard_Version.hxx
  HINTS "${OCCT_ROOT}/include/opencascade" PATH_SUFFIXES opencascade REQUIRED)
set(occt_toolkits TKernel TKMath TKG2d TKG3d TKGeomBase TKBRep TKGeomAlgo
  TKTopAlgo TKPrim TKBO TKShHealing TKBool TKFillet TKMesh TKXSBase TKDESTEP)
add_library(cad-occt-sdk INTERFACE)
target_include_directories(cad-occt-sdk SYSTEM INTERFACE "${OCCT_INCLUDE_DIR}")
foreach(toolkit IN LISTS occt_toolkits)
  find_library(OCCT_${toolkit}_LIBRARY NAMES ${toolkit}
    HINTS "${OCCT_ROOT}/lib" "${OCCT_ROOT}/lib/${CMAKE_LIBRARY_ARCHITECTURE}" REQUIRED)
  target_link_libraries(cad-occt-sdk INTERFACE "${OCCT_${toolkit}_LIBRARY}")
endforeach()
if(OCCT_ROOT AND CMAKE_SYSTEM_NAME STREQUAL "Linux")
  # Extracted SDK libraries have no RUNPATH of their own. Use build RPATH so
  # their transitive dependencies resolve in the same local SDK directory.
  target_link_options(cad-occt-sdk INTERFACE "LINKER:--disable-new-dtags")
endif()
