(set-logic QF_NIA)

(declare-const modulus Int)
(declare-const coefficient Int)

(assert (> modulus 1))
(assert (>= coefficient 0))
(assert (< coefficient modulus))

; The oriented boundary of [a,b,c] is [b,c] - [a,c] + [a,b].
; Applying the edge boundary at each vertex gives the three sums below.
(define-fun at-a () Int (+ coefficient (- coefficient)))
(define-fun at-b () Int (+ (- coefficient) coefficient))
(define-fun at-c () Int (+ coefficient (- coefficient)))

(assert
  (or (not (= (mod at-a modulus) 0))
      (not (= (mod at-b modulus) 0))
      (not (= (mod at-c modulus) 0))))

(check-sat)
