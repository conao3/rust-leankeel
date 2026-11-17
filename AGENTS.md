# Leankeel

Rust edition 2024, clap derive, serde, toml, leanfran, flake-parts and treefmt-nix. Follow shared
notes/new-project-stack.md, notes/lang/rust.md and notes/lang/lean.md. Run
`nix develop --command make check` and `nix build` before committing.

Leankeel reports Lean's diagnostics as rules and applies the fixes Lean suggests. It adds no
checks of its own: detection belongs to Lean, Lake and library linters. A rule is the message
kind Lean attaches, used verbatim.

## Lean and Rust

`native/Main.lean` is the adapter, a native Lean 4.35.0 program. It reads one JSON request
(`FileName`, `Source`, `Options`, `Setup`) from standard input, runs the frontend with
`Elab.async` enabled, and prints one JSON object with every message: kind, severity, range, full
text, text without hints, and the suggestions embedded in the message as hint widgets. Code
action titles come from the `TryThisInfo` nodes of the info trees. A failure of the analysis
itself is returned as `{"Error": ...}`.

The decisions about the source are Lean definitions with theorems, embedded with `leanfran::lean!`
and generated into Rust. Add to these blocks when a rule is deterministic and independent of I/O:

- `src/plan.rs`: `pieces` and `chosen` select non-overlapping edits and describe the new text;
  `shift` moves the span of a rejected fix across applied edits.
- `src/verdict.rs`: `accept` keeps a change only when diagnostics shrink without a new one.
- `src/policy.rs`: `level`, `fixable` and `exitCode`.

The properties proved in these blocks are:

| Theorem | Property |
| --- | --- |
| `fits_iff` | An edit fits exactly when its endpoints are ordered, at or after the cursor, and within the source. |
| `pieces_nil` | With zero edits, the whole source is kept. |
| `piecesFrom_valid` | Every kept slice has ordered endpoints within the source. |
| `kept_plus_removed` | Kept and removed lengths add up to the unprocessed source length. |
| `inserted_eq_chosen` | The planner's inserted indices match its selected edits at every cursor and index. |
| `increasing_of_le` | An ordered piece sequence remains ordered from an earlier cursor. |
| `piecesFrom_increasing` | Kept slices follow source order without overlap. |
| `shift_nil` | An empty edit list preserves a rejected fix's span. |
| `shift_before` | An edit after a rejected fix preserves its span. |
| `shift_after` | An edit before a rejected fix shifts both endpoints by its length change. |
| `shift_touched` | An overlapping edit invalidates the rejected fix in the shift traversal. |
| `pieces_chosen` | The inserted indices in the emitted text match the applied edits reported by Rust. |
| `accept_shrinks` | An accepted change strictly reduces the number of diagnostics. |
| `accept_no_new` | An accepted change preserves or reduces the count of every diagnostic key. |
| `accept_irreflexive` | An unchanged diagnostic list is rejected. |
| `accept_removing_head` | Removing the first diagnostic while retaining the rest is accepted. |
| `error_is_deny` | Lean errors always have deny level. |
| `first_setting_wins` | The first matching configuration entry determines the override. |
| `configured_warning` | A configured warning has the configured level. |
| `allowed_is_not_fixed` | Allow-level diagnostics are excluded from fixes. |
| `exit_zero_iff` | Exit status is zero exactly when there is no deny-level diagnostic. |
| `exit_is_zero_or_254` | Diagnostic exit status is either zero or 254. |
| `level_defaults` | Unconfigured warnings default to warn and information to allow. |
| `level_override` | The first matching setting applies to both non-error severities. |
| `fixable_iff` | A diagnostic is fixable exactly when selected and at warn or deny level. |

`audit/` is a separate Lake package using
[leanassay](https://github.com/conao3/lean-leanassay) at the revision pinned in its
manifest. It audits all seven generated functions called by Rust: `pieces`,
`chosen`, `shift`, `accept`, `level`, `fixable` and `exitCode`. Each function has
an `#assay` command in `audit/Audit/`, grouped by its source block. Add a useful
theorem and an audit command with each generated function used by Rust.
`lake -d audit build --wfail` rejects missing theorems, functions whose mutants
are all unrefuted, holes, and hypotheses without a satisfying sampled input.
The audit mutates the named function's body; helper properties are listed above
with their own scope. Equivalent results mean equal outputs on sampled inputs.

| Audited function | Mutants | Refuted | Holes | Untested | Equivalent | Ill-typed or failed at runtime |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `pieces` | 4 | 4 | 0 | 0 | 0 | 0 |
| `chosen` | 4 | 4 | 0 | 0 | 0 | 0 |
| `shift` | 4 | 4 | 0 | 0 | 0 | 0 |
| `accept` | 7 | 7 | 0 | 0 | 0 | 0 |
| `level` | 9 | 9 | 0 | 0 | 0 | 0 |
| `fixable` | 6 | 6 | 0 | 0 | 0 | 0 |
| `exitCode` | 6 | 6 | 0 | 0 | 0 | 0 |
| Total | 40 | 40 | 0 | 0 | 0 | 0 |

All seven Rust entry points are included; exclusions and timeouts are both zero.

Rust owns the command line, configuration, file and process I/O, `lake setup-file` and
`lake env`. `src/lib.rs` calls the generated functions on the real path; do not reimplement
their logic in Rust.

Commit `lean/` and `.leanfran/cache/` together with the source of a `lean!` block. `make check`
fails when they differ from what the build generates.

## Conventions

- The command line follows the AWS CLI grammar of notes/new-project-stack.md: named parameters,
  PascalCase JSON keys, exit status 0, 252, 254 and 255.
- A fix is applied only through the verification loop of `fix` in `src/lib.rs`.
- A rule listed in `RULES` has a test that fails on an offending input and passes on a clean
  one, and a fixed file is checked again.
- `LEANKEEL_ADAPTER` names the adapter executable. The Makefile points it at
  `native/.lake/build/bin/leankeel-adapter`; the Nix package wraps the binary with it.

## Verification

- `nix develop --command make check`: adapter build, `nix fmt -- --ci`, Clippy, `lake -d lean build`
  for the theorems, `lake -d audit build --wfail` for their mutation audit,
  the generated-code check, unit tests and the CLI tests in `tests/cli.rs`
  (which run the adapter, including the Lake projects under `tests/fixtures/`: package options and
  imports in `project`, a linter defined by the project and module-system files in `extension`).
- `nix build`: the package with the adapter; its check phase runs the same tests.
- `result/bin/leankeel source check-source --file <file>` exercises the installed binary.
