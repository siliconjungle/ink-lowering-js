# Ink JavaScript lowering

Literal ES module emission for checked Ink programs. This package depends only
on `ink-core`, `ink-abi`, JSON and hashing; it contains no optimisation laws, cost selection
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

State modules retain eligible scalar keeps until a checked table dependency
changes. Failed actions restore their cached before-images; successful restore
and host rollback clear caches. Composite values keep their ownership path.
`state.bind(name, {resultOnly:true})` supplies a synchronous typed entrypoint:
scalar arguments use native carriers, composite inputs are captured, results own
their mutable containers, and all committed events remain in the outbox. Normal
state invocation keeps its existing boundary. Pure representation definitions
use ordinary shallow calls and a private depth-triggered explicit-stack replay;
their private constructor carriers are immutable by construction. See the
distribution's [contract and evidence](https://github.com/siliconjungle/ink-lang/blob/main/docs/signals-state-performance.md).

- `u32` and `i32` wrap at 32 bits. Multiplication uses `Math.imul`.
- `u64` uses `BigInt` and wraps at 64 bits after each arithmetic operation.
  Native callers supply BigInt, including inside arrays/records. Results are
  BigInt. JSON consumers explicitly encode/decode them as decimal strings.
- Pure modules keep `f32` in native Numbers with exact tagged exceptional values;
  state modules retain the binary32 bit carrier. Every arithmetic operation
  rounds separately to binary32; negation preserves the exact sign and payload. Input/output uses
  Numbers or explicit `{F32Bits: bits}` for exceptional values. Zero signs,
  subnormals and passed-through exceptional payloads survive. Arithmetic NaNs
  use the canonical quiet payload `0x7fc00000`; F32 programs bind executable-core V7.
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
only a compatibility name lookup. Generated pure functions have no runtime input
validator, snapshot capture, fuel counter or cell budget. Stateful `createState`
invocation retains typed input checks and a bounded host execution policy. The checker still
rejects ill-typed Ink programs and checks proof applications during compilation.

Callers provide the arity, shapes and numeric ranges declared in `metadata`:
u32/i32 are integer Numbers in range, u64 is BigInt, Bool is boolean, and
collections/records contain their declared values. Pure f32 operations round
scalar reads and arithmetic separately with `Math.fround`;
explicit `{F32Bits: bits}` supports exceptional values and payloads. These
representation conversions are specialised once at module initialisation.
Field-only pure record consumers borrow stable input containers and avoid recursive
float packing. Whole-value escape/equality retains eager, invocation-local
capture; state inputs and results retain their separate ownership checks.
No conversion cache retains mutable host values across invocations.
Word-only inputs and outputs have no codec traversal or copy. An identity
function can return the original array/record; callers own aliasing and must
keep values stable during calls. Invalid caller values are outside the ABI
contract and have no specified diagnostic or result.

Pure calls have no execution timeout or collection size limit.
`callOwned(name, args, limits)` has the same stable, well-typed input contract as
`call`, but returns a canonical logical result with every container detached from
caller inputs. Literal typed copy functions use static field names; no JSON
algorithm recognition or runtime source generation is involved. The copy obeys
the shared ABI's byte/node/depth limits. Execution itself remains unbounded, and
declared preconditions still run. Internal owners and unsupported wire types
reject explicitly. Ordinary `call` retains its existing aliasing contract.
Compiler-derived `valueCodecs` can use literal materialisers with static field
names. Complete generic portable/compact validation runs first, including schema,
Unicode, count and resource checks. Materialisation runs synchronously and returns
owned values. Arbitrary runtime `createValueCodec` schemas retain generic decoding;
the emitted materialisers are trusted backend code, not equality certificates.
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

### Plain JavaScript objects

Generated `JsonCodec` instances now expose `fromJS(value, limits)` and
`toJS(value, options)` for plain object/array trees, without a JSON text round
trip. They are explicit host-boundary adapters to the ordinary library `Json`
type; pure calls, native lowerers and Wasm execution do not acquire an implicit
conversion. Inputs and outputs are detached and existing resource budgets apply.
Accessors, cycles, unsupported values and non-plain objects reject explicitly.
The default Number output policy rejects decimal precision loss; use
`numbers: 'bigint'` for exact integers or `numbers: 'string'` for exact tokens.
See the pinned ABI's README for the complete input and numeric contracts.

### Direct typed construction

Opaque-value codecs expose explicit `decodeFused` and `decodeCompactFused`
alternatives using the ABI's checked literal reader. It validates frames,
typed scalar payloads, counted sequences and source resource bounds during owned
construction. Selected record and sum layouts preserve their complete logical
wire nodes. The independent generic decoder remains a conformance oracle. Baseline `decode` methods retain
pre-validation until comparable lifecycle evidence selects an alternative; JSON
format invariants still belong to the JSON adapter or checked source properties.

Each `compiledValues` codec exposes a frozen `descriptor`, `resolve(type)` and
`builder(limits)` for typed stream adapters. The builder accepts source-order
`open(constructorIndex)`, `scalar(value)` and `close()` events. `finish()` returns
an opaque, module-owned handle; `abort()` discards the session. Incomplete,
ill-typed, over-budget or reentrant sessions reject. No method exposes private
composite nodes or accepts unchecked private children.

`openPath(indices)` combines unary constructor events into one frame.
`scalarPath(indices, value)` and `emptyPath(indices)` complete such paths without
an intermediate frame. Reusable paths must be frozen arrays of data properties.
Literal plans retain every source node's resource charge, even when the database
has selected record elision or a flat sum representation. Combining charges
requires every original check to succeed; potential failures replay those checks
in order. Constructor composition is fixed boundary machinery, not rewrite
selection or a source evaluator.

`JsonCodec.parseCompiled(codec, text, limits)` uses this interface directly.
It preserves exact number tokens, member order, Unicode validation, duplicate
rejection and portable bytes. Construction is opt-in: conformance does not
establish a speedup, and complete text lifecycles still require measurement.

`codec.visit(handle, visitor, limits)` emits portable-schema events through
`begin()`, `end()`, `byte(value)`, `word(value)`, `wide(value)`, `text(value)` and
`count(value)` methods. The source descriptor determines each scalar's meaning
and field order. The literal emitted writer supplies events; no AST is evaluated.
The visitor receives neither private composites nor writer callbacks. Node,
depth and portable byte budgets apply, and a visitor exception ends traversal.
This supports streaming serializers and observers without a logical tree or
binary buffer. Format-specific validation and output bounds remain the adapter's
responsibility; the visitor API alone establishes no performance claim.

`codec.reducer(factory)` binds a host algebra once to the checked source type.
It returns `reduce(handle, context, limits)`, using literal emitted traversal
rather than interpreting descriptors at each value. The factory receives a
frozen node descriptor and its index. `children` lists child indices; records
also supply source-order `fields`. Recursive references reuse their node index.
Return a function for each scalar, record, option or result, or a
`{start, step, finish}` algebra for each list/vector:

- Scalars: `(ownedScalar, context)`; Int and F32 use their public wrapper shapes.
- Records: `(...fieldResults, context)` in source field order.
- Options: `(tag, childResult, context)`, with 0 for None and 1 for Some.
- Results: `(tag, childResult, context)`, with 0 for Ok and 1 for Err.
- Collections: `start(length, context)`, `step(accumulator, childResult, index,
  context)` and `finish(accumulator, context)`.

Callbacks receive immutable scalars, fresh scalar wrappers and their own child
results. They cannot obtain private composites. Source node, depth and byte
limits apply even when an algebra discards a field. Callbacks may run before a
later resource failure; exceptions end that invocation. Reentrant invocations
have independent budgets. Host algebras remain trusted boundary code and confer
no rewrite authority. Algorithms and physical-layout proofs stay in knowledge.

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
representation calls skip whole-state checkpoint validation; query envelopes execute the literal proved physical step. Backend-specific envelope erasure has been removed.

Fixed table operations include bulk upsert/removal, shared keys, ordered direct sums and predicate counts. Generated state journals touched optional rows, caches ordered keys until membership changes, and reads live payloads on replacement. Shared-key scans probe the smaller map and sort only their result; direct reductions avoid intermediate row/map arrays. Portable snapshots retain logical key order and exclude physical caches/journals.
Pure buffer elements are immutable logical values and may share nested records
with inputs and outputs. Slot arrays remain privately owned; split-range growth
detaches. Structural equality uses declared fields and an aggregate identity
fast path, retaining stored float bits. Host state replies detach their reachable
values. See the distribution's `docs/internal-snapshots.md` for the ownership and private-delta contract.

## State value execution

State expressions use structural equality compiled from their checked types.
Lists compare length and elements; records compare declared fields; contained
floats compare stored bits, while top-level scalar floats retain IEEE equality.
Public state arguments compile their validation/capture traversal once per
schema. Every call still checks resource limits and produces owned composite
values; host values are never cached between calls. These are literal value
operations, with no algorithm or theorem-name recognizer.

`state.prepare(name, args)` validates and captures owned inputs once. Its opaque
token works only with that state instance; `state.invokePrepared(token)` reuses
the captured inputs while running preconditions, transactions and reply
detachment on every invocation. Preparation cost is separate from invocation
cost. Host transaction rollback also restores acknowledged outbox entries.

## License

Project code and documentation are [MIT licensed](LICENSE). Third-party dependencies,
vendored projects and research data retain their own licenses and notices.
