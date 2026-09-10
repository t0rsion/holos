(set-logic QF_LIA)

; Atom identifiers are abstract. Only atom A contains a changed edge. The
; unchanged atom B retains its graph digest and checked factorization.
(declare-const changed-a Bool)
(declare-const changed-b Bool)
(declare-const digest-b-same Bool)
(declare-const factorization-b-valid Bool)
(declare-const rebuild-b Bool)

(assert changed-a)
(assert (not changed-b))
(assert digest-b-same)
(assert factorization-b-valid)
(assert (= rebuild-b (or changed-b (not digest-b-same) (not factorization-b-valid))))

; An untouched, still-valid atom is claimed to require rebuilding.
(assert rebuild-b)

(check-sat)
