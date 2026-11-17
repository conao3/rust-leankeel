namespace Keel

/-- The depth below the waterline. -/
def draft (n : Nat) : Nat := n + 1

@[deprecated draft (since := "2026-01-01")]
def depth (n : Nat) : Nat := draft n

end Keel
