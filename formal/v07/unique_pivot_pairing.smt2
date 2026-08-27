(set-logic QF_LIA)

(declare-const first-column Int)
(declare-const second-column Int)
(declare-const first-pivot Int)
(declare-const second-pivot Int)

(assert (>= first-pivot 0))
(assert (>= second-pivot 0))
(assert (distinct first-column second-column))

; Reduced nonzero columns have distinct pivots.
(assert
  (=> (distinct first-column second-column)
      (distinct first-pivot second-pivot)))

; One birth row cannot be paired to two death columns.
(assert (= first-pivot second-pivot))

(check-sat)
