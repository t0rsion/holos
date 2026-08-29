(set-logic QF_AUFBV)

(define-fun subset ((left (_ BitVec 64)) (right (_ BitVec 64))) Bool
  (= (bvand left (bvnot right)) (_ bv0 64)))

(declare-fun covers ((_ BitVec 64)) Bool)
(declare-const active (_ BitVec 64))
(declare-const smaller-failure (_ BitVec 64))
(declare-const maximal-failure (_ BitVec 64))

(define-fun smaller-survivors () (_ BitVec 64)
  (bvand active (bvnot smaller-failure)))
(define-fun maximal-survivors () (_ BitVec 64)
  (bvand active (bvnot maximal-failure)))

(assert (subset smaller-failure maximal-failure))
(assert (subset maximal-survivors smaller-survivors))

; Adding active sensors cannot destroy an existing relative filling chain.
(assert
  (=> (and (subset maximal-survivors smaller-survivors)
           (covers maximal-survivors))
      (covers smaller-survivors)))

(assert (covers maximal-survivors))
(assert (not (covers smaller-survivors)))

(check-sat)
