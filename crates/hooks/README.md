# `purrdf-hooks`

Declarative, RDF-described, SPARQL/SQL-driven knowledge derivation rules, soft-fail governance, and automated remediation for PurRDF.

## Overview

`purrdf-hooks` implements the **Knowledge Hooks (`kh:`)** specification. Hooks are declared as ordinary RDF triples (in Turtle, N-Quads, or JSON-LD), defining reactive derivation and policy enforcement rules:
- **Condition Engine**: Matches graph patterns via SPARQL (SELECT/ASK), SHACL shapes, SQL-in-RDF queries, threshold cardinalities, or temporal sliding windows.
- **Action Engine**: Executes SPARQL `CONSTRUCT` queries or parameterized DuckDB SQL queries that project tabular results into new RDF statements.
- **Fixpoint Semantics**: Evaluates hooks in ascending `kh:priority` order across successive rounds until no new statements are emitted (or bounded ceilings are reached).
- **Lineage Deduplication**: Every firing writes an interned marker quad (`urn:graphlaw:hook-firing:<blake3> a kh:Firing`) into the dataset, guaranteeing that each solution row fires at most once per state lineage.
- **Typed Fuses & Governance**: Supports hard fuses (`kh:effect "refuse"`), SHACL soft-fail annotations (`kh:enforcementMode "annotate"`), and automated remediation (`kh:enforcementMode "repair"`).

---

## Vocabulary (`kh:`)

Namespace IRI: `http://seanchatmangpt.github.io/graphlaw/kh#`

| Property / Class | Description |
| :--- | :--- |
| `kh:Hook` | Class identifying a knowledge hook declaration. |
| `kh:Action` | Class identifying an executable action handler. |
| `kh:name` | Human-readable hook identifier string. |
| `kh:kind` | Trigger condition kind (`"sparql-select"`, `"sparql-ask"`, `"shacl"`, `"sql"`, `"threshold"`, `"window"`). |
| `kh:query` | The query string (SPARQL, SQL-in-RDF, or SHACL Turtle) evaluating the condition. |
| `kh:action` | Object reference to the `kh:Action` executed on trigger. |
| `kh:handler` | Action handler IRI (`...#sparql-construct` or `...#sql-construct`). |
| `kh:priority` | Integer firing priority (lower numbers evaluate first; ties broken by hook IRI). |
| `kh:effect` | Action effect: `"emit-delta"` (merge quads) or `"refuse"` (typed fuse). |
| `kh:reason` | Payload message surfaced in `Verdict::Refuse`. |
| `kh:enforcementMode`| SHACL mode: `"block"` (default), `"annotate"`, or `"repair"`. |
| `kh:dependsOn` | Prerequisite hook IRI establishing a dependency edge. |

---

## Supported Hook Types & Complete Examples

### 1. SPARQL SELECT Hook (`ConditionKind::SparqlSelect`)
Triggers on matching solution rows. For every distinct projected solution, pre-binds variables into the action's `CONSTRUCT` query and merges the resulting graph.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:onUserAdded a kh:Hook ;
    kh:name "user-audit-marker" ;
    kh:kind "sparql-select" ;
    kh:query "SELECT ?user ?name WHERE { ?user a ex:User ; ex:name ?name }" ;
    kh:action ex:createUserAudit ;
    kh:priority 10 ;
    kh:effect "emit-delta" .

ex:createUserAudit a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
    kh:query """
        PREFIX ex: <http://example.org/>
        CONSTRUCT {
            ?user ex:audited true ;
                  ex:auditTimestamp ?now .
        } WHERE {
            BIND(NOW() AS ?now)
        }
    """ .
```

#### Rust Evaluation
```rust
use purrdf_hooks::HookPack;
use purrdf_core::RdfDatasetBuilder;
use std::sync::Arc;

let pack_dataset = load_turtle(HOOK_TTL)?;
let hook_pack = HookPack::load(&pack_dataset)?;

let initial_state = load_turtle(STATE_TTL)?;
let materialized = hook_pack.materialize(initial_state)?;

assert_eq!(materialized.rounds, 2);
```

---

### 2. SPARQL ASK Hook (`ConditionKind::SparqlAsk`)
Evaluates a boolean condition against the dataset. If true, the action fires once for the entire state. Uses `ConditionCache` to skip re-evaluation across rounds when the store content hash is unchanged.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:checkMissingIndex a kh:Hook ;
    kh:name "missing-index-trigger" ;
    kh:kind "sparql-ask" ;
    kh:query "ASK { ?doc a ex:Document . FILTER NOT EXISTS { ?doc ex:indexed true } }" ;
    kh:action ex:emitIndexRequest ;
    kh:priority 5 ;
    kh:effect "emit-delta" .

ex:emitIndexRequest a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
    kh:query """
        PREFIX ex: <http://example.org/>
        CONSTRUCT {
            ex:IndexQueue ex:hasPendingJobs true .
        } WHERE {}
    """ .
```

---

### 3. SHACL Hook — Block Mode (`ConditionKind::Shacl`, `EnforcementMode::Block`)
Validates the current state against SHACL shapes using `purrdf-shapes`. If the dataset does not conform, immediately surfaces a `Verdict::Refuse` and halts illegal actuation (fail-closed guard).

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .

ex:adultAgeGuard a kh:Hook ;
    kh:name "adult-age-guard" ;
    kh:kind "shacl" ;
    kh:query """
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
        @prefix ex: <http://example.org/> .

        ex:UserShape a sh:NodeShape ;
            sh:targetClass ex:User ;
            sh:property [
                sh:path ex:age ;
                sh:minInclusive 18 ;
                sh:datatype xsd:integer ;
            ] .
    """ ;
    kh:action ex:refuseAction ;
    kh:enforcementMode "block" ;
    kh:priority 1 ;
    kh:effect "refuse" ;
    kh:reason "User must be at least 18 years old." .

ex:refuseAction a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
    kh:query "CONSTRUCT WHERE {}" .
```

---

### 4. SHACL Hook — Annotate Mode (`EnforcementMode::Annotate`)
Soft-fail governance: allows state transactions to proceed even when constraints fail, but records a `Verdict::Annotated` and writes the validation violation report quads directly into the state.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:profileCompletenessAudit a kh:Hook ;
    kh:name "profile-completeness-audit" ;
    kh:kind "shacl" ;
    kh:query """
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix ex: <http://example.org/> .

        ex:ProfileShape a sh:NodeShape ;
            sh:targetClass ex:Profile ;
            sh:property [
                sh:path ex:bio ;
                sh:minCount 1 ;
            ] .
    """ ;
    kh:action ex:refuseAction ;
    kh:enforcementMode "annotate" ;
    kh:priority 20 ;
    kh:effect "emit-delta" .
```

---

### 5. SHACL Hook — Repair Mode (`EnforcementMode::Repair`)
Automated remediation: when SHACL constraints fail, the hook triggers an automated repair CONSTRUCT query that synthesizes missing defaults or patches erroneous properties.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:repairMissingCurrency a kh:Hook ;
    kh:name "repair-currency-default" ;
    kh:kind "shacl" ;
    kh:query """
        @prefix sh: <http://www.w3.org/ns/shacl#> .
        @prefix ex: <http://example.org/> .

        ex:AccountShape a sh:NodeShape ;
            sh:targetClass ex:Account ;
            sh:property [
                sh:path ex:currency ;
                sh:minCount 1 ;
            ] .
    """ ;
    kh:action ex:insertDefaultCurrency ;
    kh:enforcementMode "repair" ;
    kh:priority 2 ;
    kh:effect "emit-delta" .

ex:insertDefaultCurrency a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
    kh:query """
        PREFIX ex: <http://example.org/>
        CONSTRUCT {
            ?acc ex:currency "USD" .
        } WHERE {
            ?acc a ex:Account .
            FILTER NOT EXISTS { ?acc ex:currency ?c }
        }
    """ .
```

---

### 6. SQL-in-RDF Hook (`ConditionKind::Sql`, `ActionHandler::SqlConstruct`)
Executes declarative SQL-in-RDF queries compiled via `purrdf-sql` and evaluated inside DuckDB (`purrdf-duckdb`) against columnar dataset views.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix sql: <http://seanchatmangpt.github.io/graphlaw/sql#> .
@prefix ex: <http://example.org/> .

ex:detectLedgerImbalance a kh:Hook ;
    kh:name "ledger-imbalance-detector" ;
    kh:kind "sql" ;
    kh:query """
        PREFIX sql: <http://seanchatmangpt.github.io/graphlaw/sql#>
        _:query a sql:SelectStatement ;
            sql:columns (
                [ sql:expression [ sql:column "account_id" ] ]
                [ sql:expression [ sql:aggregate "SUM" ; sql:column "amount" ] ; sql:alias "total" ]
            ) ;
            sql:from [ sql:table "ledger_entries" ] ;
            sql:groupBy ( [ sql:column "account_id" ] ) ;
            sql:having [
                sql:left [ sql:aggregate "SUM" ; sql:column "amount" ] ;
                sql:operator "<" ;
                sql:right [ sql:literal 0 ]
            ] .
    """ ;
    kh:action ex:flagOverdrawn ;
    kh:priority 15 ;
    kh:effect "emit-delta" .

ex:flagOverdrawn a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sql-construct> ;
    kh:query """
        PREFIX sql: <http://seanchatmangpt.github.io/graphlaw/sql#>
        _:repair a sql:SelectStatement ;
            sql:columns (
                [ sql:expression [ sql:column "account_id" ] ; sql:alias "subject" ]
                [ sql:literal "http://example.org/overdrawn" ; sql:alias "predicate" ]
                [ sql:literal "true" ; sql:alias "object" ]
            ) ;
            sql:from [ sql:table "ledger_entries" ] ;
            sql:groupBy ( [ sql:column "account_id" ] ) ;
            sql:having [
                sql:left [ sql:aggregate "SUM" ; sql:column "amount" ] ;
                sql:operator "<" ;
                sql:right [ sql:literal 0 ]
            ] .
    """ .
```

---

### 7. Threshold Hook (`ConditionKind::Threshold`)
Cardinality boundary check. Evaluates whether aggregate counts or numeric thresholds are exceeded, triggering refusal or remediation before resource starvation occurs.

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:burstLimitHook a kh:Hook ;
    kh:name "burst-limit-guard" ;
    kh:kind "threshold" ;
    kh:query """
        PREFIX ex: <http://example.org/>
        SELECT (COUNT(?tx) AS ?count) WHERE {
            ?tx a ex:Transaction .
        } HAVING (?count > 5000)
    """ ;
    kh:action ex:refuseAction ;
    kh:priority 1 ;
    kh:effect "refuse" ;
    kh:reason "Transaction ceiling of 5000 exceeded in single block." .
```

---

### 8. Window Hook (`ConditionKind::Window`)
Temporal sliding/tumbling window aggregation. Tracks event quads with timestamps to detect high-frequency occurrences (e.g., authentication brute-forcing).

#### RDF Specification (Turtle)
```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:rateLimitHook a kh:Hook ;
    kh:name "login-rate-limiter" ;
    kh:kind "window" ;
    kh:query """
        PREFIX ex: <http://example.org/>
        SELECT ?ip (COUNT(?attempt) AS ?attempts) WHERE {
            ?attempt a ex:FailedLogin ;
                     ex:sourceIp ?ip ;
                     ex:timestamp ?ts .
            FILTER(?ts >= (NOW() - "PT5M"^^<http://www.w3.org/2001/XMLSchema#duration>))
        } GROUP BY ?ip HAVING (?attempts >= 5)
    """ ;
    kh:action ex:blockIpAction ;
    kh:priority 5 ;
    kh:effect "emit-delta" .

ex:blockIpAction a kh:Action ;
    kh:handler <http://seanchatmangpt.github.io/graphlaw/handler#sparql-construct> ;
    kh:query """
        PREFIX ex: <http://example.org/>
        CONSTRUCT {
            ?ip ex:blocked true ;
                ex:blockedUntil ?until .
        } WHERE {
            BIND(NOW() + "PT30M"^^<http://www.w3.org/2001/XMLSchema#duration> AS ?until)
        }
    """ .
```

---

## Dependency Graphs & Execution Layers

Hooks can declare explicit dependencies with `kh:dependsOn`. `HookPack` automatically builds a `DependencyGraph` and verifies topological sortability using Kahn's algorithm:
- Detects cyclical dependencies (`HookError::Cycle`) at pack load time.
- Orders execution layers so prerequisites complete before dependent hooks evaluate.

```turtle
@prefix kh: <http://seanchatmangpt.github.io/graphlaw/kh#> .
@prefix ex: <http://example.org/> .

ex:hookA a kh:Hook ; kh:name "stage-1" ; ... .
ex:hookB a kh:Hook ; kh:name "stage-2" ; kh:dependsOn ex:hookA ; ... .
ex:hookC a kh:Hook ; kh:name "stage-3" ; kh:dependsOn ex:hookB ; ... .
```

```rust
let pack = HookPack::load(&dataset)?;
let layers = pack.graph.topological_layers().expect("no cycles");
// layers[0] -> ["stage-1"]
// layers[1] -> ["stage-2"]
// layers[2] -> ["stage-3"]
```

---

## Performance & Bounded Safety Ceilings

To ensure safety and deterministic termination within WebAssembly sandboxes:
1. `MAX_ROUNDS = 64`: Hard ceiling on fixpoint iteration rounds.
2. `MAX_FIRINGS = 10_000`: Hard ceiling on total cumulative hook firings.
3. `MAX_STATE_QUADS = 1_000_000`: Hard ceiling on dataset growth.
4. `DEFAULT_ROW_ENVELOPE = 64`: Candidate envelope limiting DuckDB relational materialization.
5. `ConditionCache`: Invalidation via 256-bit BLAKE3 hashes prevents duplicate SPARQL evaluations.
