(set-logic QF_LIA)

; Positions stand for a new total filtration order. Every nonzero off-diagonal
; term in V and every nonpivot term in R contributes one retained comparison.
(declare-const position-0 Int)
(declare-const position-1 Int)
(declare-const position-2 Int)
(declare-const position-3 Int)

(assert (distinct position-0 position-1 position-2 position-3))
(assert (and (<= 0 position-0) (< position-0 4)))
(assert (and (<= 0 position-1) (< position-1 4)))
(assert (and (<= 0 position-2) (< position-2 4)))
(assert (and (<= 0 position-3) (< position-3 4)))

; The bounded support is 0 -> 2, 1 -> 2, and 2 -> 3.
(assert (< position-0 position-2))
(assert (< position-1 position-2))
(assert (< position-2 position-3))

; A checked support term is claimed to point backward in the new filtration.
(assert
  (or
    (>= position-0 position-2)
    (>= position-1 position-2)
    (>= position-2 position-3)))

(check-sat)
