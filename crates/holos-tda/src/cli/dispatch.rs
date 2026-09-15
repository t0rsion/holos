//! CLI entry-point dispatch and command handlers.

use clap::Parser;

use super::{
    args::*, bipersistence, cohomology, compute, coverage, index, persistent, portfolio, synthesis,
    verify,
};

/// Run the `holos` CLI on `argv` and return the process exit code.
///
/// `argv[0]` is the program name. The binary and the Python bindings both
/// enter here.
pub fn run_cli<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let argv: Vec<std::ffi::OsString> = argv.into_iter().map(Into::into).collect();
    let handler = subcommand_handler(argv.get(1).map(std::ffi::OsString::as_os_str));
    match handler {
        Some(handler) => handler(argv),
        None => run_main_command(argv),
    }
}

type CommandHandler = fn(Vec<std::ffi::OsString>) -> i32;

fn subcommand_handler(command: Option<&std::ffi::OsStr>) -> Option<CommandHandler> {
    let command = command?;
    SUBCOMMANDS
        .iter()
        .find_map(|(name, handler)| (command == std::ffi::OsStr::new(name)).then_some(*handler))
}

fn parse_command<C: Parser>(
    argv: Vec<std::ffi::OsString>,
    command: &str,
    run: fn(C) -> crate::Result<()>,
) -> i32 {
    let mut command_argv = Vec::with_capacity(argv.len().saturating_sub(1));
    command_argv.push(std::ffi::OsString::from(format!("holos {command}")));
    command_argv.extend(argv.into_iter().skip(2));
    match C::try_parse_from(command_argv) {
        Ok(cli) => finish_command(run(cli)),
        Err(error) => print_parse_error(error),
    }
}

fn run_main_command(argv: Vec<std::ffi::OsString>) -> i32 {
    match Cli::try_parse_from(argv) {
        Ok(cli) => finish_command(compute::run(cli)),
        Err(error) => print_parse_error(error),
    }
}

fn finish_command(result: crate::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("holos: {error}");
            1
        }
    }
}

fn print_parse_error(error: clap::Error) -> i32 {
    let code = error.exit_code();
    let _ = error.print();
    code
}

macro_rules! command_handler {
    ($handler:ident, $parser:ty, $run:path, $command:literal) => {
        fn $handler(argv: Vec<std::ffi::OsString>) -> i32 {
            parse_command::<$parser>(argv, $command, $run)
        }
    };
}

command_handler!(
    cohomology_command,
    CohomologyCli,
    cohomology::run_cohomology,
    "cohomology"
);
command_handler!(
    circular_command,
    CircularCli,
    cohomology::run_circular,
    "circular"
);
command_handler!(
    persistent_class_command,
    PersistentClassCli,
    persistent::run_persistent_class,
    "persistent-class"
);
command_handler!(
    persistent_circular_command,
    PersistentCircularCli,
    persistent::run_persistent_circular,
    "persistent-circular"
);
command_handler!(
    bipersistence_command,
    bipersistence::BipersistenceCli,
    bipersistence::run,
    "bipersistence"
);
command_handler!(
    kinetic_command,
    KineticCli,
    cohomology::run_kinetic,
    "kinetic"
);
command_handler!(
    collapse_portfolio_command,
    portfolio::CollapsePortfolioCli,
    portfolio::run_collapse_portfolio,
    "collapse-portfolio"
);
command_handler!(
    coverage_command,
    CoverageCli,
    coverage::run_coverage,
    "cover"
);
command_handler!(
    affine_coverage_command,
    AffineCoverageCli,
    coverage::run_affine_coverage,
    "cover-affine"
);
command_handler!(
    synthesis_command,
    SynthesisCli,
    synthesis::run_synthesis,
    "synthesize"
);
command_handler!(
    kinetic_synthesis_command,
    KineticSynthesisCli,
    synthesis::run_kinetic_synthesis,
    "synthesize-kinetic"
);
command_handler!(
    cohomology_intervention_command,
    CohomologyInterventionCli,
    synthesis::run_cohomology_intervention,
    "intervene-cohomology"
);
command_handler!(
    link_plan_command,
    LinkPlanCli,
    synthesis::run_link_plan,
    "plan-links"
);
command_handler!(
    merge_interfaces_command,
    MergeInterfacesCli,
    index::run_merge_interfaces,
    "merge-interfaces"
);
command_handler!(
    interface_command,
    InterfaceCli,
    index::run_interface,
    "interface"
);
command_handler!(index_command, IndexCli, index::run_index, "index");
command_handler!(prove_command, ProveCli, index::run_prove, "prove");
command_handler!(
    verify_program_command,
    VerifyProgramCli,
    verify::run_verify_program,
    "verify-program"
);
command_handler!(
    verify_program_trace_command,
    VerifyProgramTraceCli,
    verify::run_verify_program_trace,
    "verify-program-trace"
);
command_handler!(
    intervene_command,
    InterveneCli,
    verify::run_intervene,
    "intervene"
);
command_handler!(
    verify_intervention_command,
    VerifyInterventionCli,
    verify::run_verify_intervention,
    "verify-intervention"
);
command_handler!(
    verify_atlas_command,
    VerifyAtlasCli,
    verify::run_verify_atlas,
    "verify-atlas"
);
command_handler!(
    verify_trajectory_command,
    VerifyTrajectoryCli,
    verify::run_verify_trajectory,
    "verify-trajectory"
);
command_handler!(
    verify_collapse_command,
    VerifyCli,
    verify::run_verify,
    "verify-collapse"
);

const SUBCOMMANDS: &[(&str, CommandHandler)] = &[
    ("bipersistence", bipersistence_command),
    ("collapse-portfolio", collapse_portfolio_command),
    ("cohomology", cohomology_command),
    ("circular", circular_command),
    ("persistent-class", persistent_class_command),
    ("persistent-circular", persistent_circular_command),
    ("kinetic", kinetic_command),
    ("cover", coverage_command),
    ("cover-affine", affine_coverage_command),
    ("synthesize", synthesis_command),
    ("synthesize-kinetic", kinetic_synthesis_command),
    ("intervene-cohomology", cohomology_intervention_command),
    ("plan-links", link_plan_command),
    ("merge-interfaces", merge_interfaces_command),
    ("interface", interface_command),
    ("index", index_command),
    ("prove", prove_command),
    ("verify-program", verify_program_command),
    ("verify-program-trace", verify_program_trace_command),
    ("intervene", intervene_command),
    ("verify-intervention", verify_intervention_command),
    ("verify-atlas", verify_atlas_command),
    ("verify-trajectory", verify_trajectory_command),
    ("verify-collapse", verify_collapse_command),
];
