This repository owns JavaScript emission for checked Ink programs. Keep Ink's
core types and semantics unchanged. Reject unsupported capabilities explicitly.
Optimisation laws and representation choices belong in ink-knowledge;
search/cost selection belongs in ink-planner; profiling, scheduling, fallback
and bundle assembly belong in ink-runtime. Validate generated code against the
reference interpreter, including integer overflow and binary32 operation order.
Do not claim checked rewrites prove emitted JavaScript or target machine code.
