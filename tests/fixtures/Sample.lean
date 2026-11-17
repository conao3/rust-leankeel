def unusedArg (x y : Nat) : Nat :=
  x + 1

@[deprecated Nat.add_comm (since := "2026-01-01")]
theorem oldAddComm (a b : Nat) : a + b = b + a := Nat.add_comm a b

example (a b : Nat) : a + b = b + a := oldAddComm a b

example (xs : List Nat) : xs ++ [] = xs := by
  simp [List.append_nil, Nat.add_comm]
