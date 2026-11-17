# Leankeel

Leankeel lints Lean 4 source and applies the fixes that Lean itself suggests. Lean and its
linters already say what to change: an unused variable comes with `_y`, a deprecated name with
its replacement, an unused simp argument with the shorter argument list. Leankeel collects
those suggestions, applies them from the command line, re-elaborates the file after every
change, and keeps a change only when the file has fewer diagnostics and no new one.

```console
$ leankeel --output text source check-source --file Sample.lean
Sample.lean:1:17: warn [linter.unusedVariables] Variable name `y` is not explicitly referenced.
Sample.lean:7:39: warn [Lean.Linter.deprecatedAttr] `oldAddComm` has been deprecated: Use `Nat.add_comm` instead
Sample.lean:10:25: warn [linter.unusedSimpArgs] This simp argument is unused: Nat.add_comm

$ leankeel source fix-source --file Sample.lean --write
```

The sample is `tests/fixtures/Sample.lean`. Leankeel targets Lean 4.35.0.

## Run

```console
nix run github:conao3/rust-leankeel -- source check-source --file Sample.lean
```

`nix build` produces `result/bin/leankeel` together with the Lean adapter it calls. `lake` and
`lean` come from the package; a file inside a Lake project is elaborated with that project's
imports and `leanOptions`.

## Commands

The command line follows the AWS CLI grammar: named parameters only, JSON by default.

```text
leankeel [--output json|text|table] [--debug] source check-source <input>
leankeel [--output json|text|table] [--debug] source fix-source <input> [--write] [--rules RULE...]
leankeel [--output json|text|table] rule list-rules
```

The input is one of:

- `--file PATH`: a Lean file.
- `--source TEXT`: literal source, or `file://PATH`; `file:///dev/stdin` reads standard input.
- `--paths PATH...`: files and directories. Directories are searched for `.lean` files, skipping
  names that start with `.` and symbolic links.

`--project-dir DIR` names the Lake project; by default it is the nearest directory above the
input that holds `lakefile.toml` or `lakefile.lean`. `--config FILE` names the configuration;
by default it is the nearest `leankeel.toml` above the input.

### check-source

Reports every diagnostic whose level is `warn` or `deny`.

```json
{
  "Diagnostics": [
    {
      "Rule": "linter.unusedVariables",
      "Level": "warn",
      "Severity": "warning",
      "Range": {
        "Start": { "Line": 1, "Column": 17, "Offset": 17 },
        "End": { "Line": 1, "Column": 18, "Offset": 18 }
      },
      "Message": "Variable name `y` is not explicitly referenced. ...",
      "Fixes": [
        {
          "Title": "Try this: _y",
          "Range": {
            "Start": { "Line": 1, "Column": 17, "Offset": 17 },
            "End": { "Line": 1, "Column": 18, "Offset": 18 }
          },
          "NewText": "_y"
        }
      ]
    }
  ],
  "Summary": { "Deny": 0, "Warn": 1, "Fixable": 1, "Applied": 0, "Rejected": 0 }
}
```

`Line` is 1-based and `Column` is a 0-based count of code points, as Lean prints them. `Offset`
is a UTF-8 byte offset. With `--file` the object also has `Path`; with `--paths` the result is
`{"Files": [...], "Summary": {...}}` with one such object per file, or `{"Path", "Error"}` for a
file that could not be processed.

`--output text` prints one `path:line:column: level [rule] message` line per diagnostic.

### fix-source

Applies the first fix of every diagnostic whose rule is selected, then reports what remains.
The JSON adds `Changed`, `Applied`, `Rejected` and, for `--file` and `--source`, the fixed
`Source`. `--write` writes changed files back. `--output text` prints the fixed source for
`--file` and `--source`.

Rules are selected by `--rules`, otherwise by `fix.rules` in `leankeel.toml`, otherwise the
rules that `rule list-rules` marks `default`. A diagnostic whose level is `allow` is never
fixed.

### Exit status

| Status | Meaning |
| --- | --- |
| 0 | No diagnostic is denied. |
| 252 | The command line is malformed. |
| 254 | A diagnostic is denied, or an input could not be read or elaborated. |
| 255 | Leankeel itself failed. |

## Rules

A rule is the kind that Lean attaches to a message, so a linter from any library is a rule
without Leankeel knowing it: `linter.unusedVariables`, `linter.missingDocs`, and whatever a
project's own linters emit. Messages without a kind are `lean.error`, `lean.warning` and
`lean.info`; an informational message that carries a suggestion (`simp?`, `exact?`) is
`lean.tryThis`.

Errors are always `deny`. A warning is `warn` and an informational message is `allow` unless
`leankeel.toml` says otherwise. `leankeel rule list-rules` prints the rules that have a
built-in default:

| Rule | Lean option | Level | Fix |
| --- | --- | --- | --- |
| `linter.unusedVariables` | `linter.unusedVariables` | warn | default |
| `linter.unusedSimpArgs` | `linter.unusedSimpArgs` | warn | default |
| `linter.unnecessarySimpa` | `linter.unnecessarySimpa` | warn | default |
| `Lean.Linter.deprecatedAttr` | `linter.deprecated` | warn | default |
| `hasSorry` | | warn | none |
| `lean.tryThis` | | warn | opt-in |
| `lean.error` | | deny | none |
| `lean.warning` | | warn | none |
| `lean.info` | | allow | none |

Any rule whose messages carry a suggestion can be fixed by naming it in `--rules` or
`fix.rules`.

## Configuration

```toml
# leankeel.toml
[rules]
"linter.unusedVariables" = "deny"
hasSorry = "deny"
"linter.missingDocs" = "allow"

[fix]
rules = ["linter.unusedVariables", "linter.unusedSimpArgs", "lean.tryThis"]

[lean-options]
"linter.missingDocs" = true
```

`[rules]` maps a rule to `allow`, `warn` or `deny`. `[fix] rules` replaces the default set of
fixed rules. `[lean-options]` sets Lean options for the elaboration, on top of the package's
`leanOptions`; an option that Lean does not know is an error, and the `weak.` prefix makes it
conditional as in `lake`.

## How a fix is verified

1. Leankeel elaborates the file and reads the suggestions that Lean embeds in each message.
2. It applies the selected suggestions that do not overlap, in source order, and elaborates the
   result.
3. The result is kept when it has fewer diagnostics than before and every diagnostic it has was
   already present. Diagnostics are compared by rule, severity and message text, without the
   text of the suggestion.
4. When the batch is not kept, each fix is tried alone. A fix that fails alone is reported under
   `Rejected` and not tried again.
5. The steps repeat until no selected fix is left, so fixes that overlap, such as two unused
   arguments of one `simp`, are applied one round after another.

Verification covers the file being fixed. A fix that renames a binder of a declaration, such as
`y` to `_y`, changes the name that other files may pass as a named argument; build the project
after fixing.

## Design

Rust owns the command line, files, the Lake project and the adapter process. The adapter
(`native/Main.lean`) runs Lean's frontend with the linters enabled and returns each message
with its kind, range and embedded suggestions.

The rules that decide what happens to the source are defined in Lean, proved, and generated
into Rust by [leanfran](https://github.com/conao3/lean-leanfran):

| Module | Definitions | Theorems |
| --- | --- | --- |
| `src/plan.rs` | which edits are applied, the pieces of the new text, where a rejected fix moves after other edits | kept pieces lie inside the text and in increasing order; kept plus removed bytes equal the text; the inserted edits are exactly the chosen ones; an edit that touches a span invalidates it |
| `src/verdict.rs` | whether a change is kept | a kept change has fewer diagnostics and no diagnostic occurs more often than before |
| `src/policy.rs` | the level of a rule, whether it is fixed, the exit status | errors are denied; a setting wins over the default; an allowed rule is never fixed; the status is 0 exactly when nothing is denied |

## Related tools

Leankeel adds no checks. It reports the messages that Lean produces while it elaborates a file,
which covers the linters of Lean, of imported libraries and of the project itself. Checks that
run outside elaboration keep their own commands: environment linters (`#lint`, `runLinter`,
Lake's built-in environment linters), text-based linters and `lake shake`.

| Tool | Role |
| --- | --- |
| Lean 4.35 and Lake | The linters, `lake lint` with the built-in environment and text linters, `lake lint --record-exceptions` to silence existing warnings in place, and `lake shake --fix` for imports. |
| Batteries and Mathlib | Environment linters (`#lint`, `runLinter`), syntax linters and the text-based style linter. Mathlib's `scripts/fix_deprecations.py` and `scripts/fix_unused_simp_args.py` rewrite sources from the warning text of a build log. |
| [lean-fmt](https://github.com/jcreinhold/lean-fmt) | A formatter and linter with its own rule set, rule codes, configuration, JSON and SARIF output, and `check --fix` for its rules. |
| Leankeel | Applies the edit that Lean computed for a message, with the range and replacement taken from the message itself, for every linter that attaches a suggestion, and keeps the edit only when re-elaboration confirms it. |

## Development

```console
nix develop --command make check
nix build
```

`make check` builds the adapter, checks formatting, runs Clippy, checks the Lean theorems,
confirms that the generated Rust in `.leanfran/cache/` and `lean/` matches the source, and runs
the tests.
The `audit/` Lake package runs [leanassay](https://github.com/conao3/lean-leanassay)
on all seven generated functions called by Rust; `make check` fails on theorem
coverage gaps, unrefuted mutations that change the output, and vacuous hypotheses.

## License

Apache-2.0
