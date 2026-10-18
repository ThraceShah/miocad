set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${PROJECT_SOURCE_DIR}/protocol/contract.json")
file(READ "${PROJECT_SOURCE_DIR}/protocol/contract.json" CAD_CONTRACT_JSON)
configure_file("${PROJECT_SOURCE_DIR}/modules/cad-protocol/contract.h.in"
  "${PROJECT_BINARY_DIR}/generated/cad_contract.h" @ONLY)
add_library(cad-contract modules/cad-protocol/protocol.cpp)
target_include_directories(cad-contract PUBLIC modules/cad-protocol PRIVATE "${PROJECT_BINARY_DIR}/generated")
target_link_libraries(cad-contract PUBLIC cad-json)
