Feature: Knowledge Hooks Fixpoint Materialization and Policy Enforcement

  Scenario: Basic knowledge derivation and immutable lineage recording
    Given the hermetic WASM turnstile engine is initialized
    And the canonical ontology namespace "http://seanchatmangpt.github.io/graphlaw/kh#" is mapped to prefix "kh"
    And the max fixpoint round ceiling is set to 64
    And an initial RDF dataset containing:
      """
      @prefix ex: <http://example.org/> .
      ex:alice a ex:User ;
          ex:name "Alice" .
      ex:bob a ex:User ;
          ex:name "Bob" .
      """
    And an active Knowledge Hook declared as:
      """
      @prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
      @prefix ex: <http://example.org/> .

      ex:auditHook a kh:Hook ;
          kh:name "user-auditor" ;
          kh:kind "sparql-select" ;
          kh:query "PREFIX ex: <http://example.org/> SELECT ?u WHERE { ?u a ex:User }" ;
          kh:action ex:auditAction ;
          kh:priority 10 ;
          kh:effect "emit-delta" .

      ex:auditAction a kh:Action ;
          kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
          kh:query "PREFIX ex: <http://example.org/> CONSTRUCT { ?u ex:audited true } WHERE {}" .
      """
    When the hook pack is materialized to a fixpoint
    Then the dataset should contain the triple:
      | subject                 | predicate                   | object |
      | http://example.org/alice | http://example.org/audited | true   |
      | http://example.org/bob   | http://example.org/audited | true   |
    And each firing should record an immutable lineage marker:
      | class                                      |
      | http://seanchatmangpt.github.io/graphlaw/kh#Firing |
    And re-materializing the hook pack should produce 0 new rounds and 0 new firings

  Scenario: Fail-closed typed refusal halts evaluation with zero state mutation
    Given the hermetic WASM turnstile engine is initialized
    And the max fixpoint round ceiling is set to 64
    And an initial RDF dataset containing:
      """
      @prefix ex: <http://example.org/> .
      ex:charlie a ex:RestrictedEntity .
      """
    And an active Knowledge Hook declared as:
      """
      @prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
      @prefix ex: <http://example.org/> .

      ex:refuseHook a kh:Hook ;
          kh:name "restriction-fuse" ;
          kh:kind "sparql-select" ;
          kh:query "PREFIX ex: <http://example.org/> SELECT ?e WHERE { ?e a ex:RestrictedEntity }" ;
          kh:action ex:dummyAction ;
          kh:priority 1 ;
          kh:effect "refuse" ;
          kh:reason "Restricted entities cannot be actuated." .

      ex:dummyAction a kh:Action ;
          kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
          kh:query "CONSTRUCT WHERE {}" .
      """
    When the hook pack materialization is evaluated
    Then the evaluation must halt with a typed refusal:
      | field   | value                                      |
      | code    | Refused                                    |
      | reason  | Restricted entities cannot be actuated.    |
    And exactly 0 state quads should be mutated
