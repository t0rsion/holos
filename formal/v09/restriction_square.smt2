(set-logic QF_BV)

; Bit positions stand for labeled cochain coordinates.
(declare-const class (_ BitVec 32))
(declare-const lower-mask (_ BitVec 32))
(declare-const scale-mask (_ BitVec 32))
(declare-const density-mask (_ BitVec 32))

; Both intermediate complexes contain the lower complex.
(assert (= (bvand lower-mask scale-mask) lower-mask))
(assert (= (bvand lower-mask density-mask) lower-mask))

(define-fun scale-path () (_ BitVec 32)
  (bvand (bvand class scale-mask) lower-mask))
(define-fun density-path () (_ BitVec 32)
  (bvand (bvand class density-mask) lower-mask))

; Restrictions through the two sides of a square are claimed to differ.
(assert (distinct scale-path density-path))

(check-sat)
