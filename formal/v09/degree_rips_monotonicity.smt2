(set-logic QF_UF)

; The larger grade has at least the lower grade's edges and eligible vertices.
(declare-const edge-lower Bool)
(declare-const edge-upper Bool)
(declare-const left-lower Bool)
(declare-const left-upper Bool)
(declare-const right-lower Bool)
(declare-const right-upper Bool)

(assert (=> edge-lower edge-upper))
(assert (=> left-lower left-upper))
(assert (=> right-lower right-upper))

(define-fun simplex-lower () Bool
  (and edge-lower left-lower right-lower))
(define-fun simplex-upper () Bool
  (and edge-upper left-upper right-upper))

; A degree-Rips edge is claimed to disappear along the product order.
(assert simplex-lower)
(assert (not simplex-upper))

(check-sat)
