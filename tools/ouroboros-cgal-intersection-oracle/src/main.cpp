#include "ourodc/artifact.hpp"
#include "ourodc/cgal_oracle.hpp"

#include <CGAL/version.h>

#include <chrono>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <iterator>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

std::string json_escape(const std::string& input) {
  std::string output;
  output.reserve(input.size());
  for (const unsigned char c : input) {
    switch (c) {
      case '"': output += "\\\""; break;
      case '\\': output += "\\\\"; break;
      case '\n': output += "\\n"; break;
      case '\r': output += "\\r"; break;
      case '\t': output += "\\t"; break;
      default:
        if (c < 0x20U) {
          constexpr char hex[] = "0123456789abcdef";
          output += "\\u00";
          output.push_back(hex[c >> 4U]);
          output.push_back(hex[c & 0x0fU]);
        } else {
          output.push_back(static_cast<char>(c));
        }
    }
  }
  return output;
}

std::vector<std::uint8_t> read_file(const std::string& path) {
  std::ifstream input(path, std::ios::binary | std::ios::ate);
  if (!input) throw std::runtime_error("InputOpenFailed");
  const auto end = input.tellg();
  if (end < 0) throw std::runtime_error("InputSizeFailed");
  const auto size = static_cast<std::uint64_t>(end);
  if (size == 0 || size > ourodc::kMaxArtifactBytes) {
    throw std::runtime_error("ArtifactLimitExceeded");
  }
  std::vector<std::uint8_t> bytes(static_cast<std::size_t>(size));
  input.seekg(0, std::ios::beg);
  input.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
  if (!input) throw std::runtime_error("InputReadFailed");
  return bytes;
}

void print_not_evaluated(const std::string& reason) {
  std::cout << "{"
            << "\"schema\":\"OUROCGI1\","
            << "\"parser\":\"NotEvaluated\","
            << "\"surface_construction\":\"NotEvaluated\","
            << "\"self_intersection_status\":\"NotEvaluated\","
            << "\"reason\":\"" << json_escape(reason) << "\""
            << "}\n";
}

void print_oracle_not_evaluated(
    const ourodc::Artifact& artifact,
    const std::string& reason) {
  std::cout << "{"
            << "\"schema\":\"OUROCGI1\","
            << "\"input_sha256\":\"" << artifact.input_sha256 << "\","
            << "\"input_byte_length\":" << artifact.input_byte_length << ","
            << "\"cgal_version\":\"" << CGAL_VERSION_STR << "\","
            << "\"kernel\":\"Exact_predicates_exact_constructions_kernel\","
            << "\"parser\":\"Pass\","
            << "\"surface_construction\":\"NotEvaluated\","
            << "\"self_intersection_status\":\"NotEvaluated\","
            << "\"reason\":\"" << json_escape(reason) << "\","
            << "\"pair_count\":0,\"pairs\":[]"
            << "}\n";
}

}  // namespace

int main(int argc, char** argv) {
  if (argc != 2) {
    std::cerr << "usage: ouroboros-cgal-intersection-oracle <artifact.ourodc>\n";
    return 64;
  }

  try {
    const auto bytes = read_file(argv[1]);
    const auto artifact = ourodc::parse_artifact(bytes);
    ourodc::IntersectionResult result;
    try {
      result = ourodc::inspect_self_intersections(artifact);
    } catch (const std::exception& error) {
      print_oracle_not_evaluated(artifact, error.what());
      return 4;
    }

    std::cout << "{"
              << "\"schema\":\"OUROCGI1\","
              << "\"input_sha256\":\"" << artifact.input_sha256 << "\","
              << "\"input_byte_length\":" << artifact.input_byte_length << ","
              << "\"cgal_version\":\"" << CGAL_VERSION_STR << "\","
              << "\"kernel\":\"Exact_predicates_exact_constructions_kernel\","
              << "\"compiler\":\"" << json_escape(__VERSION__) << "\","
#ifdef NDEBUG
              << "\"build_mode\":\"release\","
#else
              << "\"build_mode\":\"debug\","
#endif
              << "\"parser\":\"Pass\","
              << "\"vertex_count\":" << artifact.vertices.size() << ","
              << "\"triangle_count\":" << artifact.triangles.size() << ","
              << "\"surface_construction\":\""
              << (result.surface_construction_ok ? "Pass" : "NotEvaluated") << "\","
              << "\"self_intersection_status\":\"" << result.status << "\","
              << "\"reason\":\"" << json_escape(result.reason) << "\","
              << "\"offending_triangle\":"
              << ((result.reason == "DegenerateTriangle" ||
                   result.reason == "SurfaceMeshConstructionRejected")
                      ? std::to_string(result.offending_triangle)
                      : "null")
              << ","
              << "\"pair_count\":" << result.intersecting_triangle_pairs.size() << ","
              << "\"pairs\":[";
    for (std::size_t i = 0; i < result.intersecting_triangle_pairs.size(); ++i) {
      if (i != 0) std::cout << ",";
      std::cout << "[" << result.intersecting_triangle_pairs[i].first
                << "," << result.intersecting_triangle_pairs[i].second << "]";
    }
    std::cout << "],"
              << "\"elapsed_nanoseconds_local_observation\":"
              << result.elapsed_nanoseconds
              << "}\n";

    if (result.status == "Pass") return 0;
    if (result.status == "Fail") return 3;
    return 4;
  } catch (const std::exception& error) {
    print_not_evaluated(error.what());
    return 2;
  }
}
