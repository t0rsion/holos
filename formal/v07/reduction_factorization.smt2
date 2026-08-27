(set-logic AUF)

(declare-sort ChainQ 0)
(declare-sort ChainQMinusOne 0)
(declare-sort ChainQMinusTwo 0)
(declare-fun boundary-q (ChainQ) ChainQMinusOne)
(declare-fun boundary-q-minus-one (ChainQMinusOne) ChainQMinusTwo)
(declare-fun basis-column (ChainQ) ChainQ)
(declare-fun reduced-column (ChainQ) ChainQMinusOne)
(declare-const zero ChainQMinusTwo)
(declare-const column ChainQ)

; The checked complex is a chain complex.
(assert
  (forall ((value ChainQ))
    (= (boundary-q-minus-one (boundary-q value)) zero)))

; The certificate checker establishes D V = R column by column.
(assert
  (forall ((value ChainQ))
    (= (reduced-column value) (boundary-q (basis-column value)))))

; A checked reduced column therefore cannot have a nonzero boundary.
(assert
  (distinct (boundary-q-minus-one (reduced-column column)) zero))

(check-sat)
