(set-logic QF_LIA)

(declare-const chosen-cost Int)
(declare-const alternative-left-cost Int)
(declare-const alternative-right-cost Int)
(declare-const left-frontier-cost Int)
(declare-const right-frontier-cost Int)

; Each frontier value is a lower bound for every local plan at the
; alternative plan's activation count.
(assert (>= alternative-left-cost left-frontier-cost))
(assert (>= alternative-right-cost right-frontier-cost))

; The dynamic program chose no more than this frontier combination.
(assert
  (<= chosen-cost (+ left-frontier-cost right-frontier-cost)))

(assert (>= left-frontier-cost 0))
(assert (>= right-frontier-cost 0))
(assert
  (< (+ alternative-left-cost alternative-right-cost) chosen-cost))

(check-sat)
