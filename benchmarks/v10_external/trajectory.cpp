#include "trajectory.hpp"

#include <algorithm>
#include <cmath>
#include <cctype>
#include <fstream>
#include <limits>
#include <stdexcept>
#include <string>
#include <utility>

namespace holos_external {
namespace {

class Tokens {
public:
    explicit Tokens(const std::string& path) : input_(path) {
        if (!input_) {
            throw std::runtime_error("cannot open trajectory: " + path);
        }
    }

    std::string next(const char* label) {
        std::string value;
        if (!(input_ >> value)) {
            throw std::runtime_error(std::string("missing ") + label);
        }
        return value;
    }

    void expect(const char* expected) {
        const auto actual = next(expected);
        if (actual != expected) {
            throw std::runtime_error(
                "expected " + std::string(expected) + ", found " + actual);
        }
    }

    std::size_t size(const char* label) {
        const auto value = next(label);
        std::size_t consumed = 0;
        unsigned long long parsed = 0;
        try {
            parsed = std::stoull(value, &consumed, 10);
        } catch (const std::exception&) {
            throw std::runtime_error(std::string("invalid ") + label + ": " + value);
        }
        if (consumed != value.size() || parsed > std::numeric_limits<std::size_t>::max()) {
            throw std::runtime_error(std::string("invalid ") + label + ": " + value);
        }
        return static_cast<std::size_t>(parsed);
    }

    double real(const char* label) {
        const auto value = next(label);
        std::size_t consumed = 0;
        double parsed = 0.0;
        try {
            parsed = std::stod(value, &consumed);
        } catch (const std::exception&) {
            throw std::runtime_error(std::string("invalid ") + label + ": " + value);
        }
        if (consumed != value.size() || !std::isfinite(parsed) || parsed < 0.0) {
            throw std::runtime_error(std::string("invalid ") + label + ": " + value);
        }
        return parsed;
    }

    bool has_more() {
        std::string value;
        return static_cast<bool>(input_ >> value);
    }

private:
    std::ifstream input_;
};

void expect_positive(const char* label, std::size_t value) {
    if (value == 0) {
        throw std::runtime_error(std::string(label) + " must be positive");
    }
}

struct Header {
    std::string dataset;
    std::string source_sha256;
    std::size_t vertices;
    std::size_t edge_count;
    std::size_t snapshot_count;
};

std::string read_source_digest(Tokens& tokens) {
    tokens.expect("source_sha256");
    const auto digest = tokens.next("source SHA-256");
    const auto hexadecimal = std::all_of(
        digest.begin(), digest.end(),
        [](unsigned char value) { return std::isxdigit(value) != 0; });
    if (digest.size() != 64 || !hexadecimal) {
        throw std::runtime_error("source SHA-256 is not 64 hexadecimal digits");
    }
    return digest;
}

Header read_header(Tokens& tokens) {
    tokens.expect("HOLOSTEM1");
    tokens.expect("dataset");
    const auto dataset = tokens.next("dataset name");
    const auto source_sha256 = read_source_digest(tokens);
    tokens.expect("vertices");
    const auto vertices = tokens.size("vertex count");
    tokens.expect("edges");
    const auto edge_count = tokens.size("edge count");
    tokens.expect("snapshots");
    const auto snapshot_count = tokens.size("snapshot count");
    return {dataset, source_sha256, vertices, edge_count, snapshot_count};
}

void validate_counts(const Header& header) {
    expect_positive("vertex count", header.vertices);
    expect_positive("edge count", header.edge_count);
    if (header.snapshot_count < 2) {
        throw std::runtime_error("snapshot count must be at least two");
    }
    if (header.edge_count > 20'000'000 || header.snapshot_count > 1'000'000 ||
        header.edge_count > 20'000'000 / header.snapshot_count) {
        throw std::runtime_error("trajectory exceeds the benchmark cell limit");
    }
}

void read_constants(Tokens& tokens) {
    tokens.expect("bin_seconds");
    expect_positive("bin_seconds", tokens.size("bin_seconds"));
    tokens.expect("warmup_bins");
    expect_positive("warmup_bins", tokens.size("warmup_bins"));
    tokens.expect("decay_numerator");
    const auto decay_numerator = tokens.size("decay_numerator");
    tokens.expect("decay_denominator");
    const auto decay_denominator = tokens.size("decay_denominator");
    if (decay_denominator == 0 || decay_numerator >= decay_denominator) {
        throw std::runtime_error("invalid decay constants");
    }
    tokens.expect("score_scale");
    expect_positive("score_scale", tokens.size("score_scale"));
    tokens.expect("weight_offset");
    const auto weight_offset = tokens.size("weight_offset");
    expect_positive("weight_offset", weight_offset);
    tokens.expect("maximum_score");
    if (tokens.size("maximum_score") >= weight_offset) {
        throw std::runtime_error("invalid maximum score");
    }
}

void read_snapshot_times(Tokens& tokens, std::size_t snapshot_count) {
    tokens.expect("snapshot_times");
    std::size_t previous_time = 0;
    for (std::size_t index = 0; index < snapshot_count; ++index) {
        const auto current = tokens.size("snapshot time");
        if (index != 0 && current <= previous_time) {
            throw std::runtime_error("snapshot times are not strictly increasing");
        }
        previous_time = current;
    }
}

void validate_edge(
    const std::vector<Edge>& edges,
    std::size_t vertices,
    std::size_t u,
    std::size_t v) {
    const auto out_of_order = !edges.empty() &&
                              (u < edges.back().u ||
                               (u == edges.back().u && v <= edges.back().v));
    if (u >= v || v >= vertices || out_of_order) {
        throw std::runtime_error("trajectory edges are not canonical");
    }
}

std::pair<std::vector<Edge>, std::vector<Snapshot>> read_weights(
    Tokens& tokens,
    const Header& header) {
    tokens.expect("edge_weights");
    std::vector<Edge> edges;
    edges.reserve(header.edge_count);
    std::vector<Snapshot> snapshots(header.snapshot_count);
    for (auto& snapshot : snapshots) {
        snapshot.weights.reserve(header.edge_count);
    }
    for (std::size_t edge_index = 0; edge_index < header.edge_count; ++edge_index) {
        const auto u = tokens.size("edge endpoint");
        const auto v = tokens.size("edge endpoint");
        validate_edge(edges, header.vertices, u, v);
        edges.push_back({u, v});
        for (auto& snapshot : snapshots) {
            snapshot.weights.push_back(tokens.real("edge weight"));
        }
    }
    return {std::move(edges), std::move(snapshots)};
}

}

Trajectory read_trajectory(const std::string& path) {
    Tokens tokens(path);
    const auto header = read_header(tokens);
    validate_counts(header);
    read_constants(tokens);
    read_snapshot_times(tokens, header.snapshot_count);
    auto [edges, snapshots] = read_weights(tokens, header);
    if (tokens.has_more()) {
        throw std::runtime_error("trajectory has trailing fields");
    }
    return Trajectory{
        header.dataset,
        header.source_sha256,
        header.vertices,
        std::move(edges),
        std::move(snapshots)};
}

}
