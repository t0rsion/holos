use super::super::model::{Args, Config, Kind, Mode};

pub(super) fn configurations(args: &Args) -> Vec<Config> {
    let mut configs = Vec::new();
    if matches!(args.mode, Mode::V2 | Mode::Rounds | Mode::All) {
        for &threads in &args.collapse_threads {
            configs.push(Config {
                name: format!("v2-c{threads}"),
                kind: Kind::V2,
                collapse_threads: threads,
            });
        }
    }
    if matches!(args.mode, Mode::V1 | Mode::Rounds | Mode::All) {
        configs.push(Config {
            name: "v1-c1".to_string(),
            kind: Kind::V1,
            collapse_threads: 1,
        });
    }
    // The ordered configurations follow v1-c1, so the gate always has its
    // reference when an ordered result arrives.
    if matches!(args.mode, Mode::V1Ordered | Mode::All) {
        for &threads in &args.collapse_threads {
            configs.push(Config {
                name: format!("v1o-c{threads}"),
                kind: Kind::V1Ordered,
                collapse_threads: threads,
            });
        }
    }
    // One shipped-pipeline configuration, at the last collapse thread count.
    // The end-to-end comparison is defined at P alone.
    if matches!(args.mode, Mode::V1Product | Mode::All) {
        let threads = *args.collapse_threads.last().unwrap_or(&1);
        configs.push(Config {
            name: format!("v1p-c{threads}"),
            kind: Kind::V1Product,
            collapse_threads: threads,
        });
    }
    if matches!(args.mode, Mode::NoCollapse | Mode::Rounds | Mode::All) {
        configs.push(Config {
            name: "none".to_string(),
            kind: Kind::NoCollapse,
            collapse_threads: 0,
        });
    }
    configs
}
