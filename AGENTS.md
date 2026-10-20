This repository owns JavaScript emission for checked Ink programs. Keep Ink's
core types and semantics unchanged. Reject unsupported capabilities explicitly.
Optimisation laws and representation choices belong in ink-knowledge;
search/cost selection belongs in ink-planner; profiling, scheduling, fallback
and bundle assembly belong in ink-runtime. Validate generated code against the
reference interpreter, including integer overflow and binary32 operation order.
Do not claim checked rewrites prove emitted JavaScript or target machine code.

Immutable keep retention is fixed deterministic execution support. Derive its
dependencies from the core's checked effects; invalidate on concrete root writes,
restore cached before-images on action failure and host rollback, and clear caches
on explicit restore. Exclude owned Buffer/Map descendants. Retained composites
remain private immutable source values: pure parameters never gain unique storage,
and every public boundary still returns detached owned values. Never retain mutable
host containers or cache thrown errors; a source Result Err is an ordinary value. Typed
bound calls may skip scalar ingress checks, but must capture composite inputs,
return owned outputs, preserve transactions and retain all committed events.
Private first-order constructor nodes are immutable by construction. Direct
recursive calls may replay only on the private depth sentinel, using the existing
explicit call stack; other errors propagate. Use the core term visitor and retain
stack-only emission when the optional lane exceeds the source-size limit.

Sealed pure-call preparation may delegate to bounded, lazily created helpers.
Preserve per-syntactic-site caches and transitive parameter stability masks;
arguments execute once in source order. Stable checks stay at their original lazy
position and thrown failures retry. Bound helper preparation depth (sixteen),
source/body work and emitted size; missing factories retain ordinary calls.
Retained owners must satisfy the core readonly-loan analysis, and no cached/input
value gains unique storage. Changing model weights/context must run fresh primals;
opacity never authenticates a prepared graph at an ABI boundary. This is literal
execution support, not rewrite or profitability authority.

Sequential value-loop preparation carries lexical stability into callbacks;
iteration indices, elements and carried state always have dynamic provenance.
Preserve shadowing, zero-iteration laziness, argument order and simultaneous
updates. `fold_until` executes its triggering step before stopping;
`repeat_until` tests before its step. Strict operand traversal must not lift
short-circuit operands or sum callbacks out of their branches. Retain ordinary
execution for owned loop state/captures, unsupported forms and bounded analysis
failure. The opportunity scan (512 visits, depth 32) only avoids useless wrappers;
it supplies neither semantic facts nor a profitable-selection claim. Instrument
ordinary and residual invocation bodies when measuring source validation; a
factory's construction is not a validation execution.

Mixed immutable records carry bounded field provenance through literal constructors,
projections and helper returns. Keep returned-value provenance separate from the
work needed to produce it: unused dynamic arguments/fields, guards and failures
still execute before any cached projection. Iteration bindings stay dynamic.
Mask sharing uses only private captured policies, never source/host object access.
Bound expanded alias summaries as well as traversal; exhaustion retains ordinary
execution. Do not raise source limits to accommodate verbose optional masks.

Optional/result callback preparation preserves selected-arm laziness and receiver
execution once. Only a complete immutable captured sum fixes its payload; changing
sums keep dynamic payloads, while a None arm's Unit is constant. Loop arguments
stay dynamic. Use checked sum views/constructors and ordinary ownership fallback;
strict defaults and thrown failure retry keep source behavior. Bounded opportunity
conditions and structural work estimates affect only explicit preparation, never
proof/evidence authority. Keep direct ordinary calls for non-reusable captured
policies, and share private policy predicates without evaluating source work.

Local record wrappers can cross bounded helper expansion and value loops,
including checked affine Buffer/Map fields. Preserve each owner's private handle
and source moves/readonly loans; never infer extra uniqueness from splitting a
wrapper. Arguments/fields evaluate once before the callee; aliases, worker
captures and simultaneous updates retain snapshots. Expanded List parameters
stay shared unless a direct actual argument carries checked private transfer
facts; splitting a record wrapper alone grants none. Preserve recursive/codec and bounded
expansion fallback, account for returned expression
bytes, and reserve stack headroom for core visitors. Failed optional emission
restores its lines/callbacks/type helpers/field mappings/fresh names and ordinary
admission budget. Structural expansion grants no equality or profitability fact.
Record merges inside expanded helpers evaluate only the selected branch and
assign fresh field slots after its work. Whole-value uses rebuild normally;
mutable merge slots must never become immutable-number or alias facts by name.
Direct checked Buffer/Map parameters and records containing owners may also
expand without an already split argument. Retain the same owner handles and
callee-local List sharing analysis. Recursive helpers, exhausted body/depth/source
budgets and unsuccessful optional expansion retain ordinary calls; no bounds
are raised. Owned-parameter functions remain internal to the temporary ABI.

Normalized F32 carriers are rounded Numbers or exact NaN tags, not Number/range
facts. Normalize used scalar parameters at entry; borrowed composite fields keep
each first typed read at its source position, including repeated/throwing getters.
Scalar loop slots qualify only when initialization and every subsequent store
preserve the carrier; they still need mutable-value snapshots. Restore normalized
facts on optional-helper rollback with fresh names. Stateful bit carriers remain
separate. This grants no arithmetic rewrite, ownership or profitability authority.

Bit-carrier modules may keep adjacent F32 arithmetic and fresh immutable scalar
arithmetic locals as rounded Numbers. Every source operation still rounds in its
original grouping; opaque calls, negation, containers, bit observations, worker
captures and state/result boundaries retain exact bits. Arithmetic NaNs become
canonical when encoded; copied payloads and signed zero remain exact. Never label
parameters, mutable loop/call/branch slots or arbitrary field reads as these Number
locals. Preserve leaf snapshots and source-order failures, existing node/depth
limits, bounded optional opportunity scans and all facts on helper rollback.
These are invocation-local carriers, not retained values or range premises.

Eligible stateful modules may use native F32 pure helpers through typed private
state bridges. Actions, keeps, events, equality and snapshots retain bit storage;
bound scalar ingress and state outputs must explicitly use that representation.
Stateful-module pure/compiled entries capture normalized descendants before
execution, so their helpers may reuse normalized fields without claiming finite
Numbers. Keep pure-only borrowed getters lazy. Preserve NaN payloads, signed zero,
rounding points, ownership, and exact histories/checkpoints. Owned/recursive float
bridge shapes or bounded emission exhaustion retain the old bit lane. This is
literal representation machinery, with no equality or profitability authority.

Private evaluated constructors may use bounded scalar-record helper signatures.
Keep public borrowed getter demand/multiplicity, source-order arguments and unused
fields, mutable loop/merge snapshots, ownership and original exports. Shared
straight-line IEEE bindings reuse identical computed carriers only; retain zero
products, distinct rounding points, NaN payloads and signed zero. Exclude owned
and recursive outline shapes; register destination/prepared helpers before final
emission. Reserve full ordinary wrappers and restore all facts on optional failure.
No differentiation name, finite-range assumption or algebraic identity is involved.

Fixed-arity JavaScript scalar-result public adapters and owned/destination adapters preserve typed conversion order, contracts,
exact floats and invocation-local schema memoization. Primitive scalar arguments
need no memo; multiple converted object inputs and converted composites retain it.
Unusual arity and exhausted adapter byte bounds keep the compact ordinary path.
Prepared destination calls reuse existing sealed handles and lexical caches,
never mutable host inputs. Scalar terminal lists can write directly; structured
elements detach deeply with ordinary public floats. Destination capacity/disjointness
remain trusted and partial writes are possible on failure; bounded canonical
codec materialization is separate. No retained scratch pool, equality premise,
range fact or profitable selection authority follows. See docs/prepared-destinations.md
in the distribution and the call_boundaries conformance tests.

Generated copier namespaces rename helper symbols only; preserve quoted source
field/type names and schema literals verbatim. Do not text-replace whole emitted
schemas or copier bodies to change private namespaces.

Sequential stages may reuse only outer List arrays certified unique/dead by core.
Bounded literal helper expansion can transfer those exact facts to corresponding
List parameters; aliases, captures and pending earlier-operand loans still block
reuse. Keep whole stages eager and ordered. Residual destinations share the same
lexical caches as ordinary calls, copy structured outputs deeply, and retain the
ordinary callback body when it borrows owned captures. No retained scratch pool,
extra element ownership, fusion/rewriting authority or larger expansion limits.
