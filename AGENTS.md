This repository owns JavaScript emission for checked Ink programs. Keep Ink's
core types and semantics unchanged. Reject unsupported capabilities explicitly.
Optimisation laws and representation choices belong in ink-knowledge;
search/cost selection belongs in ink-planner; profiling, scheduling, fallback
and bundle assembly belong in ink-runtime. Validate generated code against the
reference interpreter, including integer overflow and binary32 operation order.
Do not claim checked rewrites prove emitted JavaScript or target machine code.

Scalar keep retention is fixed deterministic execution support. Derive its
dependencies from the core's checked effects; invalidate on concrete root writes,
restore cached before-images on action failure, and clear caches on restore/host
rollback. Never retain mutable host containers or cache thrown errors. Typed
bound calls may skip scalar ingress checks, but must capture composite inputs,
return owned outputs, preserve transactions and retain all committed events.
Private first-order constructor nodes are immutable by construction. Direct
recursive calls may replay only on the private depth sentinel, using the existing
explicit call stack; other errors propagate. Use the core term visitor and retain
stack-only emission when the optional lane exceeds the source-size limit.
