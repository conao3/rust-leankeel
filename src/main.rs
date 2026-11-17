use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use leankeel::{Change, Diagnostic, Failure, FixSupport, Level, Policy, Session};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, ValueEnum)]
enum Output {
    Json,
    Text,
    Table,
}

#[derive(Parser)]
#[command(
    version,
    about = "Lint Lean 4 source and apply Lean's suggested fixes, verifying each change"
)]
struct Cli {
    #[arg(long, global = true, value_enum, default_value = "json")]
    output: Output,
    #[arg(long, global = true, overrides_with = "no_debug")]
    debug: bool,
    #[arg(long, global = true, overrides_with = "debug")]
    no_debug: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check and fix Lean source.
    Source {
        #[command(subcommand)]
        command: Source,
    },
    /// Inspect the rules.
    Rule {
        #[command(subcommand)]
        command: RuleCommand,
    },
}

#[derive(Subcommand)]
enum Source {
    /// Report the diagnostics of the input (exit 254 when one is denied).
    CheckSource(Options),
    /// Apply the fixes of the selected rules and keep each change that verifies.
    FixSource(Options),
}

#[derive(Subcommand)]
enum RuleCommand {
    /// List the rules with a built-in level or fix.
    ListRules,
}

#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct Input {
    /// A UTF-8 Lean file.
    #[arg(long)]
    file: Option<PathBuf>,
    /// Literal source text, or `file://PATH` (`file:///dev/stdin` reads stdin).
    #[arg(long, allow_hyphen_values = true)]
    source: Option<String>,
    /// Files and directories; directories are searched for `.lean` files.
    #[arg(long, num_args = 1..)]
    paths: Option<Vec<PathBuf>>,
}

#[derive(clap::Args)]
struct Options {
    #[command(flatten)]
    input: Input,
    /// Write fixed text back to --file or --paths.
    #[arg(long, overrides_with = "no_write")]
    write: bool,
    #[arg(long, overrides_with = "write")]
    no_write: bool,
    /// Rules whose fixes are applied (default: `fix.rules` in leankeel.toml, otherwise the
    /// rules that `rule list-rules` marks `default`).
    #[arg(long, num_args = 1..)]
    rules: Option<Vec<String>>,
    /// Lake project to load imports from (default: found upward from the input).
    #[arg(long)]
    project_dir: Option<PathBuf>,
    /// Configuration file (default: leankeel.toml found upward from the input).
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct FixConfig {
    rules: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Config {
    #[serde(default)]
    rules: BTreeMap<String, Level>,
    #[serde(default)]
    fix: FixConfig,
    #[serde(default)]
    lean_options: BTreeMap<String, toml::Value>,
}

struct Report {
    path: Option<PathBuf>,
    original: String,
    source: String,
    applied: Vec<Change>,
    rejected: Vec<Change>,
    diagnostics: Vec<Diagnostic>,
}

fn find_upward(start: &Path, names: &[&str]) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start).ok()?;
    start
        .ancestors()
        .find(|p| names.iter().any(|n| p.join(n).is_file()))
        .map(Path::to_owned)
}

fn load_config(opts: &Options, start: &Path) -> Result<Config, Failure> {
    let path = match &opts.config {
        Some(path) => path.clone(),
        None => match find_upward(start, &["leankeel.toml"]) {
            Some(dir) => dir.join("leankeel.toml"),
            None => return Ok(Config::default()),
        },
    };
    let invalid = |e: &dyn std::fmt::Display| Failure::Input(format!("{}: {e}", path.display()));
    let text = std::fs::read_to_string(&path).map_err(|e| invalid(&e))?;
    toml::from_str(&text).map_err(|e| invalid(&e))
}

fn lean_options(config: &Config) -> Result<serde_json::Map<String, Value>, Failure> {
    config
        .lean_options
        .iter()
        .map(|(name, value)| {
            let value = match value {
                toml::Value::Boolean(b) => json!(b),
                toml::Value::String(s) => json!(s),
                toml::Value::Integer(n) if *n >= 0 => json!(n),
                _ => {
                    return Err(Failure::Input(format!(
                        "lean-options.{name}: expected a boolean, a string or a natural number"
                    )));
                }
            };
            Ok((name.clone(), value))
        })
        .collect()
}

fn process(
    opts: &Options,
    fix: bool,
    source: String,
    path: Option<&Path>,
) -> Result<Report, Failure> {
    let start = path
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let config = load_config(opts, start)?;
    let project = opts
        .project_dir
        .clone()
        .or_else(|| find_upward(start, &["lakefile.toml", "lakefile.lean"]));
    let session = Session::new(path, project, lean_options(&config)?)?;
    let fix_rules = opts.rules.clone().or(config.fix.rules);
    let policy = Policy::new(config.rules.into_iter().collect(), fix_rules);
    let path = path.map(Path::to_owned);
    if fix {
        let fixed = leankeel::fix(&session, &source, &policy)?;
        return Ok(Report {
            path,
            original: source,
            source: fixed.source,
            applied: fixed.applied,
            rejected: fixed.rejected,
            diagnostics: fixed.diagnostics,
        });
    }
    Ok(Report {
        path,
        diagnostics: session.analyze(&source, &policy)?,
        original: source.clone(),
        source,
        applied: Vec::new(),
        rejected: Vec::new(),
    })
}

fn process_file(opts: &Options, fix: bool, path: &Path) -> Result<Report, Failure> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| Failure::Input(format!("{}: {e}", path.display())))?;
    let report = process(opts, fix, source, Some(path))?;
    if opts.write && report.source != report.original {
        std::fs::write(path, &report.source)
            .map_err(|e| Failure::Input(format!("{}: {e}", path.display())))?;
    }
    Ok(report)
}

fn lean_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, Failure> {
    fn walk(path: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type()?;
            let name = entry.file_name();
            if kind.is_symlink() || name.to_string_lossy().starts_with('.') {
                continue;
            }
            if kind.is_dir() {
                walk(&entry.path(), files)?;
            } else if entry.path().extension().is_some_and(|e| e == "lean") {
                files.push(entry.path());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for path in paths {
        let found = if path.is_dir() {
            walk(path, &mut files)
        } else {
            std::fs::metadata(path).map(|_| files.push(path.clone()))
        };
        found.map_err(|e| Failure::Input(format!("{}: {e}", path.display())))?;
    }
    Ok(files)
}

fn reported(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.level != Level::Allow)
        .collect()
}

fn summary<'a>(reports: impl Iterator<Item = &'a Report>) -> Value {
    let (mut deny, mut warn, mut fixable, mut applied, mut rejected) = (0, 0, 0, 0, 0);
    for report in reports {
        for diagnostic in reported(&report.diagnostics) {
            match diagnostic.level {
                Level::Deny => deny += 1,
                _ => warn += 1,
            }
            fixable += usize::from(!diagnostic.fixes.is_empty());
        }
        applied += report.applied.len();
        rejected += report.rejected.len();
    }
    json!({"Deny": deny, "Warn": warn, "Fixable": fixable, "Applied": applied, "Rejected": rejected})
}

fn report_json(report: &Report, fix: bool, with_source: bool) -> Value {
    let mut value = json!({ "Diagnostics": reported(&report.diagnostics) });
    if let Some(path) = &report.path {
        value["Path"] = json!(path.display().to_string());
    }
    if fix {
        value["Changed"] = json!(report.source != report.original);
        value["Applied"] = json!(report.applied);
        value["Rejected"] = json!(report.rejected);
        if with_source {
            value["Source"] = json!(report.source);
        }
    }
    value
}

fn headline(text: &str) -> String {
    let paragraph = text.split("\n\n").next().unwrap_or_default();
    paragraph.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn diagnostic_lines(report: &Report) -> Vec<String> {
    let name = report
        .path
        .as_ref()
        .map_or("<input>".into(), |p| p.display().to_string());
    reported(&report.diagnostics)
        .iter()
        .map(|d| {
            format!(
                "{name}:{}:{}: {} [{}] {}",
                d.range.start.line,
                d.range.start.column,
                d.level.name(),
                d.rule,
                headline(&d.text)
            )
        })
        .collect()
}

fn print_reports(output: Output, fix: bool, reports: &[Report], errors: &[(PathBuf, String)]) {
    match output {
        Output::Json => {
            let mut files: Vec<Value> =
                reports.iter().map(|r| report_json(r, fix, false)).collect();
            files.extend(
                errors.iter().map(
                    |(path, error)| json!({"Path": path.display().to_string(), "Error": error}),
                ),
            );
            println!(
                "{}",
                json!({"Files": files, "Summary": summary(reports.iter())})
            );
        }
        Output::Text => {
            for report in reports {
                for line in diagnostic_lines(report) {
                    println!("{line}");
                }
            }
        }
        Output::Table => {
            println!("Deny\tWarn\tApplied\tRejected\tPath");
            for report in reports {
                let shown = reported(&report.diagnostics);
                let deny = shown.iter().filter(|d| d.level == Level::Deny).count();
                println!(
                    "{deny}\t{}\t{}\t{}\t{}",
                    shown.len() - deny,
                    report.applied.len(),
                    report.rejected.len(),
                    report
                        .path
                        .as_ref()
                        .map_or(String::new(), |p| p.display().to_string())
                );
            }
        }
    }
    for (path, error) in errors {
        eprintln!("{}: {error}", path.display());
    }
}

fn print_report(output: Output, fix: bool, report: &Report) -> Result<(), Failure> {
    match output {
        Output::Json => {
            let mut value = report_json(report, fix, true);
            value["Summary"] = summary(std::iter::once(report));
            println!("{value}");
        }
        Output::Text if fix => {
            std::io::stdout()
                .write_all(report.source.as_bytes())
                .map_err(|e| Failure::Internal(e.to_string()))?;
            for line in diagnostic_lines(report) {
                eprintln!("{line}");
            }
        }
        Output::Text => {
            for line in diagnostic_lines(report) {
                println!("{line}");
            }
        }
        Output::Table => {
            println!("Level\tRule\tLine\tColumn\tFixes\tMessage");
            for d in reported(&report.diagnostics) {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    d.level.name(),
                    d.rule,
                    d.range.start.line,
                    d.range.start.column,
                    d.fixes.len(),
                    headline(&d.text)
                );
            }
        }
    }
    Ok(())
}

fn list_rules(output: Output) {
    let fix = |support: FixSupport| match support {
        FixSupport::Default => "default",
        FixSupport::OptIn => "opt-in",
        FixSupport::None => "none",
    };
    match output {
        Output::Json => {
            let rules: Vec<Value> = leankeel::RULES
                .iter()
                .map(|rule| {
                    json!({
                        "Rule": rule.id,
                        "LeanOption": rule.option,
                        "DefaultLevel": rule.default,
                        "Fix": fix(rule.fix),
                        "Summary": rule.summary,
                    })
                })
                .collect();
            println!("{}", json!({ "Rules": rules }));
        }
        Output::Text | Output::Table => {
            if matches!(output, Output::Table) {
                println!("Rule\tDefaultLevel\tFix\tLeanOption\tSummary");
            }
            for rule in leankeel::RULES {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    rule.id,
                    rule.default.name(),
                    fix(rule.fix),
                    rule.option.unwrap_or("-"),
                    rule.summary
                );
            }
        }
    }
}

fn read_source(value: &str) -> Result<String, Failure> {
    if value == "file:///dev/stdin" {
        let mut data = String::new();
        std::io::stdin()
            .read_to_string(&mut data)
            .map_err(|e| Failure::Input(e.to_string()))?;
        Ok(data)
    } else if let Some(path) = value.strip_prefix("file://") {
        std::fs::read_to_string(path).map_err(|e| Failure::Input(format!("{path}: {e}")))
    } else {
        Ok(value.to_owned())
    }
}

fn run(cli: Cli) -> Result<i32, Failure> {
    leankeel::DEBUG.store(cli.debug, std::sync::atomic::Ordering::Relaxed);
    let command = match cli.command {
        Commands::Rule {
            command: RuleCommand::ListRules,
        } => {
            list_rules(cli.output);
            return Ok(0);
        }
        Commands::Source { command } => command,
    };
    let fix = matches!(command, Source::FixSource(_));
    let (Source::CheckSource(opts) | Source::FixSource(opts)) = command;
    if opts.write && (!fix || opts.input.source.is_some()) {
        return Err(Failure::Usage(
            "--write requires fix-source with --file or --paths".into(),
        ));
    }
    if opts.rules.is_some() && !fix {
        return Err(Failure::Usage("--rules requires fix-source".into()));
    }
    if let Some(paths) = &opts.input.paths {
        let mut reports = Vec::new();
        let mut errors = Vec::new();
        for path in lean_files(paths)? {
            match process_file(&opts, fix, &path) {
                Ok(report) => reports.push(report),
                Err(failure) => errors.push((path, failure.message().to_owned())),
            }
        }
        print_reports(cli.output, fix, &reports, &errors);
        let denied = reports
            .iter()
            .any(|r| leankeel::exit_code(&r.diagnostics) != 0);
        return Ok(if denied || !errors.is_empty() { 254 } else { 0 });
    }
    let report = match &opts.input.file {
        Some(path) => process_file(&opts, fix, path)?,
        None => {
            let source = read_source(opts.input.source.as_deref().unwrap_or_default())?;
            process(&opts, fix, source, None)?
        }
    };
    print_report(cli.output, fix, &report)?;
    Ok(leankeel::exit_code(&report.diagnostics))
}

fn main() {
    let cli = Cli::try_parse().unwrap_or_else(|e| {
        let success = matches!(
            e.kind(),
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
        );
        let _ = e.print();
        std::process::exit(if success { 0 } else { 252 });
    });
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(failure) => {
            eprintln!("{}", failure.message());
            std::process::exit(match failure {
                Failure::Usage(_) => 252,
                Failure::Input(_) => 254,
                Failure::Internal(_) => 255,
            });
        }
    }
}
