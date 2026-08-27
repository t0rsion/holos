(set-logic QF_UF)

(declare-sort Selection 0)
(declare-fun restrict-left (Selection) Selection)
(declare-fun restrict-right (Selection) Selection)
(declare-fun left-feasible (Selection) Bool)
(declare-fun right-feasible (Selection) Bool)
(declare-fun global-feasible (Selection) Bool)
(declare-const selected Selection)

; State-action incidence components make the global predicate a conjunction
; of predicates on the two restricted selections.
(assert
  (= (global-feasible selected)
     (and (left-feasible (restrict-left selected))
          (right-feasible (restrict-right selected)))))

(assert (left-feasible (restrict-left selected)))
(assert (right-feasible (restrict-right selected)))
(assert (not (global-feasible selected)))

(check-sat)
