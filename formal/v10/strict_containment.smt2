(set-logic QF_LIA)

; The retained guards constrain two independent chains. They omit a relation
; between a and b, so a and b can reverse without breaking either chain.
(declare-const lower-a Int)
(declare-const a-initial Int)
(declare-const a-updated Int)
(declare-const lower-b Int)
(declare-const b-initial Int)
(declare-const b-updated Int)
(declare-const upper Int)

(assert (< lower-a a-initial))
(assert (< lower-a a-updated))
(assert (< lower-b b-initial))
(assert (< lower-b b-updated))
(assert (< a-initial upper))
(assert (< a-updated upper))
(assert (< b-initial upper))
(assert (< b-updated upper))

; The complete-order chamber changes across the two states.
(assert (< a-initial b-initial))
(assert (> a-updated b-updated))

(check-sat)
