#pragma once

#include <cstddef>
#include <string>
#include <vector>

namespace holos_external {

struct Edge {
    std::size_t u;
    std::size_t v;
};

struct Snapshot {
    std::vector<double> weights;
};

struct Trajectory {
    std::string dataset;
    std::string source_sha256;
    std::size_t vertices;
    std::vector<Edge> edges;
    std::vector<Snapshot> snapshots;
};

Trajectory read_trajectory(const std::string& path);

}
