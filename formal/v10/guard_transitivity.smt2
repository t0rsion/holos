(set-logic QF_LIA)

; A removed comparison has a path through retained comparisons.
(declare-const first Int)
(declare-const middle Int)
(declare-const last Int)

(assert (<= first middle))
(assert (<= middle last))

; The transitively implied comparison is claimed to fail.
(assert (> first last))

(check-sat)
