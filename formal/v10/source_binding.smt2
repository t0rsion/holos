(set-logic QF_UF)

(declare-sort Graph 0)
(declare-sort Class 0)
(declare-sort Digest 0)
(declare-fun graph-digest (Graph) Digest)
(declare-fun class-digest (Class) Digest)
(declare-const recorded-graph Digest)
(declare-const recorded-class Digest)
(declare-const supplied-graph Graph)
(declare-const supplied-class Class)
(declare-const accepted Bool)

(assert
  (= accepted
    (and
      (= recorded-graph (graph-digest supplied-graph))
      (= recorded-class (class-digest supplied-class)))))

; One binding differs, but the record is claimed to be accepted.
(assert
  (or
    (distinct recorded-graph (graph-digest supplied-graph))
    (distinct recorded-class (class-digest supplied-class))))
(assert accepted)

(check-sat)
