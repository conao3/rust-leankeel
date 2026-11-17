import Lean

open Lean Elab Command Linter

namespace Ext

def current : Nat := 1

def retired : Nat := 1

register_option linter.ext.retired : Bool := {
  defValue := true
  descr := "report uses of `Ext.retired`"
}

def retiredLinter : Linter where
  run := withSetOptionIn fun stx => do
    unless getLinterValue linter.ext.retired (← getLinterOptions) do
      return
    for node in stx.topDown do
      if node.isIdent && node.getId == `Ext.retired then
        let suggestion : Meta.Hint.Suggestion := "Ext.current"
        let suggestion := { suggestion with span? := node, diffGranularity := .none }
        let hint ← liftCoreM <|
          MessageData.hint m!"Use the current name:" #[suggestion] (ref? := node)
        logLint linter.ext.retired node (m!"`Ext.retired` is retired" ++ hint)

initialize addLinter retiredLinter

end Ext
