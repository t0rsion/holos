(set-logic AUFBV)

; A 16-bit index covers every position in the 65,536-action format.
(declare-const selected (Array (_ BitVec 16) Bool))
(declare-const blocker (Array (_ BitVec 16) Bool))
(declare-const first_1 (_ BitVec 16))
(declare-const first_2 (_ BitVec 16))

(define-fun chosen ((index (_ BitVec 16))) Bool
  (and (select selected index) (select blocker index)))

; Two children both claim to contain the least selected blocker member.
(assert (chosen first_1))
(assert (chosen first_2))
(assert
  (forall ((index (_ BitVec 16)))
    (=> (chosen index) (bvule first_1 index))))
(assert
  (forall ((index (_ BitVec 16)))
    (=> (chosen index) (bvule first_2 index))))
(assert (distinct first_1 first_2))

(check-sat)
