pub mod adapter;
pub mod plan;
pub mod policy;
pub mod verdict;

use std::path::{Path, PathBuf};

use leanfran::Nat;
use serde::{Deserialize, Serialize};

pub static DEBUG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn trace(message: impl FnOnce() -> String) {
    if DEBUG.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("leankeel: {}", message());
    }
}

#[derive(Debug)]
pub enum Failure {
    Usage(String),
    Input(String),
    Internal(String),
}

impl Failure {
    pub fn message(&self) -> &str {
        match self {
            Failure::Usage(message) | Failure::Input(message) | Failure::Internal(message) => {
                message
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Information,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

impl Level {
    fn to_model(self) -> policy::model::Level {
        match self {
            Level::Allow => policy::model::Level::Allow,
            Level::Warn => policy::model::Level::Warn,
            Level::Deny => policy::model::Level::Deny,
        }
    }

    fn of_model(level: policy::model::Level) -> Level {
        match level {
            policy::model::Level::Allow => Level::Allow,
            policy::model::Level::Warn => Level::Warn,
            policy::model::Level::Deny => Level::Deny,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Allow => "allow",
            Level::Warn => "warn",
            Level::Deny => "deny",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Fix {
    pub title: String,
    pub range: Range,
    pub new_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Diagnostic {
    pub rule: String,
    pub level: Level,
    pub severity: Severity,
    pub range: Range,
    pub message: String,
    #[serde(skip)]
    pub text: String,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    fn key(&self) -> String {
        format!("{:?}\u{0}{}\u{0}{}", self.severity, self.rule, self.text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixSupport {
    Default,
    OptIn,
    None,
}

pub struct Rule {
    pub id: &'static str,
    pub option: Option<&'static str>,
    pub default: Level,
    pub fix: FixSupport,
    pub summary: &'static str,
}

pub const TRY_THIS: &str = "lean.tryThis";

pub const RULES: &[Rule] = &[
    Rule {
        id: "linter.unusedVariables",
        option: Some("linter.unusedVariables"),
        default: Level::Warn,
        fix: FixSupport::Default,
        summary: "A variable is bound and never referenced; the fix prefixes its name with `_`.",
    },
    Rule {
        id: "linter.unusedSimpArgs",
        option: Some("linter.unusedSimpArgs"),
        default: Level::Warn,
        fix: FixSupport::Default,
        summary: "A simp argument is never used; the fix removes it from the argument list.",
    },
    Rule {
        id: "linter.unnecessarySimpa",
        option: Some("linter.unnecessarySimpa"),
        default: Level::Warn,
        fix: FixSupport::Default,
        summary: "`simpa` closes a goal that `simp` already closes; the fix writes `simp`.",
    },
    Rule {
        id: "Lean.Linter.deprecatedAttr",
        option: Some("linter.deprecated"),
        default: Level::Warn,
        fix: FixSupport::Default,
        summary: "A deprecated declaration is used; the fix inserts the declared replacement.",
    },
    Rule {
        id: "hasSorry",
        option: None,
        default: Level::Warn,
        fix: FixSupport::None,
        summary: "A declaration depends on `sorry`.",
    },
    Rule {
        id: TRY_THIS,
        option: None,
        default: Level::Warn,
        fix: FixSupport::OptIn,
        summary: "A tactic such as `simp?` or `exact?` reports a replacement for itself.",
    },
    Rule {
        id: "lean.error",
        option: None,
        default: Level::Deny,
        fix: FixSupport::None,
        summary: "An error without a message kind. Errors are always denied.",
    },
    Rule {
        id: "lean.warning",
        option: None,
        default: Level::Warn,
        fix: FixSupport::None,
        summary: "A warning without a message kind.",
    },
    Rule {
        id: "lean.info",
        option: None,
        default: Level::Allow,
        fix: FixSupport::None,
        summary: "An informational message without a message kind.",
    },
];

pub struct Policy {
    pub settings: Vec<(String, Level)>,
    pub fix_rules: Vec<String>,
}

impl Policy {
    pub fn new(settings: Vec<(String, Level)>, fix_rules: Option<Vec<String>>) -> Policy {
        let defaults = RULES.iter().map(|rule| (rule.id.to_owned(), rule.default));
        Policy {
            settings: settings.into_iter().chain(defaults).collect(),
            fix_rules: fix_rules.unwrap_or_else(|| {
                RULES
                    .iter()
                    .filter(|rule| rule.fix == FixSupport::Default)
                    .map(|rule| rule.id.to_owned())
                    .collect()
            }),
        }
    }

    fn level(&self, rule: &str, severity: Severity) -> Level {
        let settings = self
            .settings
            .iter()
            .map(|(name, level)| (name.clone(), level.to_model()))
            .collect();
        let severity = match severity {
            Severity::Information => policy::model::Severity::Information,
            Severity::Warning => policy::model::Severity::Warning,
            Severity::Error => policy::model::Severity::Error,
        };
        Level::of_model(policy::model::level(settings, rule, severity))
    }

    fn fixes(&self, diagnostic: &Diagnostic) -> bool {
        let selected = self.fix_rules.contains(&diagnostic.rule);
        policy::model::fixable(diagnostic.level.to_model(), selected)
    }
}

pub fn exit_code(diagnostics: &[Diagnostic]) -> i32 {
    let levels = diagnostics.iter().map(|d| d.level.to_model()).collect();
    index(policy::model::exit_code(levels)) as i32
}

fn nat(value: usize) -> Nat {
    Nat::from(value as u64)
}

fn index(value: Nat) -> usize {
    u64::try_from(value).expect("an offset fits in 64 bits") as usize
}

fn span(start: usize, stop: usize) -> plan::model::Span {
    plan::model::Span {
        start: nat(start),
        stop: nat(stop),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub stop: usize,
    pub new_text: String,
}

pub fn apply(source: &str, edits: &[Edit]) -> Result<(String, Vec<usize>), Failure> {
    let mut order: Vec<usize> = (0..edits.len()).collect();
    order.sort_by_key(|&i| (edits[i].start, edits[i].stop));
    let spans = || {
        order
            .iter()
            .map(|&i| span(edits[i].start, edits[i].stop))
            .collect()
    };
    let mut result = String::with_capacity(source.len());
    for piece in plan::model::pieces(nat(source.len()), spans()) {
        match piece {
            plan::model::Piece::Keep { start, stop } => {
                let kept = source.get(index(start)..index(stop)).ok_or_else(|| {
                    Failure::Internal("an edit does not fall on a character boundary".into())
                })?;
                result.push_str(kept);
            }
            plan::model::Piece::Insert { index: i } => {
                result.push_str(&edits[order[index(i)]].new_text)
            }
        }
    }
    let applied = plan::model::chosen(nat(source.len()), spans())
        .into_iter()
        .map(|i| order[index(i)])
        .collect();
    Ok((result, applied))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawFix {
    title: String,
    new_text: String,
    start: Position,
    end: Position,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawMessage {
    kind: Option<String>,
    severity: Severity,
    start: Position,
    end: Position,
    message: String,
    text: String,
    fixes: Vec<RawFix>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Raw {
    #[serde(default)]
    messages: Vec<RawMessage>,
    error: Option<String>,
}

pub struct Session {
    adapter: adapter::Adapter,
    file_name: String,
}

impl Session {
    pub fn new(
        file: Option<&Path>,
        project: Option<PathBuf>,
        options: serde_json::Map<String, serde_json::Value>,
    ) -> Result<Session, Failure> {
        let file_name = file.map_or("<input>".into(), |path| path.display().to_string());
        Ok(Session {
            adapter: adapter::Adapter::new(file, project, options)?,
            file_name,
        })
    }

    pub fn analyze(&self, source: &str, policy: &Policy) -> Result<Vec<Diagnostic>, Failure> {
        let output = self.adapter.run(&self.file_name, source)?;
        let raw: Raw = serde_json::from_str(&output)
            .map_err(|e| Failure::Internal(format!("invalid adapter output: {e}")))?;
        if let Some(error) = raw.error {
            return Err(Failure::Input(error));
        }
        let mut diagnostics: Vec<Diagnostic> = raw
            .messages
            .into_iter()
            .map(|message| {
                let rule = match (message.kind, message.severity, message.fixes.is_empty()) {
                    (Some(kind), _, _) => kind,
                    (None, Severity::Information, false) => TRY_THIS.to_owned(),
                    (None, Severity::Information, true) => "lean.info".to_owned(),
                    (None, Severity::Warning, _) => "lean.warning".to_owned(),
                    (None, Severity::Error, _) => "lean.error".to_owned(),
                };
                Diagnostic {
                    level: policy.level(&rule, message.severity),
                    rule,
                    severity: message.severity,
                    range: Range {
                        start: message.start,
                        end: message.end,
                    },
                    message: message.message,
                    text: message.text,
                    fixes: message
                        .fixes
                        .into_iter()
                        .map(|fix| Fix {
                            title: fix.title,
                            range: Range {
                                start: fix.start,
                                end: fix.end,
                            },
                            new_text: fix.new_text,
                        })
                        .collect(),
                }
            })
            .collect();
        diagnostics.sort_by(|a, b| {
            let key = |d: &Diagnostic| (d.range.start.offset, d.range.end.offset);
            key(a).cmp(&key(b)).then_with(|| a.rule.cmp(&b.rule))
        });
        Ok(diagnostics)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Change {
    pub rule: String,
    pub title: String,
    pub range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip)]
    new_text: String,
}

impl Change {
    fn edit(&self) -> Edit {
        Edit {
            start: self.range.start.offset,
            stop: self.range.end.offset,
            new_text: self.new_text.clone(),
        }
    }
}

pub struct Fixed {
    pub source: String,
    pub applied: Vec<Change>,
    pub rejected: Vec<Change>,
    pub diagnostics: Vec<Diagnostic>,
}

fn candidates(diagnostics: &[Diagnostic], policy: &Policy, rejected: &[Change]) -> Vec<Change> {
    let mut found: Vec<Change> = Vec::new();
    for diagnostic in diagnostics.iter().filter(|d| policy.fixes(d)) {
        let Some(fix) = diagnostic.fixes.first() else {
            continue;
        };
        let change = Change {
            rule: diagnostic.rule.clone(),
            title: fix.title.clone(),
            range: fix.range,
            reason: None,
            new_text: fix.new_text.clone(),
        };
        let same = |other: &Change| other.edit() == change.edit();
        if !rejected.iter().any(same) && !found.iter().any(same) {
            found.push(change);
        }
    }
    found
}

fn accepted(before: &[Diagnostic], after: &[Diagnostic]) -> bool {
    let keys = |diagnostics: &[Diagnostic]| diagnostics.iter().map(Diagnostic::key).collect();
    verdict::model::accept(keys(before), keys(after))
}

fn position_at(source: &str, offset: usize) -> Position {
    let before = &source[..offset];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    Position {
        line: before.matches('\n').count() + 1,
        column: before[line_start..].chars().count(),
        offset,
    }
}

fn follow(rejected: &mut Vec<Change>, applied: &[&Change], source: &str) {
    let mut edits: Vec<Edit> = applied.iter().map(|change| change.edit()).collect();
    edits.sort_by_key(|edit| (edit.start, edit.stop));
    let edits = || {
        edits
            .iter()
            .map(|edit| (span(edit.start, edit.stop), nat(edit.new_text.len())))
            .collect()
    };
    rejected.retain_mut(|change| {
        let current = span(change.range.start.offset, change.range.end.offset);
        let Some(moved) = plan::model::shift(current, edits()) else {
            return false;
        };
        change.range = Range {
            start: position_at(source, index(moved.start)),
            end: position_at(source, index(moved.stop)),
        };
        true
    });
}

const REJECTION: &str =
    "the file gains a diagnostic or keeps as many as before when this fix is applied";

pub fn fix(session: &Session, source: &str, policy: &Policy) -> Result<Fixed, Failure> {
    let mut current = source.to_owned();
    let mut diagnostics = session.analyze(&current, policy)?;
    let mut applied: Vec<Change> = Vec::new();
    let mut rejected: Vec<Change> = Vec::new();
    loop {
        let pending = candidates(&diagnostics, policy, &rejected);
        if pending.is_empty() {
            break;
        }
        let edits: Vec<Edit> = pending.iter().map(Change::edit).collect();
        let (next, chosen) = apply(&current, &edits)?;
        let chosen: Vec<&Change> = chosen.into_iter().map(|i| &pending[i]).collect();
        let after = session.analyze(&next, policy)?;
        if accepted(&diagnostics, &after) {
            trace(|| format!("accepted {} fixes together", chosen.len()));
            follow(&mut rejected, &chosen, &next);
            applied.extend(chosen.into_iter().cloned());
            current = next;
            diagnostics = after;
            continue;
        }
        trace(|| format!("{} fixes fail together; trying each", chosen.len()));
        let mut progressed = false;
        for change in chosen {
            let (next, _) = apply(&current, &[change.edit()])?;
            let after = session.analyze(&next, policy)?;
            if accepted(&diagnostics, &after) {
                trace(|| format!("accepted {}", change.title));
                follow(&mut rejected, &[change], &next);
                applied.push(change.clone());
                current = next;
                diagnostics = after;
                progressed = true;
                break;
            }
            trace(|| format!("rejected {}", change.title));
            rejected.push(Change {
                reason: Some(REJECTION.into()),
                ..change.clone()
            });
        }
        if !progressed {
            break;
        }
    }
    Ok(Fixed {
        source: current,
        applied,
        rejected,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(start: usize, stop: usize, new_text: &str) -> Edit {
        Edit {
            start,
            stop,
            new_text: new_text.into(),
        }
    }

    fn change(start: usize, stop: usize, new_text: &str) -> Change {
        let at = |offset| Position {
            line: 1,
            column: offset,
            offset,
        };
        Change {
            rule: "rule".into(),
            title: "title".into(),
            range: Range {
                start: at(start),
                end: at(stop),
            },
            reason: None,
            new_text: new_text.into(),
        }
    }

    #[test]
    fn applies_disjoint_edits_in_source_order() {
        let edits = [edit(6, 11, "keel"), edit(0, 5, "lean")];
        let (text, applied) = apply("hello world", &edits).unwrap();
        assert_eq!(text, "lean keel");
        assert_eq!(applied, [1, 0]);
    }

    #[test]
    fn skips_an_edit_that_overlaps_an_earlier_one() {
        let edits = [edit(0, 5, "a"), edit(3, 8, "b"), edit(8, 11, "c")];
        let (text, applied) = apply("hello world", &edits).unwrap();
        assert_eq!(text, "a woc");
        assert_eq!(applied, [0, 2]);
    }

    #[test]
    fn skips_an_edit_outside_the_text() {
        let (text, applied) = apply("abc", &[edit(2, 9, "x"), edit(2, 1, "y")]).unwrap();
        assert_eq!(text, "abc");
        assert!(applied.is_empty());
    }

    #[test]
    fn no_edits_keep_the_text() {
        assert_eq!(apply("abc", &[]).unwrap().0, "abc");
    }

    #[test]
    fn inserts_at_a_point() {
        let (text, applied) = apply("ab", &[edit(1, 1, "-")]).unwrap();
        assert_eq!(text, "a-b");
        assert_eq!(applied, [0]);
    }

    #[test]
    fn reports_an_edit_inside_a_character() {
        assert!(matches!(
            apply("é", &[edit(1, 1, "x")]),
            Err(Failure::Internal(_))
        ));
    }

    #[test]
    fn rejected_fixes_follow_the_applied_edits() {
        let source = "ab\ncdefgh";
        let (next, _) = apply(source, &[edit(0, 1, "xyz"), edit(4, 5, "")]).unwrap();
        assert_eq!(next, "xyzb\ncefgh");
        let mut rejected = vec![change(1, 2, "B"), change(3, 4, "C"), change(6, 8, "G")];
        let applied = [change(0, 1, "xyz"), change(4, 5, "")];
        follow(&mut rejected, &applied.iter().collect::<Vec<_>>(), &next);
        let spans: Vec<_> = rejected
            .iter()
            .map(|c| (c.range.start.offset, c.range.end.offset))
            .collect();
        assert_eq!(spans, [(3, 4), (5, 6), (7, 9)]);
        assert_eq!(rejected[2].range.start.line, 2);
        assert_eq!(rejected[2].range.start.column, 2);
        assert_eq!(&next[7..9], "fg");
    }

    #[test]
    fn a_rejected_fix_touched_by_an_applied_edit_is_forgotten() {
        let mut rejected = vec![change(2, 6, "x"), change(8, 9, "y")];
        let applied = [change(4, 5, "")];
        follow(
            &mut rejected,
            &applied.iter().collect::<Vec<_>>(),
            "0123567890",
        );
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].range.start.offset, 7);
    }

    #[test]
    fn a_fix_is_kept_only_when_diagnostics_shrink_without_new_ones() {
        let keys = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(verdict::model::accept(keys(&["a", "b"]), keys(&["b"])));
        assert!(!verdict::model::accept(keys(&["a", "b"]), keys(&["c"])));
        assert!(!verdict::model::accept(keys(&["a"]), keys(&["a"])));
        assert!(!verdict::model::accept(
            keys(&["a", "b"]),
            keys(&["b", "b"])
        ));
    }

    #[test]
    fn errors_are_denied_whatever_the_settings_say() {
        let policy = Policy::new(vec![("lean.error".into(), Level::Allow)], None);
        assert_eq!(policy.level("lean.error", Severity::Error), Level::Deny);
    }

    #[test]
    fn settings_precede_the_defaults() {
        let policy = Policy::new(vec![("hasSorry".into(), Level::Deny)], None);
        assert_eq!(policy.level("hasSorry", Severity::Warning), Level::Deny);
        assert_eq!(policy.level(TRY_THIS, Severity::Information), Level::Warn);
        assert_eq!(
            policy.level("lean.info", Severity::Information),
            Level::Allow
        );
        assert_eq!(policy.level("some.linter", Severity::Warning), Level::Warn);
    }
}
