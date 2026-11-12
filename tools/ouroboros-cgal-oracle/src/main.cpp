#include <CGAL/Exact_predicates_exact_constructions_kernel.h>
#include <CGAL/Gmpq.h>
#include <CGAL/Gmpz.h>
#include <CGAL/Polygon_mesh_processing/self_intersections.h>
#include <CGAL/version.h>

#include <algorithm>
#include <array>
#include <bit>
#include <cstdint>
#include <exception>
#include <fstream>
#include <iostream>
#include <iterator>
#include <limits>
#include <map>
#include <optional>
#include <sstream>
#include <stdexcept>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

namespace PMP = CGAL::Polygon_mesh_processing;
using Kernel = CGAL::Exact_predicates_exact_constructions_kernel;
using Point = Kernel::Point_3;
using Triangle = std::array<std::size_t, 3>;
using FacePair = std::pair<std::size_t, std::size_t>;

constexpr std::array<std::uint8_t, 8> kMagic{'O','U','R','O','T','S','Q','1'};
constexpr std::uint16_t kSchemaVersion = 1;
constexpr std::uint64_t kCoordinateScale = 1'000'000'000ULL;
constexpr std::uint64_t kMaxInputBytes = 32ULL * 1024ULL * 1024ULL;
constexpr std::uint32_t kMaxVertices = 50'000;
constexpr std::uint32_t kMaxTriangles = 125'000;
constexpr std::uint32_t kDefaultMaxPairs = 10'000;

struct Input {
  std::vector<Point> points;
  std::vector<Triangle> triangles;
};

struct Result {
  std::size_t input_triangle_count{};
  std::vector<std::size_t> degenerate_face_ids;
  std::vector<FacePair> intersecting_face_pairs;
  bool pair_list_truncated{};
  std::uint32_t pair_limit{};

  [[nodiscard]] bool has_intersections() const {
    return !intersecting_face_pairs.empty() || pair_list_truncated;
  }
};

template <typename T>
T read_little_endian(const std::vector<std::uint8_t>& bytes, std::size_t& offset) {
  static_assert(std::is_integral_v<T>);
  using U = std::make_unsigned_t<T>;
  if (offset > bytes.size() || bytes.size() - offset < sizeof(T)) {
    throw std::runtime_error("truncated OUROTSQ1 input");
  }
  U value = 0;
  for (std::size_t i = 0; i < sizeof(T); ++i) {
    value |= static_cast<U>(bytes[offset + i]) << (8U * static_cast<unsigned>(i));
  }
  offset += sizeof(T);
  if constexpr (std::is_signed_v<T>) {
    return std::bit_cast<T>(value);
  } else {
    return static_cast<T>(value);
  }
}

std::uint64_t checked_add(std::uint64_t a, std::uint64_t b) {
  if (b > std::numeric_limits<std::uint64_t>::max() - a) {
    throw std::runtime_error("OUROTSQ1 size overflow");
  }
  return a + b;
}

std::uint64_t checked_mul(std::uint64_t a, std::uint64_t b) {
  if (a != 0 && b > std::numeric_limits<std::uint64_t>::max() / a) {
    throw std::runtime_error("OUROTSQ1 size overflow");
  }
  return a * b;
}

Kernel::FT exact_coordinate(std::int64_t nanounits) {
  // The target is Linux/NixOS (LP64). Use CGAL's exact GMP integers, not a
  // double conversion, so every accepted signed 64-bit coordinate is exact.
  static_assert(sizeof(long) >= sizeof(std::int64_t),
                "Exact int64-to-GMP conversion requires an LP64 target");
  const CGAL::Gmpz numerator(static_cast<long>(nanounits));
  const CGAL::Gmpz denominator(static_cast<long>(kCoordinateScale));
  return Kernel::FT(CGAL::Gmpq(numerator, denominator));
}

Input parse_input(const std::vector<std::uint8_t>& bytes) {
  if (bytes.size() > kMaxInputBytes) {
    throw std::runtime_error("OUROTSQ1 input exceeds 32 MiB limit");
  }
  if (bytes.size() < 18) {
    throw std::runtime_error("OUROTSQ1 header is truncated");
  }
  if (!std::equal(kMagic.begin(), kMagic.end(), bytes.begin())) {
    throw std::runtime_error("unsupported triangle-soup magic");
  }

  std::size_t offset = kMagic.size();
  const auto schema = read_little_endian<std::uint16_t>(bytes, offset);
  if (schema != kSchemaVersion) {
    throw std::runtime_error("unsupported OUROTSQ1 schema version");
  }
  const auto vertex_count = read_little_endian<std::uint32_t>(bytes, offset);
  const auto triangle_count = read_little_endian<std::uint32_t>(bytes, offset);
  if (vertex_count < 3 || vertex_count > kMaxVertices) {
    throw std::runtime_error("vertex count outside oracle limits");
  }
  if (triangle_count == 0 || triangle_count > kMaxTriangles) {
    throw std::runtime_error("triangle count outside oracle limits");
  }

  std::uint64_t expected = 18;
  expected = checked_add(expected, checked_mul(vertex_count, 24));
  expected = checked_add(expected, checked_mul(triangle_count, 12));
  if (expected != bytes.size()) {
    throw std::runtime_error("OUROTSQ1 byte length does not match its counts");
  }

  Input input;
  input.points.reserve(vertex_count);
  for (std::uint32_t i = 0; i < vertex_count; ++i) {
    const auto x = read_little_endian<std::int64_t>(bytes, offset);
    const auto y = read_little_endian<std::int64_t>(bytes, offset);
    const auto z = read_little_endian<std::int64_t>(bytes, offset);
    input.points.emplace_back(exact_coordinate(x), exact_coordinate(y), exact_coordinate(z));
  }

  input.triangles.reserve(triangle_count);
  for (std::uint32_t i = 0; i < triangle_count; ++i) {
    Triangle face{};
    for (std::size_t corner = 0; corner < 3; ++corner) {
      const auto index = read_little_endian<std::uint32_t>(bytes, offset);
      if (index >= vertex_count) {
        throw std::runtime_error("triangle index out of range");
      }
      face[corner] = static_cast<std::size_t>(index);
    }
    input.triangles.push_back(face);
  }
  if (offset != bytes.size()) {
    throw std::runtime_error("OUROTSQ1 parser did not consume exact input length");
  }
  return input;
}

Result inspect(const Input& input, std::uint32_t max_pairs) {
  if (max_pairs == 0 || max_pairs > 100'000) {
    throw std::runtime_error("max_pairs must be in [1, 100000]");
  }

  Result result;
  result.input_triangle_count = input.triangles.size();
  result.pair_limit = max_pairs;

  std::vector<Triangle> nondegenerate;
  std::vector<std::size_t> original_face_ids;
  nondegenerate.reserve(input.triangles.size());
  original_face_ids.reserve(input.triangles.size());

  for (std::size_t face_id = 0; face_id < input.triangles.size(); ++face_id) {
    const auto& face = input.triangles[face_id];
    const Kernel::Triangle_3 triangle(
        input.points[face[0]], input.points[face[1]], input.points[face[2]]);
    if (triangle.is_degenerate()) {
      result.degenerate_face_ids.push_back(face_id);
    } else {
      nondegenerate.push_back(face);
      original_face_ids.push_back(face_id);
    }
  }

  std::vector<FacePair> raw_pairs;
  raw_pairs.reserve(std::min<std::size_t>(
      static_cast<std::size_t>(max_pairs) + 1U, 4096U));
  // Sequential execution is intentional: deterministic input/iteration order
  // and a strict output cap. The returned indices address the filtered array.
  PMP::triangle_soup_self_intersections<CGAL::Sequential_tag>(
      input.points,
      nondegenerate,
      std::back_inserter(raw_pairs),
      CGAL::parameters::maximum_number(static_cast<unsigned int>(max_pairs + 1U)));

  for (const auto& pair : raw_pairs) {
    if (pair.first >= original_face_ids.size() || pair.second >= original_face_ids.size()) {
      throw std::runtime_error("CGAL returned an out-of-range face ID");
    }
    auto a = original_face_ids[pair.first];
    auto b = original_face_ids[pair.second];
    if (a > b) std::swap(a, b);
    result.intersecting_face_pairs.emplace_back(a, b);
  }
  std::sort(result.intersecting_face_pairs.begin(), result.intersecting_face_pairs.end());
  result.intersecting_face_pairs.erase(
      std::unique(result.intersecting_face_pairs.begin(),
                  result.intersecting_face_pairs.end()),
      result.intersecting_face_pairs.end());

  if (result.intersecting_face_pairs.size() > max_pairs) {
    result.pair_list_truncated = true;
    result.intersecting_face_pairs.resize(max_pairs);
  }
  return result;
}

template <typename T>
void append_little_endian(std::vector<std::uint8_t>& bytes, T value) {
  static_assert(std::is_integral_v<T>);
  using U = std::make_unsigned_t<T>;
  const U raw = [&]() {
    if constexpr (std::is_signed_v<T>) return std::bit_cast<U>(value);
    else return static_cast<U>(value);
  }();
  for (std::size_t i = 0; i < sizeof(T); ++i) {
    bytes.push_back(static_cast<std::uint8_t>(
        (raw >> (8U * static_cast<unsigned>(i))) & static_cast<U>(0xffU)));
  }
}

std::vector<std::uint8_t> make_triangle_soup_fixture(
    const std::vector<std::array<std::int64_t, 3>>& points,
    const std::vector<Triangle>& triangles,
    std::uint16_t schema = kSchemaVersion) {
  std::vector<std::uint8_t> bytes;
  bytes.insert(bytes.end(), kMagic.begin(), kMagic.end());
  append_little_endian(bytes, schema);
  append_little_endian(bytes, static_cast<std::uint32_t>(points.size()));
  append_little_endian(bytes, static_cast<std::uint32_t>(triangles.size()));
  for (const auto& point : points) {
    for (const auto coordinate : point) append_little_endian(bytes, coordinate);
  }
  for (const auto& triangle : triangles) {
    for (const auto index : triangle) {
      append_little_endian(bytes, static_cast<std::uint32_t>(index));
    }
  }
  return bytes;
}

bool parser_rejects(const std::vector<std::uint8_t>& bytes) {
  try {
    (void)parse_input(bytes);
    return false;
  } catch (const std::exception&) {
    return true;
  }
}

std::vector<std::uint8_t> read_file(const std::string& path) {
  std::ifstream file(path, std::ios::binary | std::ios::ate);
  if (!file) {
    throw std::runtime_error("unable to open input file");
  }
  const auto end = file.tellg();
  if (end < 0 || static_cast<std::uint64_t>(end) > kMaxInputBytes) {
    throw std::runtime_error("input file size outside 32 MiB limit");
  }
  const auto size = static_cast<std::size_t>(end);
  std::vector<std::uint8_t> bytes(size);
  file.seekg(0, std::ios::beg);
  if (size != 0) {
    file.read(reinterpret_cast<char*>(bytes.data()), static_cast<std::streamsize>(size));
  }
  if (!file || static_cast<std::size_t>(file.gcount()) != size) {
    throw std::runtime_error("failed to read complete input file");
  }
  return bytes;
}

void print_ids(const std::vector<std::size_t>& ids) {
  std::cout << "[";
  for (std::size_t i = 0; i < ids.size(); ++i) {
    if (i != 0) std::cout << ",";
    std::cout << ids[i];
  }
  std::cout << "]";
}

void print_result(const Result& result) {
  const char* intersection_status =
      result.has_intersections() ? "Fail" : "Pass";
  const char* degenerate_status =
      result.degenerate_face_ids.empty() ? "Pass" : "Fail";
  std::cout << "{"
            << "\"schema\":\"OUROCGAL1\","
            << "\"cgal_version\":\"" << CGAL_VERSION_STR << "\","
            << "\"kernel\":\"Exact_predicates_exact_constructions_kernel\","
            << "\"input_schema\":\"OUROTSQ1\","
            << "\"coordinate_scale\":1000000000,"
            << "\"triangle_count\":" << result.input_triangle_count << ","
            << "\"degenerate_face_status\":\"" << degenerate_status << "\","
            << "\"degenerate_face_ids\":";
  print_ids(result.degenerate_face_ids);
  std::cout << ",\"self_intersection_status\":\"" << intersection_status << "\","
            << "\"pair_list_truncated\":"
            << (result.pair_list_truncated ? "true" : "false") << ","
            << "\"pair_limit\":" << result.pair_limit << ","
            << "\"reported_pair_count\":" << result.intersecting_face_pairs.size()
            << ",\"intersecting_face_pairs\":[";
  for (std::size_t i = 0; i < result.intersecting_face_pairs.size(); ++i) {
    if (i != 0) std::cout << ",";
    std::cout << "[" << result.intersecting_face_pairs[i].first
              << "," << result.intersecting_face_pairs[i].second << "]";
  }
  std::cout << "]}\n";
}

bool self_test() {
  const auto p = [](long x, long y, long z) {
    return Point(exact_coordinate(x * static_cast<std::int64_t>(kCoordinateScale)),
                 exact_coordinate(y * static_cast<std::int64_t>(kCoordinateScale)),
                 exact_coordinate(z * static_cast<std::int64_t>(kCoordinateScale)));
  };

  // 1. Separated triangles.
  Input separated{{p(0,0,0), p(1,0,0), p(0,1,0),
                   p(4,0,0), p(5,0,0), p(4,1,0)},
                  {{{0,1,2}}, {{3,4,5}}}};
  if (inspect(separated, 10).has_intersections()) return false;

  // 2. Proper interior crossing.
  Input crossing{{p(0,0,0), p(2,0,0), p(0,2,0),
                  p(1,-1,-1), p(1,2,1), p(1,2,-1)},
                 {{{0,1,2}}, {{3,4,5}}}};
  if (!inspect(crossing, 10).has_intersections()) return false;

  // 3. Two triangles sharing the same indexed edge are not a soup crossing.
  Input shared_edge{{p(0,0,0), p(1,0,0), p(1,1,0), p(0,1,0)},
                    {{{0,1,2}}, {{0,2,3}}}};
  if (inspect(shared_edge, 10).has_intersections()) return false;

  // 4. Coincident triangles using duplicated point IDs must be detected.
  Input duplicate_ids{{p(0,0,0), p(2,0,0), p(0,2,0),
                       p(0,0,0), p(2,0,0), p(0,2,0)},
                      {{{0,1,2}}, {{3,4,5}}}};
  if (!inspect(duplicate_ids, 10).has_intersections()) return false;

  // 5. Degenerate faces are classified separately, not sent to the pair query.
  Input degenerate{{p(0,0,0), p(1,0,0), p(0,1,0)},
                   {{{0,1,1}}}};
  const auto degenerate_result = inspect(degenerate, 10);
  if (degenerate_result.degenerate_face_ids != std::vector<std::size_t>{0}
      || degenerate_result.has_intersections()) return false;

  // 6. Capping is explicit. Three coincident copies have three pairwise hits.
  Input capped{{p(0,0,0), p(2,0,0), p(0,2,0),
                p(0,0,0), p(2,0,0), p(0,2,0),
                p(0,0,0), p(2,0,0), p(0,2,0)},
               {{{0,1,2}}, {{3,4,5}}, {{6,7,8}}}};
  const auto capped_result = inspect(capped, 1);
  if (!capped_result.pair_list_truncated
      || capped_result.intersecting_face_pairs.size() != 1
      || !capped_result.has_intersections()) return false;

  // 7. The binary contract round-trips exact integer nanounits and topology.
  const auto wire = make_triangle_soup_fixture(
      {{0,0,0}, {1'000'000'000,0,0}, {0,1'000'000'000,0}},
      {{{0,1,2}}});
  const auto parsed = parse_input(wire);
  if (parsed.points.size() != 3 || parsed.triangles.size() != 1
      || parsed.triangles[0] != Triangle{0,1,2}) return false;
  if (Kernel::FT(parsed.points[1].x()) != Kernel::FT(1)) return false;

  // 8. Malformed length, unknown schema, and invalid indices reject.
  auto truncated = wire;
  truncated.pop_back();
  if (!parser_rejects(truncated)) return false;
  auto unknown_schema = make_triangle_soup_fixture(
      {{0,0,0}, {1,0,0}, {0,1,0}}, {{{0,1,2}}}, 99);
  if (!parser_rejects(unknown_schema)) return false;
  auto invalid_index = make_triangle_soup_fixture(
      {{0,0,0}, {1,0,0}, {0,1,0}}, {{{0,1,3}}});
  if (!parser_rejects(invalid_index)) return false;

  return true;
}

int main(int argc, char** argv) {
  try {
    if (argc == 2 && std::string(argv[1]) == "--self-test") {
      const bool passed = self_test();
      std::cout << "{\"oracle\":\"OUROCGAL1\",\"self_test\":"
                << (passed ? "\"PASS\"" : "\"FAIL\"")
                << ",\"cgal_version\":\"" << CGAL_VERSION_STR << "\"}\n";
      return passed ? 0 : 2;
    }
    if (argc < 2 || argc > 3) {
      std::cerr << "usage: ouroboros-cgal-oracle <OUROTSQ1-file> [max-pairs]\n";
      return 64;
    }
    std::uint32_t max_pairs = kDefaultMaxPairs;
    if (argc == 3) {
      const std::string text(argv[2]);
      std::size_t consumed = 0;
      const auto parsed = std::stoul(text, &consumed, 10);
      if (consumed != text.size() || parsed == 0 || parsed > 100'000) {
        throw std::runtime_error("max-pairs must be an integer in [1,100000]");
      }
      max_pairs = static_cast<std::uint32_t>(parsed);
    }
    const auto bytes = read_file(argv[1]);
    const auto input = parse_input(bytes);
    const auto result = inspect(input, max_pairs);
    print_result(result);
    return result.has_intersections() || !result.degenerate_face_ids.empty() ? 1 : 0;
  } catch (const std::exception& error) {
    std::cerr << "ouroboros-cgal-oracle: " << error.what() << "\n";
    return 2;
  }
}
