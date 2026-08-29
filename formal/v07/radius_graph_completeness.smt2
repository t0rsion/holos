(set-logic QF_NRA)

(declare-const dx Real)
(declare-const dy Real)
(declare-const radius Real)
(declare-const edge Bool)

(assert (>= radius 0.0))

(define-fun within-radius () Bool
  (<= (+ (* dx dx) (* dy dy)) (* radius radius)))

; The geometry checker defines the edge set by this exact predicate.
(assert (= edge within-radius))

; A declared edge cannot disagree with its exact squared distance.
(assert (distinct edge within-radius))

(check-sat)
