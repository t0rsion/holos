#include "trajectory.hpp"

#include <bats.hpp>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <sstream>
#include <tuple>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {

using Clock = std::chrono::steady_clock;
using Complex = bats::LightSimplicialComplex<
    std::size_t, std::unordered_map<std::size_t, std::size_t>>;

struct Bar {
    std::size_t dim;
    double birth;
    double death;
};

struct Model {
    Complex complex;
    std::unordered_map<std::uint64_t, std::size_t> edge_index;
};

struct Timing {
    std::uint64_t compile_ns = 0;
    std::uint64_t update_ns = 0;
    std::uint64_t full_update_ns = 0;
    std::uint64_t fresh_ns = 0;
    std::vector<std::uint64_t> compile_samples;
    std::vector<std::uint64_t> update_samples;
    std::vector<std::uint64_t> full_update_samples;
    std::vector<std::uint64_t> fresh_samples;
};

struct Options {
    std::string trajectory;
    std::string bars;
    std::size_t reps = 5;
    std::size_t modulus = 2;
};

std::uint64_t edge_key(std::size_t u, std::size_t v) {
    if (u > v) {
        std::swap(u, v);
    }
    return (static_cast<std::uint64_t>(u) << 32U) | static_cast<std::uint64_t>(v);
}

std::size_t parse_repetitions(const std::string& value) {
    std::size_t consumed = 0;
    const auto repetitions = std::stoull(value, &consumed);
    if (consumed != value.size() || repetitions < 5) {
        throw std::runtime_error("--reps must be at least 5");
    }
    return repetitions;
}

std::size_t parse_modulus(const std::string& value) {
    std::size_t consumed = 0;
    const auto modulus = std::stoull(value, &consumed);
    const auto supported = modulus == 2 || modulus == 3 || modulus == 5;
    if (consumed != value.size() || !supported) {
        throw std::runtime_error("--modulus must be 2, 3, or 5");
    }
    return modulus;
}

Options parse_options(int argc, char** argv) {
    if (argc < 2) {
        throw std::runtime_error(
            "usage: bats-warm TRAJECTORY [--bars PATH] [--reps N] [--modulus P]");
    }
    Options options;
    options.trajectory = argv[1];
    for (int index = 2; index < argc; ++index) {
        const std::string argument = argv[index];
        if (index + 1 >= argc) {
            throw std::runtime_error(argument + " needs a value");
        }
        const std::string value = argv[++index];
        if (argument == "--bars") {
            options.bars = value;
        } else if (argument == "--reps") {
            options.reps = parse_repetitions(value);
        } else if (argument == "--modulus") {
            options.modulus = parse_modulus(value);
        } else {
            throw std::runtime_error("unknown option " + argument);
        }
    }
    return options;
}

Model make_model(const holos_external::Trajectory& trajectory) {
    Model model{Complex(trajectory.vertices, 2), {}};
    for (std::size_t vertex = 0; vertex < trajectory.vertices; ++vertex) {
        model.complex.add({vertex});
    }
    model.edge_index.reserve(trajectory.edges.size() * 2);
    for (std::size_t index = 0; index < trajectory.edges.size(); ++index) {
        const auto edge = trajectory.edges[index];
        model.edge_index.emplace(edge_key(edge.u, edge.v), index);
        model.complex.add({edge.u, edge.v});
    }

    std::vector<std::vector<bool>> adjacency(
        trajectory.vertices, std::vector<bool>(trajectory.vertices, false));
    for (const auto edge : trajectory.edges) {
        adjacency[edge.u][edge.v] = true;
        adjacency[edge.v][edge.u] = true;
    }
    for (std::size_t u = 0; u < trajectory.vertices; ++u) {
        for (std::size_t v = u + 1; v < trajectory.vertices; ++v) {
            if (!adjacency[u][v]) {
                continue;
            }
            for (std::size_t w = v + 1; w < trajectory.vertices; ++w) {
                if (adjacency[u][w] && adjacency[v][w]) {
                    model.complex.add({u, v, w});
                }
            }
        }
    }
    return model;
}

std::vector<double> values_for(
    const Model& model,
    const holos_external::Snapshot& snapshot,
    std::size_t dimension) {
    std::vector<double> values(model.complex.ncells(dimension));
    for (std::size_t index = 0; index < values.size(); ++index) {
        const auto simplex = model.complex.get_simplex(dimension, index);
        if (dimension == 0) {
            values[index] = 0.0;
        } else {
            double filtration = 0.0;
            for (std::size_t left = 0; left < simplex.size(); ++left) {
                for (std::size_t right = left + 1; right < simplex.size(); ++right) {
                    const auto edge = model.edge_index.find(
                        edge_key(simplex[left], simplex[right]));
                    if (edge == model.edge_index.end()) {
                        throw std::runtime_error("complex contains a missing edge");
                    }
                    filtration = std::max(filtration, snapshot.weights[edge->second]);
                }
            }
            values[index] = filtration;
        }
    }
    return values;
}

template <int Prime>
auto make_filtration(const Model& model, const holos_external::Snapshot& snapshot) {
    using Filtration = bats::Filtration<double, Complex>;
    std::vector<std::vector<double>> values;
    values.reserve(3);
    for (std::size_t dimension = 0; dimension <= model.complex.maxdim(); ++dimension) {
        values.push_back(values_for(model, snapshot, dimension));
    }
    return Filtration(model.complex, values);
}

template <typename Reduced>
std::vector<Bar> bars(const Reduced& reduced) {
    std::vector<Bar> output;
    for (std::size_t dimension = 0; dimension <= 1; ++dimension) {
        for (const auto& pair : reduced.persistence_pairs(dimension)) {
            if (std::isinf(pair.death) || pair.death > pair.birth) {
                output.push_back({dimension, pair.birth, pair.death});
            }
        }
    }
    std::sort(output.begin(), output.end(), [](const Bar& left, const Bar& right) {
        if (left.dim != right.dim) {
            return left.dim < right.dim;
        }
        if (left.birth != right.birth) {
            return left.birth < right.birth;
        }
        if (std::isinf(left.death) != std::isinf(right.death)) {
            return !std::isinf(left.death);
        }
        return left.death < right.death;
    });
    return output;
}

bool equal_bars(const std::vector<Bar>& left, const std::vector<Bar>& right) {
    if (left.size() != right.size()) {
        return false;
    }
    for (std::size_t index = 0; index < left.size(); ++index) {
        if (left[index].dim != right[index].dim ||
            left[index].birth != right[index].birth ||
            left[index].death != right[index].death) {
            return false;
        }
    }
    return true;
}

std::uint64_t elapsed_ns(const Clock::time_point start) {
    return static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() - start).count());
}

std::uint64_t median(std::vector<std::uint64_t> values) {
    std::sort(values.begin(), values.end());
    return values[values.size() / 2];
}

std::string samples(const std::vector<std::uint64_t>& values) {
    std::string output;
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output += std::to_string(values[index]);
    }
    return output;
}

void write_bars(
    const std::string& path,
    const holos_external::Trajectory& trajectory,
    const std::vector<std::vector<Bar>>& values) {
    if (path.empty()) {
        return;
    }
    std::ofstream output(path);
    if (!output) {
        throw std::runtime_error("cannot create bars output: " + path);
    }
    output << "format=holos-bats-bars-v1 dataset=" << trajectory.dataset
           << " snapshots=" << values.size() << "\n";
    output << std::setprecision(std::numeric_limits<double>::max_digits10);
    for (std::size_t snapshot = 0; snapshot < values.size(); ++snapshot) {
        for (const auto bar : values[snapshot]) {
            output << snapshot << ' ' << bar.dim << ' ' << bar.birth << ' ';
            if (std::isinf(bar.death)) {
                output << "inf";
            } else {
                output << bar.death;
            }
            output << '\n';
        }
    }
}

template <typename Field, typename Filtration, typename Reduced>
std::vector<std::vector<Bar>> expected_bars(
    const std::vector<Filtration>& filtrations,
    const Reduced& initial) {
    std::vector<std::vector<Bar>> expected;
    expected.reserve(filtrations.size());
    expected.push_back(bars(initial));
    for (std::size_t index = 1; index < filtrations.size(); ++index) {
        auto chain = bats::Chain(filtrations[index], Field());
        expected.push_back(bars(bats::Reduce(chain)));
    }
    return expected;
}

template <typename Field, typename Filtration>
void measure_compile(
    const std::vector<Filtration>& filtrations,
    const std::vector<std::vector<Bar>>& expected,
    std::size_t repetitions,
    Timing& timing) {
    for (std::size_t repetition = 0; repetition < repetitions; ++repetition) {
        const auto start = Clock::now();
        auto chain = bats::Chain(filtrations.front(), Field());
        auto reduced = bats::Reduce(chain);
        timing.compile_samples.push_back(elapsed_ns(start));
        if (repetition == 0 && !equal_bars(bars(reduced), expected.front())) {
            throw std::runtime_error("initial BATS barcode is not deterministic");
        }
    }
    timing.compile_ns = median(timing.compile_samples);
}

template <typename Reduced, typename UpdateInfo>
void measure_warm_update(
    const Reduced& initial,
    const std::vector<UpdateInfo>& updates,
    std::size_t repetitions,
    Timing& timing) {
    for (std::size_t repetition = 0; repetition < repetitions; ++repetition) {
        auto reduced = initial;
        const auto start = Clock::now();
        for (const auto& update : updates) {
            reduced.update_filtration_general(update);
        }
        timing.update_samples.push_back(elapsed_ns(start));
    }
    timing.update_ns = median(timing.update_samples);
}

template <typename Reduced, typename UpdateInfo>
std::vector<std::vector<Bar>> checked_warm_bars(
    const Reduced& initial,
    const std::vector<UpdateInfo>& updates,
    const std::vector<std::vector<Bar>>& expected) {
    std::vector<std::vector<Bar>> values;
    values.reserve(expected.size());
    auto reduced = initial;
    values.push_back(bars(reduced));
    for (std::size_t index = 0; index < updates.size(); ++index) {
        reduced.update_filtration_general(updates[index]);
        values.push_back(bars(reduced));
        if (!equal_bars(values.back(), expected[index + 1])) {
            throw std::runtime_error("BATS warm update disagrees with fresh reduction");
        }
    }
    return values;
}

template <int Prime, typename Filtration, typename Reduced>
void measure_full_update(
    const Model& model,
    const holos_external::Trajectory& trajectory,
    const std::vector<Filtration>& filtrations,
    const Reduced& initial,
    std::size_t repetitions,
    Timing& timing) {
    using UpdateInfo = bats::Update_info<Filtration>;
    for (std::size_t repetition = 0; repetition < repetitions; ++repetition) {
        auto reduced = initial;
        const auto start = Clock::now();
        for (std::size_t index = 1; index < filtrations.size(); ++index) {
            auto next = make_filtration<Prime>(model, trajectory.snapshots[index]);
            UpdateInfo update(filtrations[index - 1], next);
            reduced.update_filtration_general(update);
        }
        timing.full_update_samples.push_back(elapsed_ns(start));
    }
    timing.full_update_ns = median(timing.full_update_samples);
}

template <typename Field, typename Filtration>
void measure_fresh(
    const std::vector<Filtration>& filtrations,
    std::size_t repetitions,
    Timing& timing) {
    for (std::size_t repetition = 0; repetition < repetitions; ++repetition) {
        const auto start = Clock::now();
        for (std::size_t index = 1; index < filtrations.size(); ++index) {
            auto chain = bats::Chain(filtrations[index], Field());
            auto reduced = bats::Reduce(chain);
            (void)reduced;
        }
        timing.fresh_samples.push_back(elapsed_ns(start));
    }
    timing.fresh_ns = median(timing.fresh_samples);
}

template <int Prime>
std::string timing_record(
    const holos_external::Trajectory& trajectory,
    const Model& model,
    const Options& options,
    const Timing& timing,
    std::uint64_t preparation_ns) {
    const auto cells = model.complex.ncells(0) + model.complex.ncells(1) +
                       model.complex.ncells(2);
    const auto speedup = static_cast<double>(timing.fresh_ns) /
                         static_cast<double>(timing.update_ns);
    const auto full_speedup = static_cast<double>(timing.fresh_ns) /
                              static_cast<double>(timing.full_update_ns);
    std::ostringstream record;
    record << std::setprecision(6)
           << "format=holos-bats-warm-v1 dataset=" << trajectory.dataset
           << " vertices=" << trajectory.vertices << " edges=" << trajectory.edges.size()
           << " snapshots=" << trajectory.snapshots.size() << " modulus=" << Prime
           << " reps=" << options.reps << " cells=" << cells
           << " triangles=" << model.complex.ncells(2)
           << " preparation_ns=" << preparation_ns
           << " compile_ns=" << timing.compile_ns << " update_ns=" << timing.update_ns
           << " full_update_ns=" << timing.full_update_ns << " fresh_ns=" << timing.fresh_ns
           << " update_speedup=" << speedup << " full_speedup=" << full_speedup
           << " exact=yes"
           << " compile_samples_ns=" << samples(timing.compile_samples)
           << " update_samples_ns=" << samples(timing.update_samples)
           << " full_update_samples_ns=" << samples(timing.full_update_samples)
           << " fresh_samples_ns=" << samples(timing.fresh_samples);
    return record.str();
}

template <int Prime>
std::string run(const holos_external::Trajectory& trajectory, const Options& options) {
    using Field = ModP<int, Prime>;
    using Filtration = bats::Filtration<double, Complex>;
    using UpdateInfo = bats::Update_info<Filtration>;

    const auto preparation_start = Clock::now();
    const auto model = make_model(trajectory);
    std::vector<Filtration> filtrations;
    filtrations.reserve(trajectory.snapshots.size());
    for (const auto& snapshot : trajectory.snapshots) {
        filtrations.push_back(make_filtration<Prime>(model, snapshot));
    }
    std::vector<UpdateInfo> updates;
    updates.reserve(filtrations.size() - 1);
    for (std::size_t index = 1; index < filtrations.size(); ++index) {
        updates.emplace_back(filtrations[index - 1], filtrations[index]);
    }
    const auto preparation_ns = elapsed_ns(preparation_start);

    auto initial_chain = bats::Chain(filtrations.front(), Field());
    auto initial = bats::Reduce(initial_chain);
    const auto expected = expected_bars<Field>(filtrations, initial);

    Timing timing;
    measure_compile<Field>(filtrations, expected, options.reps, timing);
    measure_warm_update(initial, updates, options.reps, timing);
    const auto warm_values = checked_warm_bars(initial, updates, expected);
    measure_full_update<Prime>(
        model, trajectory, filtrations, initial, options.reps, timing);
    measure_fresh<Field>(filtrations, options.reps, timing);
    write_bars(options.bars, trajectory, warm_values);
    return timing_record<Prime>(trajectory, model, options, timing, preparation_ns);
}

}

int main(int argc, char** argv) {
    try {
        const auto options = parse_options(argc, argv);
        const auto trajectory = holos_external::read_trajectory(options.trajectory);
        switch (options.modulus) {
        case 2:
            std::cout << run<2>(trajectory, options) << '\n';
            break;
        case 3:
            std::cout << run<3>(trajectory, options) << '\n';
            break;
        case 5:
            std::cout << run<5>(trajectory, options) << '\n';
            break;
        default:
            throw std::runtime_error("unsupported modulus");
        }
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "bats-warm: " << error.what() << '\n';
        return 1;
    }
}
