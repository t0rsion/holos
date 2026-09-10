(set-logic QF_LIA)

(declare-const source-rank Int)
(declare-const map-rank Int)
(declare-const has-solution Bool)

(assert (<= 0 map-rank))
(assert (<= map-rank source-rank))

(define-fun kernel-rank () Int (- source-rank map-rank))
(define-fun unique () Bool (and has-solution (= kernel-rank 0)))
(define-fun ambiguous () Bool (and has-solution (> kernel-rank 0)))
(define-fun no-extension () Bool (not has-solution))

; The three fiber classifications are claimed not to be exactly one.
(assert
  (not
    (or
      (and unique (not ambiguous) (not no-extension))
      (and (not unique) ambiguous (not no-extension))
      (and (not unique) (not ambiguous) no-extension))))

(check-sat)
