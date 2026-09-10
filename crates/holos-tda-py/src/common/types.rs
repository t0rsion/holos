//! Python tuple records shared by binding modules.

pub(crate) type Bars = Vec<(usize, f64, f64)>;
pub(crate) type ClassRecord = (
    String,
    String,
    usize,
    f64,
    Option<f64>,
    u32,
    f64,
    Vec<(usize, usize, u32)>,
);
pub(crate) type ScalarClassRecord = (
    String,
    String,
    usize,
    f64,
    Option<f64>,
    u32,
    f64,
    Vec<(usize, usize, u32)>,
    String,
    String,
);
pub(crate) type Explained = (Bars, Vec<ScalarClassRecord>);
pub(crate) type PersistentClassInput = (
    f64,
    Option<f64>,
    u32,
    f64,
    Vec<(usize, usize, i64)>,
    (String, String, usize, String, String),
    (f64, Option<f64>, u32, f64),
);
pub(crate) type CircularCoordinateRecord = (Vec<f64>, u32, u64, f64, f64, usize, Vec<(usize, u32)>);
pub(crate) type CircularResult = (
    Vec<u8>,
    CircularCoordinateRecord,
    String,
    usize,
    Option<CircularCoordinateRecord>,
);
pub(crate) type GradientRecord = (String, Vec<(usize, usize)>);
pub(crate) type CriticalRecord = (Vec<usize>, f64, Option<(Vec<usize>, f64)>);
pub(crate) type SpaceRecord = (
    String,
    String,
    f64,
    Option<f64>,
    Vec<ClassRecord>,
    Vec<CriticalRecord>,
);
pub(crate) type SensitivityRecord = (String, GradientRecord, GradientRecord);
pub(crate) type AtlasResult = (Bars, Vec<SpaceRecord>, Vec<SensitivityRecord>);
pub(crate) type EventRecord = (
    String,
    Option<(usize, usize)>,
    Option<(usize, usize)>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
);
pub(crate) type PointGradientValue = ((usize, usize), Vec<(usize, usize, f64)>);
pub(crate) type PointGradientRecord = Option<PointGradientValue>;
pub(crate) type PointSensitivityRecord = (String, PointGradientRecord, PointGradientRecord);
pub(crate) type ProgramSpaceRecord = (
    String,
    f64,
    Option<f64>,
    Vec<ClassRecord>,
    Vec<CriticalRecord>,
);
pub(crate) type ProgramResult = (Bars, Vec<ProgramSpaceRecord>);
pub(crate) type ProgramAtomRecord = (usize, Vec<usize>, Vec<(usize, usize)>, Vec<usize>, bool);
pub(crate) type ProgramSummaryRecord =
    (usize, usize, usize, usize, usize, usize, bool, usize, usize);
pub(crate) type WorkRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
);
pub(crate) type ProgramEventRecord = (
    String,
    Option<usize>,
    Option<(usize, usize)>,
    Option<String>,
);
pub(crate) type TransportRecord = (String, String, u32);
pub(crate) type ContinuationRecord = (String, Vec<String>, Vec<String>, Vec<TransportRecord>);
pub(crate) type CorrespondenceTermRecord = (String, u32);
pub(crate) type CorrespondenceVectorRecord =
    (Vec<CorrespondenceTermRecord>, Vec<CorrespondenceTermRecord>);
pub(crate) type CorrespondenceRecord = (
    String,
    String,
    f64,
    usize,
    usize,
    usize,
    usize,
    usize,
    Vec<CorrespondenceVectorRecord>,
);
pub(crate) type ProgramUpdateRecord = (
    String,
    Vec<ProgramEventRecord>,
    Vec<ContinuationRecord>,
    Vec<CorrespondenceRecord>,
    WorkRecord,
    ProgramResult,
);
pub(crate) type InterventionRecord = (
    String,
    String,
    f64,
    Option<f64>,
    Vec<((usize, usize), f64, f64)>,
    Option<ProgramResult>,
    Option<Vec<u8>>,
);
pub(crate) type IndexSummaryRecord = (
    (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
    ),
    (usize, usize, usize, usize, usize),
    (bool, usize, bool),
);
pub(crate) type InterfaceRecord = (
    String,
    usize,
    Vec<usize>,
    Vec<usize>,
    Vec<usize>,
    usize,
    usize,
    String,
    usize,
    Vec<usize>,
    (usize, usize, usize),
);
pub(crate) type IndexWorkRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    (usize, usize, usize, usize, usize),
    usize,
    usize,
    usize,
);
pub(crate) type IndexEventRecord = (String, Option<String>, Option<(usize, usize)>);
pub(crate) type IndexUpdateRecord = (
    String,
    Bars,
    Bars,
    Bars,
    Vec<IndexEventRecord>,
    Vec<CorrespondenceRecord>,
    IndexWorkRecord,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
);
pub(crate) type IndexDiffRecord = (bool, usize, Bars, Bars);
pub(crate) type RelativeInterfaceRecord = (Vec<u8>, Bars, (usize, usize, usize));
pub(crate) type DistributedInterfaceRecord =
    (Vec<u8>, Vec<u8>, String, (usize, usize, usize, usize));
pub(crate) type FixedCohomologyRecord = (String, Vec<usize>, Vec<(String, Vec<(Vec<usize>, u32)>)>);
pub(crate) type CohomologyRelationRecord = (
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    usize,
    bool,
    Vec<(Vec<(String, u32)>, Vec<(String, u32)>)>,
);
pub(crate) type AffineEventRecord = (f64, f64, f64, Vec<String>);
pub(crate) type AffineCohomologyRecord = (f64, usize, usize, usize);
pub(crate) type KineticZigzagNodeRecord = (String, f64, usize, usize, String);
pub(crate) type KineticZigzagArrowRecord = (String, usize);
pub(crate) type KineticZigzagIntervalRecord = (String, usize, usize, usize);
pub(crate) type KineticZigzagRecord = (
    Vec<u8>,
    String,
    usize,
    Vec<KineticZigzagNodeRecord>,
    Vec<KineticZigzagArrowRecord>,
    Vec<KineticZigzagIntervalRecord>,
    Vec<usize>,
);
pub(crate) type CohomologyScenarioInput = (Vec<(usize, usize, f64)>, usize);
pub(crate) type CohomologyInterventionRecord = (
    Vec<u8>,
    String,
    Vec<(usize, usize, u64)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    Vec<Vec<usize>>,
    Vec<usize>,
    Vec<usize>,
);
pub(crate) type SynthesisRecord = (
    Vec<u8>,
    String,
    Vec<(usize, usize, u64)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    usize,
    Vec<usize>,
    Vec<usize>,
);
pub(crate) type CoverageCandidateInput = (usize, u64, Option<Vec<usize>>);
pub(crate) type RelativeCoverageRecord = (bool, Vec<(usize, usize, usize, u32)>, usize, usize);
pub(crate) type CoverageRecord = (
    Vec<u8>,
    String,
    Vec<(usize, u64, Vec<usize>)>,
    Option<u64>,
    Option<u64>,
    usize,
    usize,
    usize,
    usize,
    usize,
    Option<usize>,
    usize,
);
pub(crate) type PortfolioRecord = (Vec<u8>, usize, Vec<(String, Vec<u64>, usize)>);
pub(crate) type ExplicitRecord = (Vec<u8>, Bars, Vec<usize>, Vec<usize>);
