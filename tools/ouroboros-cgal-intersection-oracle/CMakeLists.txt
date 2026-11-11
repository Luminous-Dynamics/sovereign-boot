cmake_minimum_required(VERSION 3.24)
project(ouroboros_cgal_intersection_oracle VERSION 0.1.0 LANGUAGES CXX)

include(CTest)
find_package(OpenSSL REQUIRED COMPONENTS Crypto)

add_library(ourodc_artifact STATIC src/artifact.cpp)
target_include_directories(ourodc_artifact PUBLIC include)
target_compile_features(ourodc_artifact PUBLIC cxx_std_20)
target_link_libraries(ourodc_artifact PUBLIC OpenSSL::Crypto)
target_compile_options(ourodc_artifact PRIVATE -Wall -Wextra -Wpedantic)

if(BUILD_TESTING)
  add_executable(ourodc_artifact_tests tests/artifact_tests.cpp)
  target_link_libraries(ourodc_artifact_tests PRIVATE ourodc_artifact)
  target_compile_features(ourodc_artifact_tests PRIVATE cxx_std_20)
  target_compile_options(ourodc_artifact_tests PRIVATE -Wall -Wextra -Wpedantic)
  add_test(NAME ourodc_artifact_tests COMMAND ourodc_artifact_tests)
endif()

option(OUROBOROS_BUILD_CGAL_ORACLE "Build exact-predicate CGAL self-intersection oracle" OFF)
if(OUROBOROS_BUILD_CGAL_ORACLE)
  find_package(CGAL 6.2.1 EXACT REQUIRED)
  add_executable(ouroboros-cgal-intersection-oracle
    src/main.cpp
    src/cgal_oracle.cpp
  )
  target_link_libraries(ouroboros-cgal-intersection-oracle
    PRIVATE ourodc_artifact CGAL::CGAL
  )
  target_compile_features(ouroboros-cgal-intersection-oracle PRIVATE cxx_std_20)
  target_compile_options(ouroboros-cgal-intersection-oracle PRIVATE -Wall -Wextra -Wpedantic)
endif()

if(BUILD_TESTING AND OUROBOROS_BUILD_CGAL_ORACLE)
  add_executable(ourodc_cgal_oracle_tests tests/cgal_oracle_tests.cpp src/cgal_oracle.cpp)
  target_link_libraries(ourodc_cgal_oracle_tests PRIVATE ourodc_artifact CGAL::CGAL)
  target_compile_features(ourodc_cgal_oracle_tests PRIVATE cxx_std_20)
  target_compile_options(ourodc_cgal_oracle_tests PRIVATE -Wall -Wextra -Wpedantic)
  add_test(NAME ourodc_cgal_oracle_tests COMMAND ourodc_cgal_oracle_tests)
endif()
