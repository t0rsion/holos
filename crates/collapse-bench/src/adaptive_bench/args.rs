use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    None,
    V1,
    V2,
    V3H1,
    V3H2,
}

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::V3H1 => "v3-h1",
            Self::V3H2 => "v3-h2",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "v1" => Ok(Self::V1),
            "v2" => Ok(Self::V2),
            "v3-h1" => Ok(Self::V3H1),
            "v3-h2" => Ok(Self::V3H2),
            _ => Err(format!(
                "unknown configuration {value}; use none, v1, v2, v3-h1, or v3-h2"
            )),
        }
    }
}

pub(super) struct Args {
    pub(super) input: String,
    pub(super) entry: String,
    pub(super) threshold: f64,
    pub(super) threshold_text: String,
    pub(super) max_dim: usize,
    pub(super) modulus: u32,
    pub(super) threads: usize,
    pub(super) reps: usize,
    pub(super) kinds: Vec<Kind>,
    pub(super) work_limit: Option<u64>,
}

struct ArgsBuilder {
    input: Option<String>,
    entry: Option<String>,
    threshold_text: Option<String>,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    reps: usize,
    kinds: Vec<Kind>,
    work_limit: Option<u64>,
}

impl Default for ArgsBuilder {
    fn default() -> Self {
        Self {
            input: None,
            entry: None,
            threshold_text: None,
            max_dim: 2,
            modulus: 2,
            threads: 1,
            reps: 5,
            kinds: vec![Kind::None, Kind::V1, Kind::V2, Kind::V3H1, Kind::V3H2],
            work_limit: None,
        }
    }
}

pub(super) fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut builder = ArgsBuilder::default();
    let mut arguments = argv.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        parse_argument(&mut builder, flag, value)?;
    }
    finish_args(builder)
}

fn parse_argument(builder: &mut ArgsBuilder, flag: &str, value: &str) -> Result<(), String> {
    if matches!(flag, "--input" | "--entry" | "--threshold" | "--configs") {
        return parse_text_argument(builder, flag, value);
    }
    if matches!(
        flag,
        "--max-dim" | "--modulus" | "--threads" | "--reps" | "--work-limit"
    ) {
        return parse_numeric_argument(builder, flag, value);
    }
    Err(format!("unknown argument {flag}; run with --help"))
}

fn parse_text_argument(builder: &mut ArgsBuilder, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "--input" => builder.input = Some(value.to_string()),
        "--entry" => builder.entry = Some(value.to_string()),
        "--threshold" => builder.threshold_text = Some(value.to_string()),
        "--configs" => builder.kinds = parse_kinds(value)?,
        _ => unreachable!(),
    }
    Ok(())
}

fn parse_numeric_argument(
    builder: &mut ArgsBuilder,
    flag: &str,
    value: &str,
) -> Result<(), String> {
    let number = value
        .parse::<u64>()
        .map_err(|_| format!("{flag} {value} is not a whole number"))?;
    match flag {
        "--max-dim" => set_usize(&mut builder.max_dim, number, flag),
        "--modulus" => set_u32(&mut builder.modulus, number, flag),
        "--threads" => set_threads(&mut builder.threads, number, flag),
        "--reps" => set_usize(&mut builder.reps, number, flag),
        "--work-limit" => {
            builder.work_limit = Some(number);
            Ok(())
        }
        _ => unreachable!(),
    }
}

fn set_usize(target: &mut usize, number: u64, flag: &str) -> Result<(), String> {
    *target = usize::try_from(number).map_err(|_| format!("{flag} is out of range"))?;
    Ok(())
}

fn set_u32(target: &mut u32, number: u64, flag: &str) -> Result<(), String> {
    *target = u32::try_from(number).map_err(|_| format!("{flag} is out of range"))?;
    Ok(())
}

fn set_threads(target: &mut usize, number: u64, flag: &str) -> Result<(), String> {
    set_usize(target, number, flag)?;
    *target = (*target).max(1);
    Ok(())
}

fn parse_kinds(text: &str) -> Result<Vec<Kind>, String> {
    let kinds: Vec<_> = text.split(',').map(Kind::parse).collect::<Result<_, _>>()?;
    if kinds.is_empty() {
        return Err("--configs must name at least one configuration".to_string());
    }
    let mut deduplicated = Vec::with_capacity(kinds.len());
    for kind in kinds {
        if !deduplicated.contains(&kind) {
            deduplicated.push(kind);
        }
    }
    Ok(deduplicated)
}

fn finish_args(builder: ArgsBuilder) -> Result<Args, String> {
    let input = builder
        .input
        .ok_or_else(|| "--input is required".to_string())?;
    let threshold_text = builder
        .threshold_text
        .ok_or_else(|| "--threshold is required".to_string())?;
    let threshold: f64 = threshold_text
        .parse()
        .map_err(|_| format!("--threshold {threshold_text} is not a number"))?;
    if threshold.is_nan() || threshold < 0.0 {
        return Err(format!("--threshold {threshold_text} must be non-negative"));
    }
    if builder.reps == 0 {
        return Err("--reps must be at least 1".to_string());
    }
    let entry = builder.entry.unwrap_or_else(|| file_stem(&input));
    Ok(Args {
        input,
        entry,
        threshold,
        threshold_text,
        max_dim: builder.max_dim,
        modulus: builder.modulus,
        threads: builder.threads,
        reps: builder.reps,
        kinds: builder.kinds,
        work_limit: builder.work_limit,
    })
}

pub(super) fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
