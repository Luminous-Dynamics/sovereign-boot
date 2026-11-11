#include "ourodc/cgal_oracle.hpp"

#include <CGAL/Exact_predicates_exact_constructions_kernel.h>
#include <CGAL/Polygon_mesh_processing/self_intersections.h>
#include <CGAL/Surface_mesh.h>

#include <algorithm>
#include <chrono>
#include <iterator>
#include <limits>
#include <vector>

namespace ourodc {
namespace {

using Kernel = CGAL::Exact_predicates_exact_constructions_kernel;
using Exact = Kernel::FT;
using Point = Kernel::Point_3;
using Mesh = CGAL::Surface_mesh<Point>;
namespace PMP = CGAL::Polygon_mesh_processing;

Exact exact_coordinate(std::int64_t nanounits) {
  // Integer construction and division are exact in EPECK: the serialized
  // geometry remains on its declared 1e-9 coordinate lattice.
  return Exact(nanounits) / Exact(kPositionScale);
}

}  // namespace

IntersectionResult inspect_self_intersections(const Artifact& artifact) {
  const auto start = std::chrono::steady_clock::now();
  IntersectionResult result;

  Mesh mesh;
  std::vector<Mesh::Vertex_index> vertices;
  vertices.reserve(artifact.vertices.size());
  for (const auto& vertex : artifact.vertices) {
    const auto& p = vertex.position_nanounits;
    vertices.push_back(mesh.add_vertex(
        Point(exact_coordinate(p[0]), exact_coordinate(p[1]), exact_coordinate(p[2]))));
  }

  std::vector<std::uint32_t> face_to_input;
  face_to_input.reserve(artifact.triangles.size());
  for (std::uint32_t input_index = 0;
       input_index < artifact.triangles.size(); ++input_index) {
    const auto& triangle = artifact.triangles[input_index];
    const Point& a = mesh.point(vertices[triangle[0]]);
    const Point& b = mesh.point(vertices[triangle[1]]);
    const Point& c = mesh.point(vertices[triangle[2]]);
    if (CGAL::collinear(a, b, c)) {
      result.reason = "DegenerateTriangle";
      result.offending_triangle = input_index;
      result.elapsed_nanoseconds = static_cast<std::uint64_t>(
          std::chrono::duration_cast<std::chrono::nanoseconds>(
              std::chrono::steady_clock::now() - start).count());
      return result;
    }

    const auto face = mesh.add_face(
        vertices[triangle[0]], vertices[triangle[1]], vertices[triangle[2]]);
    if (face == Mesh::null_face()) {
      result.reason = "SurfaceMeshConstructionRejected";
      result.offending_triangle = input_index;
      result.elapsed_nanoseconds = static_cast<std::uint64_t>(
          std::chrono::duration_cast<std::chrono::nanoseconds>(
              std::chrono::steady_clock::now() - start).count());
      return result;
    }
    if (face.idx() >= face_to_input.size()) {
      face_to_input.resize(face.idx() + 1, std::numeric_limits<std::uint32_t>::max());
    }
    face_to_input[face.idx()] = input_index;
  }

  result.surface_construction_ok = true;

  // Ask CGAL for both the fast predicate and the complete intersecting-face
  // pairs. The pair list is mapped back to original zero-based input indices.
  const bool predicate = PMP::does_self_intersect(mesh);
  using FacePair = std::pair<Mesh::Face_index, Mesh::Face_index>;
  std::vector<FacePair> cgal_pairs;
  PMP::self_intersections(mesh, std::back_inserter(cgal_pairs));

  result.intersecting_triangle_pairs.reserve(cgal_pairs.size());
  for (const auto& pair : cgal_pairs) {
    const auto first = pair.first.idx();
    const auto second = pair.second.idx();
    if (first >= face_to_input.size() || second >= face_to_input.size() ||
        face_to_input[first] == std::numeric_limits<std::uint32_t>::max() ||
        face_to_input[second] == std::numeric_limits<std::uint32_t>::max()) {
      result.surface_construction_ok = false;
      result.reason = "FaceMappingFailure";
      result.intersecting_triangle_pairs.clear();
      result.elapsed_nanoseconds = static_cast<std::uint64_t>(
          std::chrono::duration_cast<std::chrono::nanoseconds>(
              std::chrono::steady_clock::now() - start).count());
      return result;
    }
    auto a = face_to_input[first];
    auto b = face_to_input[second];
    if (b < a) std::swap(a, b);
    result.intersecting_triangle_pairs.emplace_back(a, b);
  }
  std::sort(result.intersecting_triangle_pairs.begin(),
            result.intersecting_triangle_pairs.end());
  result.intersecting_triangle_pairs.erase(
      std::unique(result.intersecting_triangle_pairs.begin(),
                  result.intersecting_triangle_pairs.end()),
      result.intersecting_triangle_pairs.end());

  result.has_self_intersections = predicate;
  if (predicate != !result.intersecting_triangle_pairs.empty()) {
    result.surface_construction_ok = false;
    result.reason = "PredicatePairEnumerationMismatch";
    result.intersecting_triangle_pairs.clear();
    result.has_self_intersections = false;
    result.status = "NotEvaluated";
  } else if (predicate) {
    result.status = "Fail";
    result.reason = "SelfIntersectionsDetected";
  } else {
    result.status = "Pass";
    result.reason = "NoSelfIntersectionsDetected";
  }

  result.elapsed_nanoseconds = static_cast<std::uint64_t>(
      std::chrono::duration_cast<std::chrono::nanoseconds>(
          std::chrono::steady_clock::now() - start).count());
  return result;
}

}  // namespace ourodc
