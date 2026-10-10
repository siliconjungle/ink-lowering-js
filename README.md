# Ink JavaScript lowering

Literal ES module emission for checked Ink programs. This package depends only
on `ink-core`, JSON and hashing; it contains no optimisation laws, cost selection
or browser scheduling. Those responsibilities belong to `ink-knowledge`,
`ink-planner` and `ink-runtime`.

The [Ink distribution](https://github.com/siliconjungle/ink-lang) pins this
repository and exposes:

```sh
ink build examples/particles.ink --target javascript -o particles.mjs
```

```js
import { functions, call, metadata } from './particles.mjs';
const velocities = functions.kick([[1, 2, 3]], 0.5, [0, -2, 0]);
// [[1, 1, 3]]
```

The module contains generated JavaScript functions, not an AST interpreter. It
works in browsers and Node without Wasm, native compilers or a GPU. A browser
still needs a compiler wrapper to accept newly edited Ink source.

## Semantics and boundary

- `u32` and `i32` wrap at 32 bits. Multiplication uses `Math.imul`.
- `u64` uses `BigInt` and wraps at 64 bits after each arithmetic operation. Host
  inputs accept BigInt, safe integer Number or a canonical decimal string.
  Results are BigInt. JSON callers must explicitly encode them as strings; unsafe
  Numbers, coercible booleans, hexadecimal strings and out-of-range values reject.
- `f32` is represented internally by binary32 bits. Every arithmetic operation
  rounds separately to binary32; negation flips the sign bit. Input/output uses
  finite Numbers or explicit `{F32Bits: bits}` for exceptional values. Zero signs,
  subnormals and passed-through exceptional payloads survive. NaN payloads
  produced by arithmetic remain engine-dependent, as they are on native CPU;
  this package does not promise cross-engine NaN payload reproducibility.
- Bool, vectors, records and nested lists retain their checked types. Host
  inputs are captured and validated; collection operations do not mutate them.
- Calls, lexical bindings, map/filter, indexed map, truncating zip, ordered sum,
  right fold, scan, numeric sort, bounded repeat and lazy branches/fallbacks are
  emitted literally. No floating-point reassociation or fused reductions.
- Exact Int, String, Unit, nominal IDs, enums and tagged Option/Result are supported.
- `createState()` exposes typed table-backed queries/changes, keeps,
  rollback, ordered events, outbox acknowledgement and asynchronous portable
  checkpoint/restore. Query-local Result errors do not poison a change unless
  propagated; nested failing changes do poison it even if their Result is ignored.
- Portable snapshot bytes match the reference format for the same program.
  WebCrypto supplies hashing; the host supplies durable storage and external I/O.
  Keeps use full recomputation as the baseline implementation.


Ingress/output have a one-million-cell limit. Calls and collection iterations
share a one-million-step pure execution budget; state invocation uses 100 million steps. This is a host resource policy, not
an optimisation rule or the reference interpreter's exact fuel accounting.

`lower(&CheckedModule)` returns source and a manifest binding the checked core
identity, semantics version, typed interface and emitted-source hash. The checker,
lowerer, value codec and JavaScript engine remain trusted implementations.
Differential testing is evidence, not a formal target-code proof.

## Validation

```sh
cargo test --locked
```

Rust tests execute emitted modules in Node and compare results with the pinned
Ink reference interpreter, including full-width integer overflow, f32 rounding,
exceptional inputs, lazy evaluation, nested collections, records, vectors,
particles, identifier hygiene, invalid inputs and resource limits.

`lower_selected(&CheckedSelection)` retains the original checked snapshot namespace for state instances while the emitted code/manifest identify the selected module. Bare-module emission keeps that module’s own namespace; no arbitrary snapshot identity override is accepted.
