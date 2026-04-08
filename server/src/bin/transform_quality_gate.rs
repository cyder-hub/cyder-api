use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use cyder_api::service::transform::quality::{
    BenchmarkSummary, BenchmarkThresholds, TransformQualityReport, build_transform_quality_report,
};

const DEFAULT_THRESHOLDS_PATH: &str = "task/transform-quality-thresholds.json";

struct GateArgs {
    quick: bool,
    thresholds_path: PathBuf,
    report_out: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            print_report_summary(&report);
            if report.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(err) => {
            eprintln!("transform quality gate failed: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<TransformQualityReport, String> {
    let args = parse_args(env::args_os().skip(1).collect())?;
    let workspace_root = workspace_root();
    let thresholds_path = absolutize(&workspace_root, &args.thresholds_path);
    let thresholds: BenchmarkThresholds = serde_json::from_slice(
        &fs::read(&thresholds_path)
            .map_err(|err| format!("read thresholds {}: {err}", thresholds_path.display()))?,
    )
    .map_err(|err| format!("parse thresholds {}: {err}", thresholds_path.display()))?;

    let bench_json_path = temp_report_path("transform-benchmark-summary.json");
    let benchmark_summary = run_benchmark(&workspace_root, args.quick, &bench_json_path)?;
    let report = build_transform_quality_report(benchmark_summary, &thresholds);

    if let Some(report_out) = args.report_out {
        let report_path = absolutize(&workspace_root, &report_out);
        let payload = serde_json::to_vec_pretty(&report)
            .map_err(|err| format!("serialize quality report: {err}"))?;
        if let Some(parent) = report_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("create report directory {}: {err}", parent.display()))?;
        }
        fs::write(&report_path, payload)
            .map_err(|err| format!("write report {}: {err}", report_path.display()))?;
        eprintln!(
            "Wrote transform quality report to {}",
            report_path.display()
        );
    }

    Ok(report)
}

fn parse_args(args: Vec<OsString>) -> Result<GateArgs, String> {
    let mut quick = false;
    let mut thresholds_path = PathBuf::from(DEFAULT_THRESHOLDS_PATH);
    let mut report_out = None;
    let mut index = 0;

    while index < args.len() {
        let current = args[index]
            .to_str()
            .ok_or_else(|| "non-utf8 argument is not supported".to_string())?;
        match current {
            "--quick" => {
                quick = true;
                index += 1;
            }
            "--thresholds" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--thresholds requires a path".to_string());
                };
                thresholds_path = PathBuf::from(value);
                index += 2;
            }
            "--report-out" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--report-out requires a path".to_string());
                };
                report_out = Some(PathBuf::from(value));
                index += 2;
            }
            other => {
                return Err(format!("unsupported argument: {other}"));
            }
        }
    }

    Ok(GateArgs {
        quick,
        thresholds_path,
        report_out,
    })
}

fn run_benchmark(
    workspace_root: &Path,
    quick: bool,
    json_out: &Path,
) -> Result<BenchmarkSummary, String> {
    let mut command = Command::new("cargo");
    command.current_dir(workspace_root);
    command.args([
        "bench",
        "-p",
        "cyder-api",
        "--bench",
        "transform_benchmark",
        "--",
    ]);
    if quick {
        command.arg("--quick");
    }
    command.arg("--json-out");
    command.arg(json_out);

    let status = command
        .status()
        .map_err(|err| format!("spawn cargo bench: {err}"))?;
    if !status.success() {
        return Err(format!("cargo bench exited with status {status}"));
    }

    serde_json::from_slice(
        &fs::read(json_out)
            .map_err(|err| format!("read benchmark summary {}: {err}", json_out.display()))?,
    )
    .map_err(|err| format!("parse benchmark summary {}: {err}", json_out.display()))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate should have a workspace root parent")
        .to_path_buf()
}

fn absolutize(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn temp_report_path(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis();
    env::temp_dir().join(format!("{millis}-{name}"))
}

fn print_report_summary(report: &TransformQualityReport) {
    let passed_checks = report
        .threshold_checks
        .iter()
        .filter(|check| check.passed)
        .count();
    eprintln!(
        "Transform quality gate: replay_passed={}, benchmark_checks={}/{}",
        report.replay_summary.passed,
        passed_checks,
        report.threshold_checks.len()
    );

    for check in report.threshold_checks.iter().filter(|check| !check.passed) {
        eprintln!(
            "benchmark regression: {}/{} -> {}",
            check.kind,
            check.scenario,
            check.failures.join("; ")
        );
    }
}
