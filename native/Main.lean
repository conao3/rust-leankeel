import Lean

open Lean Elab

namespace Leankeel

structure Request where
  fileName : String
  source : String
  options : Json := Json.mkObj []
  setup : Option ModuleSetup := none

def Request.ofJson (json : Json) : Except String Request := do
  let fileName ← json.getObjValAs? String "FileName"
  let source ← json.getObjValAs? String "Source"
  let options := (json.getObjVal? "Options").toOption.getD (Json.mkObj [])
  let setup ← match json.getObjVal? "Setup" with
    | .ok .null | .error _ => pure none
    | .ok value => some <$> fromJson? value
  return { fileName, source, options, setup }

def optionsOfJson (json : Json) : Except String Options := do
  let .obj entries := json | throw "Options must be an object"
  entries.foldlM (init := {}) fun (options : Options) name value => do
    let value : DataValue ← match value with
      | .bool b => pure (.ofBool b)
      | .str s => pure (.ofString s)
      | .num n =>
        if n.exponent == 0 && n.mantissa ≥ 0 then pure (.ofNat n.mantissa.toNat)
        else throw s!"option {name}: expected a natural number"
      | _ => throw s!"option {name}: expected a boolean, a string or a natural number"
    return options.set name.toName value

def position (map : FileMap) (pos : String.Pos.Raw) : Json :=
  let p := map.toPosition pos
  Json.mkObj [("Line", p.line), ("Column", p.column), ("Offset", pos.byteIdx)]

partial def withoutHints : MessageData → MessageData
  | .tagged `hint _ => .nil
  | .tagged tag message => .tagged tag (withoutHints message)
  | .compose first second => .compose (withoutHints first) (withoutHints second)
  | .nest indent message => .nest indent (withoutHints message)
  | .group message => .group (withoutHints message)
  | .withContext context message => .withContext context (withoutHints message)
  | .withNamingContext context message => .withNamingContext context (withoutHints message)
  | .ofOriginatingSyntax origin message => .ofOriginatingSyntax origin (withoutHints message)
  | .ofWidget widget message => .ofWidget widget (withoutHints message)
  | message => message

def severity : MessageSeverity → String
  | .information => "information"
  | .warning => "warning"
  | .error => "error"

structure Suggestion where
  start : String.Pos.Raw
  stop : String.Pos.Raw
  newText : String
  title : String

partial def titles (map : FileMap) (tree : InfoTree) (found : Array Suggestion) :
    Array Suggestion :=
  match tree with
  | .context _ tree => titles map tree found
  | .hole _ => found
  | .node info children =>
    let found := match info with
      | .ofCustomInfo custom =>
        match custom.value.get? Lean.Meta.Tactic.TryThis.TryThisInfo with
        | some suggestion =>
          found.push {
            start := map.lspPosToUtf8Pos suggestion.edit.range.start
            stop := map.lspPosToUtf8Pos suggestion.edit.range.end
            newText := suggestion.edit.newText
            title := suggestion.codeActionTitle }
        | none => found
      | _ => found
    children.foldl (fun found child => titles map child found) found

partial def widgets (data : MessageData) (found : Array Widget.WidgetInstance) :
    Array Widget.WidgetInstance :=
  match data with
  | .ofWidget widget message => widgets message (found.push widget)
  | .tagged _ message => widgets message found
  | .compose first second => widgets second (widgets first found)
  | .nest _ message => widgets message found
  | .group message => widgets message found
  | .withContext _ message => widgets message found
  | .withNamingContext _ message => widgets message found
  | .ofOriginatingSyntax _ message => widgets message found
  | .trace _ message children =>
    children.foldl (fun found child => widgets child found) (widgets message found)
  | _ => found

def fixes (map : FileMap) (known : Array Suggestion) (data : MessageData) : Array Json :=
  (widgets data #[]).filterMap fun widget =>
    let props := widget.props.run' {}
    match props.getObjValAs? Lsp.Range "range", props.getObjValAs? String "suggestion" with
    | .ok range, .ok newText =>
      let start := map.lspPosToUtf8Pos range.start
      let stop := map.lspPosToUtf8Pos range.end
      let title := known.find? fun suggestion =>
        suggestion.start == start && suggestion.stop == stop && suggestion.newText == newText
      some <| Json.mkObj [
        ("Title", (title.map (·.title)).getD s!"Try this: {newText}"),
        ("NewText", newText),
        ("Start", position map start),
        ("End", position map stop)]
    | _, _ => none

def message (map : FileMap) (known : Array Suggestion) (m : Message) : IO Json := do
  let stop := m.endPos.getD m.pos
  return Json.mkObj [
    ("Kind", if m.data.kind.isAnonymous then Json.null else toString m.data.kind),
    ("Severity", severity m.severity),
    ("Start", position map (map.ofPosition m.pos)),
    ("End", position map (map.ofPosition stop)),
    ("Message", ← m.data.toString),
    ("Text", ← (withoutHints m.data).toString),
    ("Fixes", Json.arr (fixes map known m.data))]

def analyze (request : Request) : IO Json := do
  let requested ← IO.ofExcept (optionsOfJson request.options)
  let package := match request.setup with
    | some setup => setup.options.toOptions
    | none => {}
  let initial := package.mergeBy (fun _ _ value => value) requested
  let initial := Elab.async.setIfNotSet initial true
  let inputCtx := Parser.mkInputContext request.source request.fileName
  let map := inputCtx.fileMap
  let (header, parserState, messages) ← Parser.parseHeader inputCtx
  let (env, messages) ← match request.setup with
    | some setup => do
      setup.dynlibs.forM Lean.loadDynlib
      processHeaderCore (HeaderSyntax.startPos header)
        (setup.imports?.getD (HeaderSyntax.imports header))
        (strictOr setup.isModule (HeaderSyntax.isModule header)) initial messages inputCtx
        (plugins := setup.plugins) (mainModule := setup.name) (package? := setup.package?)
        (arts := setup.importArts) (headerStx? := header)
    | none => processHeader header initial messages inputCtx
  if messages.hasErrors then
    let messages ← messages.toList.mapM (message map #[])
    return Json.mkObj [("Messages", Json.arr messages.toArray)]
  let package ← Lean.Language.Lean.reparseOptions package
  let requested ← Lean.Language.Lean.reparseOptions requested
  let options := package.mergeBy (fun _ _ value => value) requested
  let options := Elab.async.setIfNotSet options true
  let commandState := Command.mkState env messages options
  let commandState := { commandState with infoState.enabled := true }
  let state ← IO.processCommands inputCtx parserState commandState
  let known := state.commandState.infoState.trees.foldl
    (fun found tree => titles map tree found) #[]
  let messages ← state.commandState.messages.toList.mapM (message map known)
  return Json.mkObj [("Messages", Json.arr messages.toArray)]

end Leankeel

unsafe def main : IO UInt32 := do
  enableInitializersExecution
  initSearchPath (← findSysroot)
  let stdin ← IO.getStdin
  let line ← stdin.getLine
  match Json.parse line >>= Leankeel.Request.ofJson with
  | .error error =>
    IO.eprintln s!"invalid request: {error}"
    return 1
  | .ok request =>
    let result ← (Leankeel.analyze request).toBaseIO
    match result with
    | .ok result => IO.println result.compress
    | .error error => IO.println (Json.mkObj [("Error", toString error)]).compress
    return 0
