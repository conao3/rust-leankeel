namespace model

structure Span where
  start : Nat
  stop : Nat
  deriving DecidableEq, Repr

inductive Piece where
  | keep (start stop : Nat)
  | insert (index : Nat)
  deriving DecidableEq, Repr

def width (span : Span) : Nat := span.stop - span.start

def shiftFrom (span : Span) (added removed : Nat) : List (Span × Nat) → Option Span
  | [] => some ⟨span.start + added - removed, span.stop + added - removed⟩
  | (edit, inserted) :: rest =>
    if edit.stop ≤ span.start then
      shiftFrom span (added + inserted) (removed + width edit) rest
    else if span.stop ≤ edit.start then
      some ⟨span.start + added - removed, span.stop + added - removed⟩
    else none

def shift (span : Span) (applied : List (Span × Nat)) : Option Span :=
  shiftFrom span 0 0 applied

def fits (len pos : Nat) (edit : Span) : Bool :=
  pos ≤ edit.start && edit.start ≤ edit.stop && edit.stop ≤ len

def chosenFrom (len pos index : Nat) : List Span → List Nat
  | [] => []
  | edit :: rest =>
    if fits len pos edit then index :: chosenFrom len edit.stop (index + 1) rest
    else chosenFrom len pos (index + 1) rest

def piecesFrom (len pos index : Nat) : List Span → List Piece
  | [] => [.keep pos len]
  | edit :: rest =>
    if fits len pos edit then
      .keep pos edit.start :: .insert index :: piecesFrom len edit.stop (index + 1) rest
    else piecesFrom len pos (index + 1) rest

def chosen (len : Nat) (edits : List Span) : List Nat := chosenFrom len 0 0 edits

def pieces (len : Nat) (edits : List Span) : List Piece := piecesFrom len 0 0 edits

def Piece.valid (len : Nat) : Piece → Prop
  | .keep start stop => start ≤ stop ∧ stop ≤ len
  | .insert _ => True

def keptLength : List Piece → Nat
  | [] => 0
  | .keep start stop :: rest => (stop - start) + keptLength rest
  | .insert _ :: rest => keptLength rest

def removedFrom (len pos : Nat) : List Span → Nat
  | [] => 0
  | edit :: rest =>
    if fits len pos edit then width edit + removedFrom len edit.stop rest
    else removedFrom len pos rest

def inserted : List Piece → List Nat
  | [] => []
  | .keep _ _ :: rest => inserted rest
  | .insert index :: rest => index :: inserted rest

def increasing (pos : Nat) : List Piece → Prop
  | [] => True
  | .keep start stop :: rest => pos ≤ start ∧ start ≤ stop ∧ increasing stop rest
  | .insert _ :: rest => increasing pos rest

theorem fits_iff (len pos : Nat) (edit : Span) :
    fits len pos edit = true ↔ pos ≤ edit.start ∧ edit.start ≤ edit.stop ∧ edit.stop ≤ len := by
  simp [fits, and_assoc]

theorem pieces_nil (len : Nat) : pieces len [] = [.keep 0 len] := rfl

theorem piecesFrom_valid (len : Nat) (edits : List Span) :
    ∀ pos index, pos ≤ len → ∀ piece ∈ piecesFrom len pos index edits, piece.valid len := by
  induction edits with
  | nil =>
    intro pos index h piece hp
    simp [piecesFrom] at hp
    subst hp
    exact ⟨h, Nat.le_refl len⟩
  | cons edit rest ih =>
    intro pos index h piece hp
    unfold piecesFrom at hp
    split at hp
    next hfit =>
      have ⟨h1, h2, h3⟩ := (fits_iff len pos edit).mp hfit
      simp only [List.mem_cons] at hp
      rcases hp with hp | hp | hp
      · subst hp; exact ⟨h1, by omega⟩
      · subst hp; trivial
      · exact ih edit.stop (index + 1) h3 piece hp
    next => exact ih pos (index + 1) h piece hp

theorem kept_plus_removed (len : Nat) (edits : List Span) :
    ∀ pos index, pos ≤ len →
      keptLength (piecesFrom len pos index edits) + removedFrom len pos edits = len - pos := by
  induction edits with
  | nil => intro pos index h; simp [piecesFrom, keptLength, removedFrom]
  | cons edit rest ih =>
    intro pos index h
    unfold piecesFrom removedFrom
    split
    next hfit =>
      have ⟨h1, h2, h3⟩ := (fits_iff len pos edit).mp hfit
      have := ih edit.stop (index + 1) h3
      simp only [keptLength, width]
      omega
    next => exact ih pos (index + 1) h

theorem inserted_eq_chosen (len : Nat) (edits : List Span) :
    ∀ pos index, inserted (piecesFrom len pos index edits) = chosenFrom len pos index edits := by
  induction edits with
  | nil => intro pos index; rfl
  | cons edit rest ih =>
    intro pos index
    unfold piecesFrom chosenFrom
    split
    next => simp [inserted, ih]
    next => exact ih pos (index + 1)

theorem increasing_of_le (pieces : List Piece) :
    ∀ low high, low ≤ high → increasing high pieces → increasing low pieces := by
  induction pieces with
  | nil => intro _ _ _ _; trivial
  | cons piece rest ih =>
    intro low high hle h
    cases piece with
    | keep start stop => exact ⟨Nat.le_trans hle h.1, h.2.1, h.2.2⟩
    | insert index => exact ih low high hle h

theorem piecesFrom_increasing (len : Nat) (edits : List Span) :
    ∀ pos index, pos ≤ len → increasing pos (piecesFrom len pos index edits) := by
  induction edits with
  | nil => intro pos index h; exact ⟨Nat.le_refl pos, h, trivial⟩
  | cons edit rest ih =>
    intro pos index h
    unfold piecesFrom
    split
    next hfit =>
      have ⟨h1, h2, h3⟩ := (fits_iff len pos edit).mp hfit
      exact ⟨Nat.le_refl pos, h1,
        increasing_of_le _ edit.start edit.stop h2 (ih edit.stop (index + 1) h3)⟩
    next => exact ih pos (index + 1) h

theorem shift_nil (span : Span) : shift span [] = some span := rfl

theorem shift_before (span edit : Span) (inserted : Nat)
    (h1 : span.stop ≤ edit.start) (h2 : span.start < edit.stop) :
    shift span [(edit, inserted)] = some span := by
  have h3 : ¬ edit.stop ≤ span.start := by omega
  simp [shift, shiftFrom, h1, h3]

theorem shift_after (span edit : Span) (inserted : Nat) (h : edit.stop ≤ span.start) :
    shift span [(edit, inserted)] =
      some ⟨span.start + inserted - width edit, span.stop + inserted - width edit⟩ := by
  simp [shift, shiftFrom, h]

theorem shift_touched (span edit : Span) (inserted added removed : Nat)
    (rest : List (Span × Nat)) (h1 : span.start < edit.stop) (h2 : edit.start < span.stop) :
    shiftFrom span added removed ((edit, inserted) :: rest) = none := by
  have h3 : ¬ edit.stop ≤ span.start := by omega
  have h4 : ¬ span.stop ≤ edit.start := by omega
  simp [shiftFrom, h3, h4]

end model
