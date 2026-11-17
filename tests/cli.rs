use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}\n{}", self.stdout, self.stderr))
    }
}

fn leankeel_in(dir: &Path, args: &[&str], stdin: &str) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_leankeel"))
        .args(args)
        .current_dir(dir)
        .env("HOME", std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn leankeel(args: &[&str]) -> Run {
    leankeel_in(Path::new("."), args, "")
}

fn check(source: &str) -> Value {
    leankeel(&["source", "check-source", "--source", source]).json()
}

fn fix(source: &str, extra: &[&str]) -> Value {
    let mut args = vec!["source", "fix-source", "--source", source];
    args.extend(extra);
    leankeel(&args).json()
}

fn rules(report: &Value) -> Vec<&str> {
    report["Diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["Rule"].as_str().unwrap())
        .collect()
}

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("leankeel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

const UNUSED_VARIABLE: &str = "def first (x y : Nat) : Nat :=\n  let z := 3\n  x + 1\n";
const UNUSED_SIMP_ARG: &str = "example (xs : List Nat) : xs ++ [] = xs := by\n  simp [List.append_nil, Nat.add_comm, Nat.mul_comm]\n";
const DEPRECATED: &str = "@[deprecated Nat.add_comm (since := \"2026-01-01\")]\ntheorem oldAddComm (a b : Nat) : a + b = b + a := Nat.add_comm a b\n\nexample (a b : Nat) : a + b = b + a := oldAddComm a b\n";
const SHADOWING_FIX: &str = "def pick (_y : Nat) (y : String) : Nat := _y\n";
const SIMP_QUESTION: &str = "example (xs : List Nat) : xs ++ [] = xs := by\n  simp?\n";

#[test]
fn clean_source_has_no_diagnostics() {
    let run = leankeel(&[
        "source",
        "check-source",
        "--source",
        "def double (n : Nat) : Nat := n + n\n",
    ]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let report = run.json();
    assert_eq!(report["Diagnostics"], serde_json::json!([]));
    assert_eq!(report["Summary"]["Warn"], 0);
}

#[test]
fn unused_variable_is_reported_with_its_fix() {
    let report = check(UNUSED_VARIABLE);
    assert_eq!(
        rules(&report),
        ["linter.unusedVariables", "linter.unusedVariables"]
    );
    let first = &report["Diagnostics"][0];
    assert_eq!(first["Level"], "warn");
    assert_eq!(first["Severity"], "warning");
    assert_eq!(first["Range"]["Start"]["Line"], 1);
    assert_eq!(first["Range"]["Start"]["Column"], 13);
    assert_eq!(first["Range"]["Start"]["Offset"], 13);
    assert_eq!(first["Fixes"][0]["NewText"], "_y");
    assert_eq!(first["Fixes"][0]["Range"]["End"]["Offset"], 14);
    assert_eq!(report["Summary"]["Fixable"], 2);
}

#[test]
fn unused_variable_is_fixed_and_the_result_is_clean() {
    let fixed = fix(UNUSED_VARIABLE, &[]);
    assert_eq!(fixed["Changed"], true);
    let source = fixed["Source"].as_str().unwrap();
    assert_eq!(
        source,
        "def first (x _y : Nat) : Nat :=\n  let _z := 3\n  x + 1\n"
    );
    assert_eq!(fixed["Applied"].as_array().unwrap().len(), 2);
    assert_eq!(fixed["Diagnostics"], serde_json::json!([]));
    assert_eq!(rules(&check(source)), Vec::<&str>::new());
    let again = fix(source, &[]);
    assert_eq!(again["Changed"], false);
    assert_eq!(again["Source"], source);
}

#[test]
fn overlapping_simp_fixes_are_applied_over_several_rounds() {
    let report = check(UNUSED_SIMP_ARG);
    assert_eq!(
        rules(&report),
        ["linter.unusedSimpArgs", "linter.unusedSimpArgs"]
    );
    let fixed = fix(UNUSED_SIMP_ARG, &[]);
    assert_eq!(
        fixed["Source"],
        "example (xs : List Nat) : xs ++ [] = xs := by\n  simp [List.append_nil]\n"
    );
    assert_eq!(fixed["Applied"].as_array().unwrap().len(), 2);
    assert_eq!(fixed["Diagnostics"], serde_json::json!([]));
}

#[test]
fn deprecated_name_is_replaced() {
    assert_eq!(rules(&check(DEPRECATED)), ["Lean.Linter.deprecatedAttr"]);
    let fixed = fix(DEPRECATED, &[]);
    assert!(
        fixed["Source"]
            .as_str()
            .unwrap()
            .ends_with("b + a := Nat.add_comm a b\n")
    );
    assert_eq!(fixed["Applied"][0]["Rule"], "Lean.Linter.deprecatedAttr");
    assert_eq!(fixed["Diagnostics"], serde_json::json!([]));
}

#[test]
fn a_fix_that_breaks_the_file_is_rejected() {
    let fixed = fix(SHADOWING_FIX, &[]);
    assert_eq!(fixed["Changed"], false);
    assert_eq!(fixed["Source"], SHADOWING_FIX);
    assert_eq!(fixed["Applied"], serde_json::json!([]));
    assert_eq!(fixed["Rejected"][0]["Rule"], "linter.unusedVariables");
    assert_eq!(fixed["Rejected"][0]["Title"], "Try this: _y");
    assert!(fixed["Rejected"][0]["Reason"].is_string());
    assert_eq!(rules(&fixed), ["linter.unusedVariables"]);
    assert_eq!(fixed["Summary"]["Rejected"], 1);
}

#[test]
fn a_breaking_fix_does_not_block_the_other_fixes() {
    let source = format!("{SHADOWING_FIX}\n{UNUSED_VARIABLE}");
    let fixed = fix(&source, &[]);
    let expected =
        format!("{SHADOWING_FIX}\ndef first (x _y : Nat) : Nat :=\n  let _z := 3\n  x + 1\n");
    assert_eq!(fixed["Source"], expected.as_str());
    assert_eq!(fixed["Applied"].as_array().unwrap().len(), 2);
    assert_eq!(fixed["Rejected"].as_array().unwrap().len(), 1);
    assert_eq!(rules(&fixed), ["linter.unusedVariables"]);
}

#[test]
fn try_this_is_reported_and_fixed_only_on_request() {
    let report = check(SIMP_QUESTION);
    assert_eq!(rules(&report), ["lean.tryThis"]);
    assert_eq!(
        report["Diagnostics"][0]["Fixes"][0]["NewText"],
        "simp only [List.append_nil]"
    );
    assert_eq!(fix(SIMP_QUESTION, &[])["Changed"], false);
    let fixed = fix(SIMP_QUESTION, &["--rules", "lean.tryThis"]);
    assert_eq!(
        fixed["Source"],
        "example (xs : List Nat) : xs ++ [] = xs := by\n  simp only [List.append_nil]\n"
    );
    assert_eq!(fixed["Diagnostics"], serde_json::json!([]));
}

#[test]
fn rules_limit_the_applied_fixes() {
    let source = format!("{DEPRECATED}\n{UNUSED_VARIABLE}");
    let fixed = fix(&source, &["--rules", "Lean.Linter.deprecatedAttr"]);
    assert_eq!(fixed["Applied"].as_array().unwrap().len(), 1);
    assert_eq!(
        rules(&fixed),
        ["linter.unusedVariables", "linter.unusedVariables"]
    );
}

#[test]
fn offsets_follow_multibyte_text() {
    let source = "-- 日本語のコメント\ndef «加算» (α β : Nat) : Nat :=\n  α + 1\n";
    let report = check(source);
    let start = &report["Diagnostics"][0]["Range"]["Start"];
    assert_eq!(start["Line"], 2);
    assert_eq!(start["Column"], 12);
    assert_eq!(start["Offset"], source.find('β').unwrap());
    let fixed = fix(source, &[]);
    assert_eq!(
        fixed["Source"],
        "-- 日本語のコメント\ndef «加算» (α _β : Nat) : Nat :=\n  α + 1\n"
    );
}

#[test]
fn sorry_is_reported_without_a_fix() {
    let source = "theorem pending : 1 + 1 = 2 := by\n  sorry\n";
    let report = check(source);
    assert_eq!(rules(&report), ["hasSorry"]);
    assert_eq!(report["Diagnostics"][0]["Fixes"], serde_json::json!([]));
    assert_eq!(fix(source, &[])["Changed"], false);
}

#[test]
fn an_error_is_denied_and_exits_254() {
    let run = leankeel(&[
        "source",
        "check-source",
        "--source",
        "def broken : Nat := \"text\"\n",
    ]);
    assert_eq!(run.code, 254);
    let report = run.json();
    assert_eq!(report["Diagnostics"][0]["Level"], "deny");
    assert_eq!(report["Diagnostics"][0]["Severity"], "error");
    assert_eq!(report["Summary"]["Deny"], 1);
}

#[test]
fn warnings_exit_zero() {
    let run = leankeel(&["source", "check-source", "--source", UNUSED_VARIABLE]);
    assert_eq!(run.code, 0);
}

#[test]
fn configuration_sets_levels_fix_rules_and_lean_options() {
    let dir = workspace("config");
    std::fs::write(
        dir.join("leankeel.toml"),
        "[rules]\n\"linter.unusedVariables\" = \"deny\"\n\"linter.missingDocs\" = \"allow\"\nhasSorry = \"allow\"\n\n[fix]\nrules = [\"linter.unusedSimpArgs\"]\n\n[lean-options]\n\"linter.missingDocs\" = true\n",
    )
    .unwrap();
    let source = format!(
        "{UNUSED_VARIABLE}\ntheorem pending : 1 + 1 = 2 := by\n  sorry\n\n{UNUSED_SIMP_ARG}"
    );
    std::fs::write(dir.join("Sample.lean"), &source).unwrap();
    let run = leankeel_in(
        &dir,
        &["source", "check-source", "--file", "Sample.lean"],
        "",
    );
    assert_eq!(run.code, 254, "{}", run.stderr);
    let report = run.json();
    assert_eq!(
        rules(&report),
        [
            "linter.unusedVariables",
            "linter.unusedVariables",
            "linter.unusedSimpArgs",
            "linter.unusedSimpArgs"
        ]
    );
    assert_eq!(report["Diagnostics"][0]["Level"], "deny");
    assert_eq!(report["Path"], "Sample.lean");

    let run = leankeel_in(
        &dir,
        &["source", "fix-source", "--file", "Sample.lean", "--write"],
        "",
    );
    assert_eq!(run.code, 254);
    let written = std::fs::read_to_string(dir.join("Sample.lean")).unwrap();
    assert!(written.contains("simp [List.append_nil]\n"));
    assert!(written.contains("(x y : Nat)"));

    std::fs::write(
        dir.join("leankeel.toml"),
        "[lean-options]\n\"linter.missingDocs\" = true\n",
    )
    .unwrap();
    let run = leankeel_in(
        &dir,
        &[
            "source",
            "check-source",
            "--source",
            "def documented : Nat := 1\n",
        ],
        "",
    );
    assert_eq!(rules(&run.json()), ["linter.missingDocs"]);

    std::fs::write(dir.join("leankeel.toml"), "[rule]\nhasSorry = \"deny\"\n").unwrap();
    let run = leankeel_in(
        &dir,
        &["source", "check-source", "--file", "Sample.lean"],
        "",
    );
    assert_eq!(run.code, 254);
    assert!(run.stderr.contains("leankeel.toml"), "{}", run.stderr);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stdin_and_text_output() {
    let run = leankeel_in(
        Path::new("."),
        &[
            "--output",
            "text",
            "source",
            "check-source",
            "--source",
            "file:///dev/stdin",
        ],
        UNUSED_SIMP_ARG,
    );
    assert_eq!(
        run.stdout,
        "<input>:2:25: warn [linter.unusedSimpArgs] This simp argument is unused: Nat.add_comm\n<input>:2:39: warn [linter.unusedSimpArgs] This simp argument is unused: Nat.mul_comm\n"
    );
    let run = leankeel_in(
        Path::new("."),
        &[
            "--output",
            "text",
            "source",
            "fix-source",
            "--source",
            "file:///dev/stdin",
        ],
        UNUSED_VARIABLE,
    );
    assert_eq!(
        run.stdout,
        "def first (x _y : Nat) : Nat :=\n  let _z := 3\n  x + 1\n"
    );
}

#[test]
fn lake_project_imports_options_and_paths() {
    let dir = workspace("project");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project"),
        &dir,
    );
    let run = leankeel_in(&dir, &["source", "check-source", "--paths", "."], "");
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    let report = run.json();
    let files = report["Files"].as_array().unwrap();
    let paths: Vec<_> = files.iter().map(|f| f["Path"].as_str().unwrap()).collect();
    assert_eq!(
        paths,
        ["./Keel/Basic.lean", "./Keel/Use.lean", "./Keel.lean"]
    );
    assert_eq!(rules(&files[0]), ["linter.missingDocs"]);
    assert_eq!(
        rules(&files[1]),
        ["linter.unusedVariables", "Lean.Linter.deprecatedAttr"]
    );
    assert_eq!(report["Summary"]["Warn"], 3);

    let run = leankeel_in(
        &dir,
        &["source", "fix-source", "--paths", "Keel", "--write"],
        "",
    );
    assert_eq!(run.code, 0, "{}\n{}", run.stdout, run.stderr);
    assert_eq!(run.json()["Summary"]["Applied"], 2);
    assert_eq!(
        std::fs::read_to_string(dir.join("Keel/Use.lean")).unwrap(),
        "import Keel.Basic\n\n/-- Uses the current name. -/\ndef useKeel (n _m : Nat) : Nat := Keel.draft n\n"
    );
    let run = leankeel_in(
        &dir,
        &[
            "--output",
            "table",
            "source",
            "check-source",
            "--paths",
            "Keel",
        ],
        "",
    );
    assert_eq!(
        run.stdout,
        "Deny\tWarn\tApplied\tRejected\tPath\n0\t1\t0\t0\tKeel/Basic.lean\n0\t0\t0\t0\tKeel/Use.lean\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_missing_file_is_an_input_error() {
    let run = leankeel(&["source", "check-source", "--file", "/nonexistent/A.lean"]);
    assert_eq!(run.code, 254);
    assert!(run.stderr.contains("/nonexistent/A.lean"));
}

#[test]
fn grammar_errors_exit_252() {
    assert_eq!(leankeel(&["source", "check-source"]).code, 252);
    assert_eq!(leankeel(&["source", "check-source", "A.lean"]).code, 252);
    let run = leankeel(&["source", "check-source", "--source", "", "--write"]);
    assert_eq!(run.code, 252);
    let run = leankeel(&[
        "source",
        "check-source",
        "--source",
        "",
        "--rules",
        "hasSorry",
    ]);
    assert_eq!(run.code, 252);
    assert_eq!(leankeel(&["--version"]).code, 0);
    assert_eq!(leankeel(&["source", "help"]).code, 0);
}

#[test]
fn rules_are_listed() {
    let report = leankeel(&["rule", "list-rules"]).json();
    let listed = report["Rules"].as_array().unwrap();
    let rule = listed
        .iter()
        .find(|r| r["Rule"] == "Lean.Linter.deprecatedAttr")
        .unwrap();
    assert_eq!(rule["LeanOption"], "linter.deprecated");
    assert_eq!(rule["DefaultLevel"], "warn");
    assert_eq!(rule["Fix"], "default");
}

#[test]
fn an_unknown_lean_option_is_an_input_error() {
    let dir = workspace("option");
    std::fs::write(
        dir.join("leankeel.toml"),
        "[lean-options]\n\"linter.noSuchLinter\" = true\n",
    )
    .unwrap();
    let run = leankeel_in(
        &dir,
        &["source", "check-source", "--source", "def one : Nat := 1\n"],
        "",
    );
    assert_eq!(run.code, 254, "{}", run.stdout);
    assert!(run.stderr.contains("linter.noSuchLinter"), "{}", run.stderr);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_import_is_a_denied_diagnostic() {
    let run = leankeel(&[
        "source",
        "check-source",
        "--source",
        "import No.Such.Module\n",
    ]);
    assert_eq!(run.code, 254);
    let report = run.json();
    assert_eq!(report["Diagnostics"][0]["Severity"], "error");
    assert_eq!(report["Diagnostics"][0]["Level"], "deny");
}

#[test]
fn readme_example() {
    let dir = workspace("readme");
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Sample.lean");
    std::fs::copy(sample, dir.join("Sample.lean")).unwrap();
    let args = [
        "--output",
        "text",
        "source",
        "check-source",
        "--file",
        "Sample.lean",
    ];
    let run = leankeel_in(&dir, &args, "");
    assert_eq!(
        run.stdout,
        "Sample.lean:1:17: warn [linter.unusedVariables] Variable name `y` is not explicitly referenced.\nSample.lean:7:39: warn [Lean.Linter.deprecatedAttr] `oldAddComm` has been deprecated: Use `Nat.add_comm` instead\nSample.lean:10:25: warn [linter.unusedSimpArgs] This simp argument is unused: Nat.add_comm\n"
    );
    let args = ["source", "fix-source", "--file", "Sample.lean", "--write"];
    let run = leankeel_in(&dir, &args, "");
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.json()["Summary"]["Applied"], 3);
    let fixed = std::fs::read_to_string(dir.join("Sample.lean")).unwrap();
    assert!(fixed.contains("(x _y : Nat)"));
    assert!(fixed.contains(":= Nat.add_comm a b\n\nexample (xs"));
    assert!(fixed.ends_with("  simp [List.append_nil]\n"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unnecessary_simpa_becomes_simp() {
    let source = "example (xs : List Nat) : xs ++ [] = xs := by\n  simpa\n";
    assert_eq!(rules(&check(source)), ["linter.unnecessarySimpa"]);
    let fixed = fix(source, &[]);
    assert_eq!(
        fixed["Source"],
        "example (xs : List Nat) : xs ++ [] = xs := by\n  simp\n"
    );
    assert_eq!(fixed["Diagnostics"], serde_json::json!([]));
    let needed = "example (a b : Nat) (h : a = b) : a + 0 = b := by\n  simpa using h\n";
    assert_eq!(rules(&check(needed)), Vec::<&str>::new());
}
