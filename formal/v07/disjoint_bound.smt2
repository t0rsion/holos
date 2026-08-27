(set-logic QF_LIA)

(declare-const chosen_1 Int)
(declare-const chosen_2 Int)
(declare-const chosen_3 Int)
(declare-const minimum_1 Int)
(declare-const minimum_2 Int)
(declare-const minimum_3 Int)
(declare-const included_cost Int)
(declare-const claimed_bound Int)

(assert (>= chosen_1 minimum_1))
(assert (>= chosen_2 minimum_2))
(assert (>= chosen_3 minimum_3))
(assert (>= minimum_1 0))
(assert (>= minimum_2 0))
(assert (>= minimum_3 0))
(assert (>= included_cost 0))
(assert
  (= claimed_bound
     (+ included_cost minimum_1 minimum_2 minimum_3)))

(assert
  (< (+ included_cost chosen_1 chosen_2 chosen_3)
     claimed_bound))

(check-sat)
