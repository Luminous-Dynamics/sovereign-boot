#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace ourodc {

inline constexpr std::array<std::uint8_t, 8> kMagic{
    'O', 'U', 'R', 'O', 'D', 'C', 'Q', '1'};
inline constexpr std::uint16_t kSchemaVersion = 1;
inline constexpr std::uint64_t kMaxArtifactBytes = 32ULL * 1024ULL * 1024ULL;
inline constexpr std::uint32_t kMaxVertices = 50'000;
inline constexpr std::uint32_t kMaxQuads = 62'500;
inline constexpr std::uint32_t kMaxTriangles = 125'000;
inline constexpr std::uint32_t kMaxActiveCells = 2'097'152;
inline constexpr std::int64_t kPositionScale = 1'000'000'000LL;
inline constexpr std::int32_t kNormalScale = 1'000'000'000;

struct Vertex {
  std::array<std::int64_t, 3> position_nanounits{};
  std::array<std::int32_t, 3> normal_nanounits{};
  std::uint32_t feature_mask{};
};

struct Metrics {
  std::array<std::uint32_t, 3> grid_dimensions{};
  std::uint32_t active_cell_count{};
  std::uint32_t sign_changing_edge_count{};
  std::uint32_t qef_centroid_fallback_count{};
  std::uint32_t qef_clamped_vertex_count{};
  std::uint32_t gradient_fallback_count{};
  std::uint64_t qef_residual_p50_nanounits{};
  std::uint64_t qef_residual_p95_nanounits{};
  std::uint64_t qef_residual_max_nanounits{};
  std::uint64_t geometry_bytes{};
  std::uint64_t working_payload_estimate_bytes{};
};

struct Artifact {
  std::vector<std::uint8_t> source_grid_bytes;
  std::vector<Vertex> vertices;
  std::vector<std::array<std::uint32_t, 4>> quads;
  std::vector<std::array<std::uint32_t, 3>> triangles;
  Metrics metrics;
  std::string input_sha256;
  std::uint64_t input_byte_length{};
};

struct ParseError {
  std::string code;
  std::string detail;
};

Artifact parse_artifact(std::span<const std::uint8_t> bytes);
std::string sha256_hex(std::span<const std::uint8_t> bytes);

}  // namespace ourodc
