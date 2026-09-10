(set-logic AUFLIA)

(define-fun precedes ((as Int) (ad Int) (bs Int) (bd Int)) Bool
  (and (<= as bs) (<= ad bd)))

(declare-fun birth-scale (Int) Int)
(declare-fun birth-density (Int) Int)
(declare-const birth-count Int)
(declare-const lower-scale Int)
(declare-const lower-density Int)
(declare-const upper-scale Int)
(declare-const upper-density Int)

(assert (> birth-count 0))
(assert
  (exists ((index Int))
    (and
      (<= 0 index)
      (< index birth-count)
      (precedes
        (birth-scale index)
        (birth-density index)
        lower-scale
        lower-density))))
(assert (precedes lower-scale lower-density upper-scale upper-density))

; A simplex supported below is claimed to be absent above.
(assert
  (forall ((index Int))
    (=>
      (and (<= 0 index) (< index birth-count))
      (not
        (precedes
          (birth-scale index)
          (birth-density index)
          upper-scale
          upper-density)))))

(check-sat)
