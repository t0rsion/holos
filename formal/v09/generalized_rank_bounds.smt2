(set-logic QF_LIA)

(declare-const relation-rank Int)
(declare-const union-rank Int)
(declare-const selected-node-rank Int)
(declare-const ambient-rank Int)

(assert (<= 0 relation-rank))
(assert (<= relation-rank union-rank))
(assert (<= union-rank ambient-rank))
(assert (<= 0 selected-node-rank))
(assert (<= union-rank (+ relation-rank selected-node-rank)))

(define-fun generalized-rank () Int (- union-rank relation-rank))

; The limit-to-colimit rank is claimed to violate a dimension bound.
(assert
  (or
    (< generalized-rank 0)
    (> generalized-rank selected-node-rank)))

(check-sat)
