use super::*;

// What the construction costs in counters: entries, comparisons, and
// the largest heap capacity. Not a gate.
#[test]
#[ignore]
fn heap_construction_counters() {
    let mut rng = Rng::new(0x8eaa_1f70_0000_0002);
    for (label, dist, max_dim) in [
        ("cube-d1", cloud(&mut rng, 200, 3), 1),
        ("cube-d2", cloud(&mut rng, 90, 3), 2),
        ("lattice-d1", lattice(12), 1),
    ] {
        let params = RipsParams::new(max_dim);
        let (a, b) = heaps_agree(label, &dist, &params, Z2);
        println!(
            "HEAP {label} entries={} comparisons: pushes={} heapify={} ratio={:.3} \
                 capacity: pushes={} heapify={}",
            b.entries,
            a.comparisons,
            b.comparisons,
            b.comparisons as f64 / a.comparisons as f64,
            a.capacity,
            b.capacity
        );
    }
}
