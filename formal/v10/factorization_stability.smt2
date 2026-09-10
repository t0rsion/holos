(set-logic QF_BV)

; A two-by-two F2 factorization. Filtration values can change, but D, V, and
; R remain the checked matrices while their support guards hold elsewhere.
(declare-const d00 (_ BitVec 1))
(declare-const d01 (_ BitVec 1))
(declare-const d10 (_ BitVec 1))
(declare-const d11 (_ BitVec 1))
(declare-const v00 (_ BitVec 1))
(declare-const v01 (_ BitVec 1))
(declare-const v10 (_ BitVec 1))
(declare-const v11 (_ BitVec 1))
(declare-const r00 (_ BitVec 1))
(declare-const r01 (_ BitVec 1))
(declare-const r10 (_ BitVec 1))
(declare-const r11 (_ BitVec 1))

(define-fun product-00 () (_ BitVec 1)
  (bvxor (bvand d00 v00) (bvand d01 v10)))
(define-fun product-01 () (_ BitVec 1)
  (bvxor (bvand d00 v01) (bvand d01 v11)))
(define-fun product-10 () (_ BitVec 1)
  (bvxor (bvand d10 v00) (bvand d11 v10)))
(define-fun product-11 () (_ BitVec 1)
  (bvxor (bvand d10 v01) (bvand d11 v11)))

(assert (= product-00 r00))
(assert (= product-01 r01))
(assert (= product-10 r10))
(assert (= product-11 r11))

; The unchanged matrices are claimed not to satisfy the same equation.
(assert
  (or
    (distinct product-00 r00)
    (distinct product-01 r01)
    (distinct product-10 r10)
    (distinct product-11 r11)))

(check-sat)
