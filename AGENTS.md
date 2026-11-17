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
  for the theorems, the generated-code check, unit tests and the CLI tests in `tests/cli.rs`
  (which run the adapter, including the Lake projects under `tests/fixtures/`: package options and
  imports in `project`, a linter defined by the project and module-system files in `extension`).
- `nix build`: the package with the adapter; its check phase runs the same tests.
- `result/bin/leankeel source check-source --file <file>` exercises the installed binary.
