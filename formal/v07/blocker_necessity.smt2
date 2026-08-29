(set-logic QF_AUFBV)

(define-fun subset ((left (_ BitVec 65536)) (right (_ BitVec 65536))) Bool
  (= (bvand left (bvnot right)) (_ bv0 65536)))

(declare-fun survives ((_ BitVec 65536)) Bool)
(declare-const included (_ BitVec 65536))
(declare-const retained (_ BitVec 65536))
(declare-const available (_ BitVec 65536))
(declare-const descendant (_ BitVec 65536))

(define-fun witness () (_ BitVec 65536) (bvor included retained))
(define-fun blocker () (_ BitVec 65536) (bvand available (bvnot retained)))

(assert (subset retained available))
(assert (survives witness))
(assert (subset included descendant))
(assert (subset descendant (bvor included available)))
(assert (= (bvand descendant blocker) (_ bv0 65536)))

; This is the antitone instance used by the proof rule.
(assert (=> (and (subset descendant witness) (survives witness))
            (survives descendant)))

(assert (not (survives descendant)))

(check-sat)
