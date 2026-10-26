# N3 conformance corpus

Portable Notation 3 conformance fixtures under the portable corpus contract,
version 1 (see `../community/SPEC.md`). Cases are named per-family; every case
carries an inventory record and a reachable RDF manifest entry whose base is
`http://example.org/n3/`.

Positive parse cases (`query-evaluation`) expect an independently authored
N-Triples artifact. N3 rule cases (`rules`) split rule document (shapes slot)
from facts document (data slot) and expect an independently authored inferences
artifact. Syntax-error cases (`negative-syntax`) carry no result artifact.
