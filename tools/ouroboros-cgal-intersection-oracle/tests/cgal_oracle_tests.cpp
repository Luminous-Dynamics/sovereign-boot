#include "ourodc/cgal_oracle.hpp"

#include <array>
#include <cstdint>
#include <exception>
#include <iostream>
#include <stdexcept>
#include <vector>

namespace {

using Triangle = std::array<std::uint32_t, 3>;

ourodc::Artifact tetra_pair(std::int64_t second_x_offset) {
  ourodc::Artifact artifact;
  const std::array<std::array<std::int64_t, 3>, 4> tetra{{
      {{0, 0, 0}},
      {{1'000'000'000, 0, 0}},
      {{0, 1'000'000'000, 0}},
      {{0, 0, 1'000'000'000}},
  }};
  for (const auto& point : tetra) {
    ourodc::Vertex vertex;
    vertex.position_nanounits = point;
    vertex.normal_nanounits = {0, 0, 1'000'000'000};
    vertex.feature_mask = 9;
    artifact.vertices.push_back(vertex);
  }
  for (const auto& point : tetra) {
    ourodc::Vertex vertex;
    vertex.position_nanounits = {point[0] + second_x_offset, point[1], point[2]};
    vertex.normal_nanounits = {0, 0, 1'000'000'000};
    vertex.feature_mask = 9;
    artifact.vertices.push_back(vertex);
  }
  const std::array<Triangle, 4> faces{{
      {{0, 2, 1}},
      {{0, 1, 3}},
      {{0, 3, 2}},
      {{1, 2, 3}},
  }};
  for (const auto& face : faces) artifact.triangles.push_back(face);
  for (const auto& face : faces) {
    artifact.triangles.push_back(
        {face[0] + 4, face[1] + 4, face[2] + 4});
  }
  return artifact;
}

ourodc::Artifact two_triangle_artifact(
    const std::array<std::array<std::int64_t, 3>, 6>& positions) {
  ourodc::Artifact artifact;
  for (const auto& position : positions) {
    ourodc::Vertex vertex;
    vertex.position_nanounits = position;
    vertex.normal_nanounits = {0, 0, 1'000'000'000};
    vertex.feature_mask = 9;
    artifact.vertices.push_back(vertex);
  }
  artifact.triangles.push_back({0, 1, 2});
  artifact.triangles.push_back({3, 4, 5});
  return artifact;
}

void require(bool condition, const char* detail) {
  if (!condition) throw std::runtime_error(detail);
}

void run() {
  const auto disjoint = tetra_pair(3'000'000'000LL);
  const auto disjoint_result = ourodc::inspect_self_intersections(disjoint);
  require(disjoint_result.surface_construction_ok,
          "disjoint tetrahedra should construct as a surface mesh");
  require(disjoint_result.status == "Pass" &&
              disjoint_result.intersecting_triangle_pairs.empty(),
          "disjoint tetrahedra must produce an empty intersection list");

  const auto overlapping = tetra_pair(250'000'000LL);
  const auto overlap_result = ourodc::inspect_self_intersections(overlapping);
  require(overlap_result.surface_construction_ok,
          "overlapping disconnected tetrahedra should still construct");
  require(overlap_result.status == "Fail" &&
              !overlap_result.intersecting_triangle_pairs.empty(),
          "overlapping tetrahedra must produce intersecting face pairs");

  const auto repeated = ourodc::inspect_self_intersections(overlapping);
  require(repeated.intersecting_triangle_pairs == overlap_result.intersecting_triangle_pairs,
          "CGAL pair enumeration must be deterministic for identical input");

  auto degenerate = disjoint;
  degenerate.triangles[0] = {0, 0, 1};
  const auto degenerate_result = ourodc::inspect_self_intersections(degenerate);
  require(degenerate_result.status == "NotEvaluated" &&
              degenerate_result.reason == "DegenerateTriangle",
          "degenerate triangle must not be called intersection-free");

  auto inconsistent = disjoint;
  inconsistent.triangles[0] = {0, 1, 2};
  const auto inconsistent_result = ourodc::inspect_self_intersections(inconsistent);
  require(inconsistent_result.status == "NotEvaluated",
          "inconsistent winding must not be called intersection-free");

  const auto coplanar_overlap = two_triangle_artifact({{
      {{0, 0, 0}}, {{2'000'000'000, 0, 0}}, {{0, 2'000'000'000, 0}},
      {{500'000'000, 500'000'000, 0}}, {{2'000'000'000, 500'000'000, 0}},
      {{500'000'000, 2'000'000'000, 0}},
  }});
  const auto coplanar_result = ourodc::inspect_self_intersections(coplanar_overlap);
  require(coplanar_result.status == "Fail" &&
              coplanar_result.intersecting_triangle_pairs ==
                  std::vector<std::pair<std::uint32_t, std::uint32_t>>{{0, 1}},
          "coplanar overlapping faces must be reported by original face index");

  const auto coincident = two_triangle_artifact({{
      {{0, 0, 0}}, {{1'000'000'000, 0, 0}}, {{0, 1'000'000'000, 0}},
      {{0, 0, 0}}, {{1'000'000'000, 0, 0}}, {{0, 1'000'000'000, 0}},
  }});
  const auto coincident_result = ourodc::inspect_self_intersections(coincident);
  require(coincident_result.status == "Fail" &&
              !coincident_result.intersecting_triangle_pairs.empty(),
          "coincident faces with distinct vertex IDs must not disappear");

  const auto one_nanounit_separation = two_triangle_artifact({{
      {{0, 0, 0}}, {{1'000'000'000, 0, 0}}, {{0, 1'000'000'000, 0}},
      {{0, 0, 1}}, {{1'000'000'000, 0, 1}}, {{0, 1'000'000'000, 1}},
  }});
  const auto separated_result = ourodc::inspect_self_intersections(one_nanounit_separation);
  require(separated_result.status == "Pass" &&
              separated_result.intersecting_triangle_pairs.empty(),
          "one-nanounit parallel separation must remain exactly disjoint");
}

}  // namespace

int main() {
  try {
    run();
    std::cout << "ourodc CGAL oracle tests: PASS\n";
    return 0;
  } catch (const std::exception& error) {
    std::cerr << "ourodc CGAL oracle tests: FAIL: " << error.what() << '\n';
    return 1;
  }
}
