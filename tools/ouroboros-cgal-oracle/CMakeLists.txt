cmake_minimum_required(VERSION 3.21)
project(ouroboros_cgal_oracle VERSION 0.1.0 LANGUAGES CXX)

# Pin the last stable release observed for the 2026-11-12 qualification pass.
find_package(CGAL 6.2.1 EXACT REQUIRED)

add_executable(ouroboros-cgal-oracle src/main.cpp)
target_compile_features(ouroboros-cgal-oracle PRIVATE cxx_std_20)
target_link_libraries(ouroboros-cgal-oracle PRIVATE CGAL::CGAL)
target_compile_options(ouroboros-cgal-oracle PRIVATE
  $<$<CXX_COMPILER_ID:GNU,Clang>:-Wall;-Wextra;-Wpedantic;-Wconversion;-Wshadow>
)

enable_testing()
add_test(NAME ouroboros-cgal-oracle-self-test
  COMMAND ouroboros-cgal-oracle --self-test
)
