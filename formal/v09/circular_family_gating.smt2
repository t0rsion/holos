(set-logic QF_LIA)

; Kind 1 is unique, 2 is ambiguous, and 3 is no extension.
(declare-const kind Int)
(declare-const has-coordinate Bool)

(assert (or (= kind 1) (= kind 2) (= kind 3)))
(assert (= has-coordinate (= kind 1)))

; A family entry is claimed to violate the coordinate gate.
(assert
  (or
    (and (= kind 1) (not has-coordinate))
    (and (not (= kind 1)) has-coordinate)))

(check-sat)
