(set-logic QF_LIA)

(declare-const winner-primary Int)
(declare-const winner-secondary Int)
(declare-const winner-order Int)
(declare-const alternative-primary Int)
(declare-const alternative-secondary Int)
(declare-const alternative-order Int)

(define-fun lex-leq (
    (left-primary Int)
    (left-secondary Int)
    (left-order Int)
    (right-primary Int)
    (right-secondary Int)
    (right-order Int)) Bool
  (or (< left-primary right-primary)
      (and (= left-primary right-primary)
           (< left-secondary right-secondary))
      (and (= left-primary right-primary)
           (= left-secondary right-secondary)
           (<= left-order right-order))))

(define-fun lex-lt (
    (left-primary Int)
    (left-secondary Int)
    (left-order Int)
    (right-primary Int)
    (right-secondary Int)
    (right-order Int)) Bool
  (or (< left-primary right-primary)
      (and (= left-primary right-primary)
           (< left-secondary right-secondary))
      (and (= left-primary right-primary)
           (= left-secondary right-secondary)
           (< left-order right-order))))

; The verifier checks this relation for every declared candidate.
(assert
  (lex-leq winner-primary winner-secondary winner-order
           alternative-primary alternative-secondary alternative-order))

; Assume an alternative is strictly better under the same ordering.
(assert
  (lex-lt alternative-primary alternative-secondary alternative-order
          winner-primary winner-secondary winner-order))

(check-sat)
