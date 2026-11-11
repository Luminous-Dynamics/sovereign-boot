#include "ourodc/artifact.hpp"

#include <openssl/evp.h>

#include <algorithm>
#include <array>
#include <bit>
#include <limits>
#include <stdexcept>
#include <type_traits>

namespace ourodc {
namespace {

constexpr std::size_t kHeaderBytes = 8 + 2 + 4;
constexpr std::size_t kConfigBytes = 37;
constexpr std::size_t kVertexBytes = 40;
constexpr std::size_t kQuadBytes = 16;
constexpr std::size_t kTriangleBytes = 12;
constexpr std::size_t kMetricsBytes = 3 * 4 + 5 * 4 + 5 * 8;
constexpr std::uint64_t kMaxWorkingBytes = 128ULL * 1024ULL * 1024ULL;

void require(bool condition, const char* code, const char* detail) {
  if (!condition) {
    throw std::runtime_error(std::string(code) + ": " + detail);
  }
}

void require_available(std::span<const std::uint8_t> bytes,
                       std::size_t offset,
                       std::size_t count) {
  require(offset <= bytes.size() && count <= bytes.size() - offset,
          "TruncatedArtifact", "a required section extends beyond end of file");
}

template <typename T>
T read_le(std::span<const std::uint8_t> bytes, std::size_t& offset) {
  static_assert(std::is_integral_v<T>);
  using U = std::make_unsigned_t<T>;
  require_available(bytes, offset, sizeof(T));
  U value = 0;
  for (std::size_t i = 0; i < sizeof(T); ++i) {
    value |= static_cast<U>(bytes[offset + i]) << (i * 8U);
  }
  offset += sizeof(T);
  if constexpr (std::is_signed_v<T>) {
    return std::bit_cast<T>(value);
  } else {
    return value;
  }
}

void skip(std::span<const std::uint8_t> bytes,
          std::size_t& offset,
          std::size_t count) {
  require_available(bytes, offset, count);
  offset += count;
}

std::size_t checked_bytes(std::uint32_t count, std::size_t stride) {
  const auto count64 = static_cast<std::uint64_t>(count);
  require(count64 <= std::numeric_limits<std::size_t>::max() / stride,
          "CountOverflow", "count multiplied by record size overflows size_t");
  return static_cast<std::size_t>(count64) * stride;
}

bool all_distinct(const std::array<std::uint32_t, 4>& q) {
  return q[0] != q[1] && q[0] != q[2] && q[0] != q[3] &&
         q[1] != q[2] && q[1] != q[3] && q[2] != q[3];
}

bool all_distinct(const std::array<std::uint32_t, 3>& t) {
  return t[0] != t[1] && t[0] != t[2] && t[1] != t[2];
}

}  // namespace

std::string sha256_hex(std::span<const std::uint8_t> bytes) {
  std::array<unsigned char, EVP_MAX_MD_SIZE> digest{};
  unsigned int digest_length = 0;
  const auto* data = bytes.empty() ? nullptr : bytes.data();
  require(EVP_Digest(data, bytes.size(), digest.data(), &digest_length,
                     EVP_sha256(), nullptr) == 1,
          "DigestFailure", "OpenSSL EVP_Digest failed");
  require(digest_length == 32, "DigestFailure", "unexpected SHA-256 length");
  static constexpr char hex[] = "0123456789abcdef";
  std::string result;
  result.reserve(64);
  for (unsigned int i = 0; i < digest_length; ++i) {
    result.push_back(hex[digest[i] >> 4U]);
    result.push_back(hex[digest[i] & 0x0fU]);
  }
  return result;
}

Artifact parse_artifact(std::span<const std::uint8_t> bytes) {
  require(bytes.size() >= kHeaderBytes, "TruncatedArtifact", "header is incomplete");
  require(bytes.size() <= kMaxArtifactBytes, "ArtifactLimitExceeded",
          "artifact exceeds the 32 MiB parser ceiling");
  require(std::equal(kMagic.begin(), kMagic.end(), bytes.begin()),
          "InvalidMagic", "expected OURODCQ1");
  std::size_t offset = 8;
  const auto schema = read_le<std::uint16_t>(bytes, offset);
  require(schema == kSchemaVersion, "UnsupportedSchema", "expected schema version 1");
  const auto source_size = read_le<std::uint32_t>(bytes, offset);
  require(source_size > 0, "InvalidSourceGrid", "source-grid section must be non-empty");
  require_available(bytes, offset, source_size);

  Artifact artifact;
  artifact.source_grid_bytes.assign(bytes.begin() + static_cast<std::ptrdiff_t>(offset),
                                    bytes.begin() + static_cast<std::ptrdiff_t>(offset + source_size));
  offset += source_size;

  skip(bytes, offset, kConfigBytes);

  const auto vertex_count = read_le<std::uint32_t>(bytes, offset);
  require(vertex_count > 0 && vertex_count <= kMaxVertices,
          "InvalidVertexCount", "vertex count is outside the canonical limits");
  skip(bytes, offset, checked_bytes(vertex_count, kVertexBytes));
  const auto vertex_start = offset - checked_bytes(vertex_count, kVertexBytes);
  artifact.vertices.reserve(vertex_count);
  std::size_t vertex_offset = vertex_start;
  for (std::uint32_t i = 0; i < vertex_count; ++i) {
    Vertex vertex;
    for (auto& value : vertex.position_nanounits) {
      value = read_le<std::int64_t>(bytes, vertex_offset);
    }
    for (auto& value : vertex.normal_nanounits) {
      value = read_le<std::int32_t>(bytes, vertex_offset);
      require(value >= -kNormalScale && value <= kNormalScale,
              "InvalidNormal", "normal component exceeds fixed-point unit bounds");
    }
    vertex.feature_mask = read_le<std::uint32_t>(bytes, vertex_offset);
    require(vertex.feature_mask != 0, "InvalidFeatureMask", "vertex feature mask is zero");
    artifact.vertices.push_back(vertex);
  }

  const auto quad_count = read_le<std::uint32_t>(bytes, offset);
  require(quad_count > 0 && quad_count <= kMaxQuads,
          "InvalidQuadCount", "quad count is outside the canonical limits");
  require_available(bytes, offset, checked_bytes(quad_count, kQuadBytes));
  artifact.quads.reserve(quad_count);
  for (std::uint32_t i = 0; i < quad_count; ++i) {
    std::array<std::uint32_t, 4> quad{};
    for (auto& index : quad) {
      index = read_le<std::uint32_t>(bytes, offset);
      require(index < vertex_count, "FaceIndexOutOfBounds", "quad index exceeds vertex count");
    }
    require(all_distinct(quad), "DegenerateQuad", "quad repeats a vertex index");
    artifact.quads.push_back(quad);
  }

  const auto triangle_count = read_le<std::uint32_t>(bytes, offset);
  require(triangle_count == quad_count * 2U && triangle_count <= kMaxTriangles,
          "InvalidTriangleCount", "triangle view must contain exactly two triangles per quad");
  require_available(bytes, offset, checked_bytes(triangle_count, kTriangleBytes));
  artifact.triangles.reserve(triangle_count);
  for (std::uint32_t i = 0; i < triangle_count; ++i) {
    std::array<std::uint32_t, 3> triangle{};
    for (auto& index : triangle) {
      index = read_le<std::uint32_t>(bytes, offset);
      require(index < vertex_count, "FaceIndexOutOfBounds",
              "triangle index exceeds vertex count");
    }
    require(all_distinct(triangle), "DegenerateTriangle",
            "triangle repeats a vertex index");
    artifact.triangles.push_back(triangle);
  }

  for (std::size_t i = 0; i < artifact.quads.size(); ++i) {
    const auto& q = artifact.quads[i];
    const auto& t0 = artifact.triangles[2 * i];
    const auto& t1 = artifact.triangles[2 * i + 1];
    require(t0 == std::array<std::uint32_t, 3>{q[0], q[1], q[2]} &&
                t1 == std::array<std::uint32_t, 3>{q[0], q[2], q[3]},
            "TriangleViewMismatch", "triangle pair does not match fixed quad diagonal (0,2)");
  }

  for (auto& dimension : artifact.metrics.grid_dimensions) {
    dimension = read_le<std::uint32_t>(bytes, offset);
    require(dimension >= 3, "InvalidGridDimensions", "grid dimensions must be at least 3");
  }
  artifact.metrics.active_cell_count = read_le<std::uint32_t>(bytes, offset);
  artifact.metrics.sign_changing_edge_count = read_le<std::uint32_t>(bytes, offset);
  artifact.metrics.qef_centroid_fallback_count = read_le<std::uint32_t>(bytes, offset);
  artifact.metrics.qef_clamped_vertex_count = read_le<std::uint32_t>(bytes, offset);
  artifact.metrics.gradient_fallback_count = read_le<std::uint32_t>(bytes, offset);
  artifact.metrics.qef_residual_p50_nanounits = read_le<std::uint64_t>(bytes, offset);
  artifact.metrics.qef_residual_p95_nanounits = read_le<std::uint64_t>(bytes, offset);
  artifact.metrics.qef_residual_max_nanounits = read_le<std::uint64_t>(bytes, offset);
  artifact.metrics.geometry_bytes = read_le<std::uint64_t>(bytes, offset);
  artifact.metrics.working_payload_estimate_bytes = read_le<std::uint64_t>(bytes, offset);

  require(artifact.metrics.active_cell_count <= kMaxActiveCells,
          "InvalidMetrics", "active cell count exceeds extractor ceiling");
  require(artifact.metrics.sign_changing_edge_count == quad_count,
          "InvalidMetrics", "quad count and sign-changing edge count differ");
  require(artifact.metrics.qef_centroid_fallback_count <= vertex_count &&
              artifact.metrics.qef_clamped_vertex_count <= vertex_count &&
              artifact.metrics.gradient_fallback_count <= 12U * vertex_count,
          "InvalidMetrics", "fallback counters exceed possible output counts");
  const auto expected_geometry_bytes =
      static_cast<std::uint64_t>(vertex_count) * 40ULL +
      static_cast<std::uint64_t>(quad_count) * 40ULL;
  require(artifact.metrics.geometry_bytes == expected_geometry_bytes,
          "InvalidMetrics", "geometry byte count does not match canonical buffers");
  require(artifact.metrics.working_payload_estimate_bytes <= kMaxWorkingBytes,
          "InvalidMetrics", "working-payload estimate exceeds extractor ceiling");

  const auto audit_size = read_le<std::uint32_t>(bytes, offset);
  require(audit_size > 0, "InvalidAuditReceipt", "embedded audit receipt is empty");
  require_available(bytes, offset, audit_size);
  offset += audit_size;
  require(offset == bytes.size(), "TrailingBytes", "artifact has trailing bytes");

  artifact.input_sha256 = sha256_hex(bytes);
  artifact.input_byte_length = bytes.size();
  return artifact;
}

}  // namespace ourodc
