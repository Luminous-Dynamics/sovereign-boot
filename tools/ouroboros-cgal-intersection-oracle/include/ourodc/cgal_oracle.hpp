#pragma once

#include "ourodc/artifact.hpp"

#include <cstdint>
#include <string>
#include <utility>
#include <vector>

namespace ourodc {

struct IntersectionResult {
  bool surface_construction_ok{};
  bool has_self_intersections{};
  std::string status{"NotEvaluated"};
  std::string reason;
  std::uint32_t offending_triangle{};
  std::vector<std::pair<std::uint32_t, std::uint32_t>> intersecting_triangle_pairs;
  std::uint64_t elapsed_nanoseconds{};
};

IntersectionResult inspect_self_intersections(const Artifact& artifact);

}  // namespace ourodc
