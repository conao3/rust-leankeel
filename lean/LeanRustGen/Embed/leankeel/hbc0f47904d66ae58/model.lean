namespace model

def noNew (before after : List String) : Bool :=
  after.all fun key => after.count key ≤ before.count key

def accept (before after : List String) : Bool :=
  after.length < before.length && noNew before after

theorem accept_shrinks (before after : List String) (h : accept before after = true) :
    after.length < before.length := by
  simp [accept] at h
  exact h.1

theorem accept_no_new (before after : List String) (h : accept before after = true) (key : String) :
    after.count key ≤ before.count key := by
  simp [accept, noNew] at h
  by_cases hk : key ∈ after
  · exact h.2 key hk
  · simp [List.count_eq_zero_of_not_mem hk]

theorem accept_irreflexive (messages : List String) : accept messages messages = false := by
  simp [accept]

end model
