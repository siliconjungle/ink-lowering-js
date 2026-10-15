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
