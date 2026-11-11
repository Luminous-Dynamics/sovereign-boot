#include "ourodc/artifact.hpp"

#include <array>
#include <cstdint>
#include <exception>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

void append_u16(std::vector<std::uint8_t>& out, std::uint16_t value) {
  out.push_back(static_cast<std::uint8_t>(value));
  out.push_back(static_cast<std::uint8_t>(value >> 8U));
}
void append_u32(std::vector<std::uint8_t>& out, std::uint32_t value) {
  for (unsigned i = 0; i < 4; ++i) out.push_back(static_cast<std::uint8_t>(value >> (8U * i)));
}
void append_u64(std::vector<std::uint8_t>& out, std::uint64_t value) {
  for (unsigned i = 0; i < 8; ++i) out.push_back(static_cast<std::uint8_t>(value >> (8U * i)));
}
void append_i64(std::vector<std::uint8_t>& out, std::int64_t value) {
  append_u64(out, static_cast<std::uint64_t>(value));
}
void append_i32(std::vector<std::uint8_t>& out, std::int32_t value) {
  append_u32(out, static_cast<std::uint32_t>(value));
}

std::vector<std::uint8_t> valid_artifact() {
  std::vector<std::uint8_t> bytes{'O', 'U', 'R', 'O', 'D', 'C', 'Q', '1'};
  append_u16(bytes, 1);
  std::vector<std::uint8_t> source{'O', 'U', 'R', 'O', 'G', 'R', 'D', '1'};
  append_u16(source, 1);
  append_u16(source, 1);
  source.push_back('f');
  append_u32(source, 1);
  source.push_back(0);
  append_u32(source, 3); append_u32(source, 3); append_u32(source, 3);
  source.insert(source.end(), {1, 2, 3, 1});
  for (int i = 0; i < 3; ++i) append_i64(source, -1'000'000);
  for (int i = 0; i < 3; ++i) append_i64(source, 1'000'000);
  append_i64(source, 1'000'000);
  append_u64(source, 27);
  for (int i = 0; i < 27; ++i) append_i32(source, 1);
  append_u32(bytes, static_cast<std::uint32_t>(source.size()));
  bytes.insert(bytes.end(), source.begin(), source.end());

  append_u32(bytes, 50'000);
  append_u32(bytes, 62'500);
  append_u64(bytes, 8ULL * 1024ULL * 1024ULL);
  append_u64(bytes, 128ULL * 1024ULL * 1024ULL);
  append_u32(bytes, 100); append_u32(bytes, 1);
  bytes.insert(bytes.end(), {1, 1, 1, 1, 1});

  append_u32(bytes, 4);                  // vertices
  const std::array<std::array<std::int64_t, 3>, 4> positions{{
      {{0, 0, 0}}, {{1'000'000'000, 0, 0}},
      {{1'000'000'000, 1'000'000'000, 0}}, {{0, 1'000'000'000, 0}}
  }};
  for (const auto& position : positions) {
    for (const auto value : position) append_i64(bytes, value);
    append_i32(bytes, 0); append_i32(bytes, 0); append_i32(bytes, 1'000'000'000);
    append_u32(bytes, 9);
  }

  append_u32(bytes, 1);                  // one quad
  append_u32(bytes, 0); append_u32(bytes, 1);
  append_u32(bytes, 2); append_u32(bytes, 3);

  append_u32(bytes, 2);                  // fixed diagonal (0,2)
  append_u32(bytes, 0); append_u32(bytes, 1); append_u32(bytes, 2);
  append_u32(bytes, 0); append_u32(bytes, 2); append_u32(bytes, 3);

  append_u32(bytes, 3); append_u32(bytes, 3); append_u32(bytes, 3);
  append_u32(bytes, 1); append_u32(bytes, 1);
  append_u32(bytes, 0); append_u32(bytes, 0); append_u32(bytes, 0);
  for (int i = 0; i < 3; ++i) append_u64(bytes, 0);
  append_u64(bytes, 200);
  append_u64(bytes, 1'000);
  append_u32(bytes, 1); bytes.push_back(0x7f); // opaque audit receipt
  return bytes;
}

void expect_error(std::vector<std::uint8_t> bytes, const std::string& code) {
  try {
    (void)ourodc::parse_artifact(bytes);
  } catch (const std::runtime_error& error) {
    const std::string message = error.what();
    if (message.find(code + ":") == std::string::npos) {
      throw std::runtime_error("expected " + code + ", received " + message);
    }
    return;
  }
  throw std::runtime_error("expected parse failure " + code);
}

void run() {
  const auto bytes = valid_artifact();
  const auto parsed = ourodc::parse_artifact(bytes);
  if (parsed.vertices.size() != 4 || parsed.quads.size() != 1 ||
      parsed.triangles.size() != 2 || parsed.metrics.grid_dimensions !=
          std::array<std::uint32_t, 3>{3, 3, 3}) {
    throw std::runtime_error("valid artifact decoded to unexpected counts");
  }
  if (parsed.input_byte_length != bytes.size() || parsed.input_sha256.size() != 64) {
    throw std::runtime_error("input digest/length missing");
  }
  if (ourodc::sha256_hex(bytes) != parsed.input_sha256) {
    throw std::runtime_error("SHA-256 is not stable");
  }

  auto bad_magic = bytes; bad_magic[0] = 'X';
  expect_error(std::move(bad_magic), "InvalidMagic");

  auto truncated = bytes; truncated.pop_back();
  expect_error(std::move(truncated), "TruncatedArtifact");

  auto bad_triangle = bytes;
  // The first triangle's second index is at the fixed offset after header,
  // source, config, vertices and quad section.
  const auto source_size = static_cast<std::size_t>(
      static_cast<std::uint32_t>(bytes[10]) |
      (static_cast<std::uint32_t>(bytes[11]) << 8U) |
      (static_cast<std::uint32_t>(bytes[12]) << 16U) |
      (static_cast<std::uint32_t>(bytes[13]) << 24U));
  const std::size_t triangle_section = 14 + source_size + 37 + 4 + 4 * 40 + 4 + 16 + 4;
  bad_triangle[triangle_section + 4] = 99;
  expect_error(std::move(bad_triangle), "FaceIndexOutOfBounds");

  auto bad_diagonal = bytes;
  bad_diagonal[triangle_section + 4] = 3;
  expect_error(std::move(bad_diagonal), "TriangleViewMismatch");

  auto trailing = bytes; trailing.push_back(0);
  expect_error(std::move(trailing), "TrailingBytes");

  auto bad_metrics = bytes;
  const std::size_t metrics_section = triangle_section + 4 + 2 * 12;
  // sign-changing edge count follows the three grid dimensions and active cell count.
  bad_metrics[metrics_section + 12 + 4] = 2;
  expect_error(std::move(bad_metrics), "InvalidMetrics");
}

}  // namespace

int main() {
  try {
    run();
    std::cout << "ourodc artifact parser tests: PASS\n";
    return 0;
  } catch (const std::exception& error) {
    std::cerr << "ourodc artifact parser tests: FAIL: " << error.what() << '\n';
    return 1;
  }
}
