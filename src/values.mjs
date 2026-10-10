function __in(v,t,b){b.depth=(b.depth??0)+1;if(b.depth>128)__fail('Ink input nesting limit');try{return __inValue(v,t,b);}finally{b.depth--;}}
function __out(v,t,b){b.depth=(b.depth??0)+1;if(b.depth>128)__fail('Ink output nesting limit');try{return __outValue(v,t,b);}finally{b.depth--;}}
// Ink value representation, not JavaScript's implicit numeric coercions.
const __view = new DataView(new ArrayBuffer(4));
const __fail = message => { throw new TypeError(message); };
const __float = bits => { __view.setUint32(0, bits, true); return __view.getFloat32(0, true); };
const __bits = number => { __view.setFloat32(0, number, true); return __view.getUint32(0, true); };
const __array = value => Array.isArray(value) || (ArrayBuffer.isView(value) && typeof value.length === 'number');
function __inValue(value, type, budget) {
  if (--budget.cells < 0) __fail('Ink input cell budget exceeded');
  if(type.kind==='ref')return __in(value,__namedTypes[type.name],budget);
  switch (type.kind) {
    case 'unit': if(value!==null)__fail('expected Unit');return null;
    case 'string': if(typeof value!=='string')__fail('expected String');if(new TextDecoder('utf-8',{ignoreBOM:true}).decode(new TextEncoder().encode(value))!==value)__fail('invalid Unicode string');return value;
    case 'int': {
      const x=value && typeof value==='object' && Object.keys(value).length===1 && typeof value.Int==='string' ? value.Int : value;
      if(!(typeof x==='bigint'||typeof x==='number'&&Number.isSafeInteger(x)||typeof x==='string'&&/^-?(0|[1-9][0-9]*)$/.test(x)))__fail('expected exact Int');return BigInt(x);
    }
    case 'id':if(typeof value!=='string'||! /^[0-9a-fA-F]{32}$/.test(value))__fail('expected 128-bit ID');return value.toLowerCase();
    case 'enum':if(!type.variants.includes(value))__fail('invalid enum variant');return value;
    case 'option': case 'result': {
      if(!value||typeof value!=='object'||Object.keys(value).length!==1)__fail('expected one variant tag');
      if(type.kind==='option'){if(Object.hasOwn(value,'Some'))return {Some:__in(value.Some,type.element,budget)};if(Object.hasOwn(value,'None')&&value.None===null)return {None:null};}
      else {if(Object.hasOwn(value,'Ok'))return {Ok:__in(value.Ok,type.ok,budget)};if(Object.hasOwn(value,'Err'))return {Err:__in(value.Err,type.error,budget)};}
      __fail('invalid variant tag');
    }
    case 'u64': {
      if (!(typeof value === 'bigint' || (typeof value === 'number' && Number.isSafeInteger(value)) ||
            (typeof value === 'string' && /^(0|[1-9][0-9]{0,19})$/.test(value)))) __fail('u64 requires BigInt, exact safe Number or decimal string');
      const n = BigInt(value);
      if (n < 0n || n > 18446744073709551615n) __fail('u64 outside range');
      return n;
    }
    case 'u32': case 'i32':
      if (typeof value !== 'number' || !Number.isInteger(value) ||
          value < (type.kind === 'u32' ? 0 : -2147483648) ||
          value > (type.kind === 'u32' ? 4294967295 : 2147483647)) __fail(`${type.kind} outside range`);
      return value === 0 ? 0 : value;
    case 'bool':
      if (typeof value !== 'boolean') __fail('expected Bool');
      return value;
    case 'f32':
      if (typeof value === 'number' && Number.isFinite(value)) {
        const b = __bits(value);
        if (!Number.isFinite(__float(b))) __fail('f32 input overflow');
        return b;
      }
      if (value && typeof value === 'object' && Object.keys(value).length === 1 &&
          Object.hasOwn(value, 'F32Bits') && Number.isInteger(value.F32Bits) &&
          value.F32Bits >= 0 && value.F32Bits <= 4294967295) return value.F32Bits;
      return __fail('expected finite f32 or explicit F32Bits');
    case 'list': case 'vector':
      if (!__array(value)) __fail('expected array');
      if (type.kind === 'vector' && value.length !== type.size) __fail('vector dimension mismatch');
      if (value.length > budget.cells) __fail('Ink input cell budget exceeded');
      return Array.from(value, v => __in(v, type.element, budget));
    case 'record':
      if (!value || typeof value !== 'object' || __array(value) || Object.keys(value).length !== type.fields.length)
        __fail('record field count mismatch');
      return Object.fromEntries(type.fields.map(f => {
        if (!Object.hasOwn(value, f.name)) __fail('missing record field');
        return [f.name, __in(value[f.name], f.type, budget)];
      }));
    default: return __fail('unsupported Ink JavaScript type');
  }
}
function __outValue(value, type, budget) {
  if (--budget.cells < 0) __fail("Ink output cell budget exceeded");
  if(type.kind==='ref')return __out(value,__namedTypes[type.name],budget);
  switch (type.kind) {
    case 'int': return {Int:value.toString()};
    case 'option':return Object.hasOwn(value,'Some')?{Some:__out(value.Some,type.element,budget)}:{None:null};
    case 'result':return Object.hasOwn(value,'Ok')?{Ok:__out(value.Ok,type.ok,budget)}:{Err:__out(value.Err,type.error,budget)};
    case 'f32': { const n = __float(value); return Number.isFinite(n) ? n : {F32Bits:value}; }
    case 'list': case 'vector': return value.map(v => __out(v, type.element, budget));
    case 'record': return Object.fromEntries(type.fields.map(f => [f.name, __out(value[f.name], f.type, budget)]));
    default: return value;
  }
}
const __u64 = n => BigInt.asUintN(64, n);
function __div(a, b, fallback, signed, remainder) {
  if (b === 0 || b === 0n || (signed && a === -2147483648 && b === -1)) return fallback();
  return typeof a === 'bigint' ? (remainder ? a % b : a / b) :
    ((remainder ? a % b : Math.trunc(a / b)) + 0); // integer zero has no sign
}
function __at(xs, index, fallback) { return index < xs.length ? xs[index] : fallback(); }
function __zip(xs, ys, fn) {
  const n = Math.min(xs.length, ys.length), out = new Array(n);
  for (let i = 0; i < n; i++) out[i] = fn(xs[i], ys[i]);
  return out;
}
function __repeat(n, initial, fn) { let value = initial; for (let i = 0; i < n; i++) value = fn(i, value); return value; }
function __foldr(xs, initial, fn) { let value = initial; for (let i = xs.length - 1; i >= 0; i--) value = fn(xs[i], value); return value; }
function __scan(xs, zero, add) { let value = zero; return xs.map(x => value = add(value, x)); }
const __sort = xs => xs.slice().sort((a,b) => a < b ? -1 : a > b ? 1 : 0);
