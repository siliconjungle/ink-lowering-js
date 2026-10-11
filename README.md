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
- `u64` uses `BigInt` and wraps at 64 bits after each arithmetic operation.
  Native callers supply BigInt, including inside arrays/records. Results are
  BigInt. JSON consumers explicitly encode/decode them as decimal strings.
- `f32` is represented internally by binary32 bits. Every arithmetic operation
  rounds separately to binary32; negation flips the sign bit. Input/output uses
  Numbers or explicit `{F32Bits: bits}` for exceptional values. Zero signs,
  subnormals and passed-through exceptional payloads survive. NaN payloads
  produced by arithmetic remain engine-dependent, as they are on native CPU;
  this package does not promise cross-engine NaN payload reproducibility.
- Bool, vectors, records and nested lists retain their checked types. Callers
  supply the declared types; collection operations do not mutate inputs. Typed
  word arrays are accepted as list inputs, and map/filter/scan produce ordinary
  arrays so maps to Bool or u64 retain their element type.
- Calls, lexical bindings, map/filter, indexed map, truncating zip, ordered sum,
  right fold, scan, numeric sort, bounded repeat and lazy branches/fallbacks are
  emitted literally. No floating-point reassociation or fused reductions.
- Ordered sums emit indexed loops with the declared rounding/wrapping at each
  addition, eliminating per-element reduction callbacks.

## Trusted native caller contract

`functions.name(...args)` binds directly to the generated synchronous function
for signatures without f32 representation conversion. `call(name, args)` is
only a compatibility name lookup. Generated modules have no runtime input
validator, snapshot capture, fuel counter or cell budget. The checker still
rejects ill-typed Ink programs and checks proof applications during compilation.

Callers provide the arity, shapes and numeric ranges declared in `metadata`:
u32/i32 are integer Numbers in range, u64 is BigInt, Bool is boolean, and
collections/records contain their declared values. f32 boundaries convert
Numbers to binary32 bits and back, preserving separately rounded operations;
explicit `{F32Bits: bits}` supports exceptional values and payloads. These
representation conversions are specialised once at module initialisation.
Word-only inputs and outputs have no codec traversal or copy. An identity
function can return the original array/record; callers own aliasing and must
keep values stable during calls. Invalid caller values are outside the ABI
contract and have no specified diagnostic or result.

Pure calls have no execution timeout or collection size limit.
Resource policy belongs to the application. This is an intentional ABI change
from `ink-javascript-values-v1` to `ink-javascript-trusted-values-v2`.

- Exact Int, String, Unit, nominal IDs, enums and tagged Option/Result are supported.
- `createState()` exposes typed table-backed queries/changes, keeps,
  rollback, ordered events, outbox acknowledgement and asynchronous portable
  checkpoint/restore. Query-local Result errors do not poison a change unless
  propagated; nested failing changes do poison it even if their Result is ignored.
- Portable snapshot bytes match the reference format for the same program.
  WebCrypto supplies hashing; the host supplies durable storage and external I/O.
  Keeps use full recomputation as the baseline implementation.


State transactions retain their separate bounded host execution policy.

`lower(&CheckedModule)` returns source and a manifest binding the checked core
identity, semantics version, typed interface and emitted-source hash. The checker,
lowerer, representation conversion and JavaScript engine remain trusted implementations.
Differential testing is evidence, not a formal target-code proof.

## Validation

```sh
cargo test --locked
```

Rust tests execute emitted modules in Node and compare results with the pinned
Ink reference interpreter, including full-width integer overflow, f32 rounding,
exceptional inputs, lazy evaluation, nested collections, records, vectors,
particles, identifier hygiene, typed maps that change element type, native
identity aliasing and absence of runtime checker machinery. Separate compile-time
negative tests reject type errors and unsupported capabilities.


`lower_selected(&CheckedSelection)` retains the original checked snapshot namespace for state instances while the emitted code/manifest identify the selected module. Bare-module emission keeps that module’s own namespace; no arbitrary snapshot identity override is accepted.

Existing i32/f32 and fixed numerical vectors also cross state/action/keep/event boundaries. Scalar float comparisons use IEEE semantics; aggregate action equality compares stored float bits. Numerical checkpoints preserve payloads and signed zero. Arithmetic NaN payloads remain engine dependent.

All targets consume [ink-abi](https://github.com/siliconjungle/ink-abi) for shared
type/layout contracts. Early termination, owned-buffer transforms and helper
unfolding are checked database choices. The backend emits the resulting general
operations literally; it does not infer those transformations from source patterns.

Standalone proof-selected representations use the compile-time checked witness
and emit literal functions with native state retained between calls. Generated
applications carry no proof package, checker context, source AST or Ink runtime
dependency. See ink-lang/docs/database-representations.md and the shared
`representation_parity` conformance gate. Recursive GPU state uses the complete
host lane; device eligibility remains separate.


Generated entry points trust the caller's declared argument types, arity, shapes,
tags and canonical ranges. Bad data is unsafe and may crash, trap, corrupt state
or produce incorrect results; raw native pointers also require valid alignment,
length and lifetime. No malformed-input diagnostic or rollback is promised.
Representation conversion/JSON parsing and explicit portable checkpoint integrity
checks remain. Source/proof checking happens during compilation. Standalone
representation calls skip whole-state checkpoint validation; read-only observation
envelopes are erased when existing definitional computation establishes unchanged
logical state/version and no events.
