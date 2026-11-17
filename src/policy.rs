leanfran::lean! {
    pub mod model;
    r#"
    inductive Severity where
      | information | warning | error
      deriving DecidableEq, Repr

    inductive Level where
      | allow | warn | deny
      deriving DecidableEq, Repr

    def configured (rule : String) : List (String × Level) → Option Level
      | [] => none
      | (name, level) :: rest => if name == rule then some level else configured rule rest

    def level (settings : List (String × Level)) (rule : String) (severity : Severity) : Level :=
      match severity with
      | .error => .deny
      | .warning => (configured rule settings).getD .warn
      | .information => (configured rule settings).getD .allow

    def fixable (level : Level) (selected : Bool) : Bool :=
      selected && level != .allow

    def exitCode (levels : List Level) : Nat :=
      if levels.contains .deny then 254 else 0

    theorem error_is_deny (settings : List (String × Level)) (rule : String) :
        level settings rule .error = .deny := rfl

    theorem first_setting_wins (rule : String) (chosen : Level) (rest : List (String × Level)) :
        configured rule ((rule, chosen) :: rest) = some chosen := by
      simp [configured]

    theorem configured_warning (settings : List (String × Level)) (rule : String) (chosen : Level)
        (h : configured rule settings = some chosen) : level settings rule .warning = chosen := by
      simp [level, h]

    theorem allowed_is_not_fixed (selected : Bool) : fixable .allow selected = false := by
      simp [fixable]

    theorem exit_zero_iff (levels : List Level) : exitCode levels = 0 ↔ Level.deny ∉ levels := by
      simp [exitCode]

    theorem exit_is_zero_or_254 (levels : List Level) : exitCode levels = 0 ∨ exitCode levels = 254 := by
      unfold exitCode
      split <;> simp
    "#
}
