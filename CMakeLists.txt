cmake_minimum_required(VERSION 3.20)
project(ai_cad VERSION 0.1.0 LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()
include(cmake/SMLib.cmake)
include(CTest)
add_executable(kernel_probe tests/kernel_probe.cpp)
target_link_libraries(kernel_probe PRIVATE sm_api)
add_test(NAME kernel_capabilities COMMAND kernel_probe)

add_library(cad-kernel modules/cad-kernel/smlib.cpp)
target_include_directories(cad-kernel PUBLIC modules/cad-kernel)
target_link_libraries(cad-kernel PRIVATE sm_api)
foreach(module cad-document cad-modeling cad-topology)
  add_library(${module} INTERFACE)
  target_include_directories(${module} INTERFACE modules/${module})
endforeach()
target_link_libraries(cad-document INTERFACE cad-kernel)
target_link_libraries(cad-modeling INTERFACE cad-document)
add_library(cad-json INTERFACE)
target_include_directories(cad-json SYSTEM INTERFACE third_party/json)
include(cmake/Protocol.cmake)
add_library(cad-commander modules/cad-commander/commander.cpp)
target_include_directories(cad-commander PUBLIC modules/cad-commander)
target_link_libraries(cad-commander PUBLIC cad-kernel cad-json PRIVATE cad-modeling cad-topology cad-contract)
add_executable(commander_test tests/commander_test.cpp)
target_link_libraries(commander_test PRIVATE cad-commander)
add_test(NAME commander_contract COMMAND commander_test)
set_tests_properties(commander_contract PROPERTIES TIMEOUT 30)

add_library(cad-wire modules/cad-protocol/endpoint.cpp)
target_link_libraries(cad-wire PUBLIC cad-contract PRIVATE cad-commander)
add_executable(cad-server modules/cad-server/main.cpp)
target_link_libraries(cad-server PRIVATE cad-commander cad-wire)
if(BUILD_TESTING)
find_package(Python3 REQUIRED COMPONENTS Interpreter)
add_test(NAME service_transactions COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tests/service_test.py $<TARGET_FILE:cad-server>)
add_test(NAME access_preview_mcp COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tests/access_test.py)
set_tests_properties(kernel_capabilities service_transactions access_preview_mcp PROPERTIES TIMEOUT 30)

set_tests_properties(access_preview_mcp PROPERTIES ENVIRONMENT "CAD_SERVER=$<TARGET_FILE:cad-server>")

add_test(NAME wire_contract COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tests/wire_test.py $<TARGET_FILE:cad-server>)
set_tests_properties(wire_contract PROPERTIES TIMEOUT 30)
endif()
add_executable(protocol_schema_test tests/protocol_schema_test.cpp)
target_link_libraries(protocol_schema_test PRIVATE cad-contract)
add_test(NAME protocol_schema_contract COMMAND protocol_schema_test "${PROJECT_SOURCE_DIR}/protocol/contract.json" "${PROJECT_SOURCE_DIR}/protocol/conformance.json")
set_tests_properties(protocol_schema_contract PROPERTIES TIMEOUT 30)
option(CAD_ENABLE_RUST_TESTS "Include Rust protocol/client contracts in CTest" ON)
if(CAD_ENABLE_RUST_TESTS AND BUILD_TESTING)
  find_program(CAD_CARGO NAMES cargo HINTS "$ENV{HOME}/.cargo/bin" REQUIRED)
  add_test(NAME rust_workspace COMMAND "${CAD_CARGO}" test --workspace --locked)
  set_tests_properties(rust_workspace PROPERTIES WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    ENVIRONMENT "CAD_SERVER=$<TARGET_FILE:cad-server>" TIMEOUT 300)
  add_test(NAME rust_products_build COMMAND "${CAD_CARGO}" build --workspace --locked)
  set_tests_properties(rust_products_build PROPERTIES WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}"
    FIXTURES_SETUP rust_products TIMEOUT 300)
  add_test(NAME rust_products_access COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tests/product_test.py)
  set_tests_properties(rust_products_access PROPERTIES FIXTURES_REQUIRED rust_products
    ENVIRONMENT "CAD_SERVER=$<TARGET_FILE:cad-server>" TIMEOUT 120)
  option(CAD_ENABLE_GUI_TESTS "Run isolated Xvfb desktop acceptance (Linux, requires Xvfb/ffmpeg/XTest)" OFF)
  if(CAD_ENABLE_GUI_TESTS)
    find_program(CAD_XVFB NAMES Xvfb REQUIRED)
    find_program(CAD_FFMPEG NAMES ffmpeg REQUIRED)
    find_program(CAD_XWININFO NAMES xwininfo REQUIRED)
    add_test(NAME desktop_workspace COMMAND ${Python3_EXECUTABLE} ${PROJECT_SOURCE_DIR}/tests/gui_smoke.py)
    set_tests_properties(desktop_workspace PROPERTIES FIXTURES_REQUIRED rust_products
      WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" ENVIRONMENT "CAD_BIN=${PROJECT_SOURCE_DIR}/target/debug;CAD_SERVER=$<TARGET_FILE:cad-server>"
      TIMEOUT 120 RUN_SERIAL TRUE)
  endif()

endif()

install(TARGETS cad-server RUNTIME DESTINATION bin)

option(CAD_ENABLE_WEB_TESTS "Run actual Chromium/WASM acceptance (requires prebuilt Web assets and Playwright)" OFF)
if(CAD_ENABLE_WEB_TESTS AND CAD_ENABLE_RUST_TESTS AND BUILD_TESTING)
  find_program(CAD_NODE NAMES node REQUIRED)
  add_test(NAME web_tablet_workspace COMMAND "${CAD_NODE}" "${PROJECT_SOURCE_DIR}/tests/web_tablet.cjs")
  set_tests_properties(web_tablet_workspace PROPERTIES FIXTURES_REQUIRED rust_products
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" ENVIRONMENT "CAD_BIN=${PROJECT_SOURCE_DIR}/target/debug;CAD_TEST_DPR=2"
    TIMEOUT 180 RUN_SERIAL TRUE)
  add_test(NAME web_workspace COMMAND "${CAD_NODE}" "${PROJECT_SOURCE_DIR}/tests/web_smoke.cjs")
  add_test(NAME web_micro_fillet_picking COMMAND "${CAD_NODE}" "${PROJECT_SOURCE_DIR}/tests/web_micro_fillet.cjs")
  set_tests_properties(web_micro_fillet_picking PROPERTIES FIXTURES_REQUIRED rust_products
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" ENVIRONMENT "CAD_BIN=${PROJECT_SOURCE_DIR}/target/debug;CAD_TEST_DPR=2"
    TIMEOUT 120 RUN_SERIAL TRUE)
  add_test(NAME web_touch_and_picking COMMAND "${CAD_NODE}" "${PROJECT_SOURCE_DIR}/tests/web_interaction.cjs")
  set_tests_properties(web_touch_and_picking PROPERTIES FIXTURES_REQUIRED rust_products
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" ENVIRONMENT "CAD_BIN=${PROJECT_SOURCE_DIR}/target/debug;CAD_TEST_DPR=2"
    TIMEOUT 180 RUN_SERIAL TRUE)
  set_tests_properties(web_workspace PROPERTIES FIXTURES_REQUIRED rust_products
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" ENVIRONMENT "CAD_BIN=${PROJECT_SOURCE_DIR}/target/debug"
    TIMEOUT 180 RUN_SERIAL TRUE)
endif()

option(CAD_ENABLE_AGENT_HOST_TESTS "Verify MCP discovery in the installed real Codex host (no inference)" OFF)
if(CAD_ENABLE_AGENT_HOST_TESTS AND CAD_ENABLE_RUST_TESTS AND BUILD_TESTING)
  find_program(CAD_CODEX NAMES codex REQUIRED)
  add_test(NAME agent_host_mcp COMMAND ${Python3_EXECUTABLE} "${PROJECT_SOURCE_DIR}/tests/codex_mcp_host.py")
  set_tests_properties(agent_host_mcp PROPERTIES FIXTURES_REQUIRED rust_products
    WORKING_DIRECTORY "${PROJECT_SOURCE_DIR}" TIMEOUT 120 RUN_SERIAL TRUE)
endif()
