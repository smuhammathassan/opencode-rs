import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import esbuild from 'esbuild';

const MOCK_OPENTUI = `
export class RGBA {
  constructor(r=0, g=0, b=0, a=1) {
    this.r = r; this.g = g; this.b = b; this.a = a;
  }
  static fromHex(hex) {
    const c = parseInt(hex.replace('#', ''), 16);
    return new RGBA((c >> 16) & 255, (c >> 8) & 255, c & 255, 1);
  }
}
export class SyntaxStyle {}
export class InputRenderable {}
export class TextareaRenderable {}
`;

const MOCK_SOLID = `
export function onMount() {}
export function createStore(init = {}) { return [init, () => {}]; }
export function produce(fn) { return fn; }
export function unwrap(x) { return x; }
export function createContext() { return {}; }
export function useContext() { return {}; }
export function Show(props) { return props.children; }
export function For(props) { return props.children; }
export function createSignal(val) { return [() => val, () => {}]; }
export function createMemo(fn) { return fn; }
export function createEffect(fn) {}
export function createComponent(Comp, props) { return typeof Comp === 'function' ? Comp(props) : props; }
`;

export async function resolve(specifier, context, defaultResolve) {
  if (specifier === '@opentui/core') {
    return { url: 'data:text/javascript,' + encodeURIComponent(MOCK_OPENTUI), shortCircuit: true };
  }
  if (specifier.startsWith('@opentui/keymap')) {
    const keymapShim = 'export function createBindingLookup() { return { get: () => [], gather: () => [] }; } export function stringifyKeyStroke() { return ""; } export function formatCommandBindings() {} export function formatKeySequence() {} export function KeymapProvider() {} export function useKeymap() { return {}; } export function useKeymapSelector() {} export function useBindings() {} export function registerBackspacePopsPendingSequence() {} export function registerBaseLayoutFallback() {} export function registerCommaBindings() {} export function registerEscapeClearsPendingSequence() {} export function registerManagedTextareaLayer() {} export function registerTimedLeader() {}';
    return { url: 'data:text/javascript,' + encodeURIComponent(keymapShim), shortCircuit: true };
  }
  if (specifier === 'effect' || specifier.startsWith('effect/')) {
    const effectShim = 'const makeSchema = (val) => new Proxy(Object.assign({ value: val }, { annotate: () => makeSchema(val), check: () => makeSchema(val) }), { get(target, prop) { if (prop in target) return target[prop]; return (...args) => makeSchema(args); } }); export const Effect = { gen: (fn) => fn(), succeed: (x) => x, void: undefined }; export const Context = { Service: () => () => class {} }; export const Layer = { effect: () => ({}) }; export const Schema = new Proxy({ optional: (s) => makeSchema(s), Boolean: makeSchema("boolean"), String: makeSchema("string"), Number: makeSchema("number"), Int: makeSchema("int"), Array: (s) => makeSchema(s), Tuple: (...args) => makeSchema(args), mutable: (s) => s, Struct: (s) => makeSchema(s), StructWithRest: (s) => makeSchema(s), Union: (...args) => makeSchema(args), Record: () => makeSchema({}), Literal: (...args) => makeSchema(args), Literals: (...args) => makeSchema(args), decodeUnknownSync: () => (x) => x, Class: () => class {} }, { get(target, prop) { if (prop in target) return target[prop]; return (...args) => makeSchema(args); } }); export default {};';
    return {
      url: 'data:text/javascript,' + encodeURIComponent(effectShim),
      shortCircuit: true,
    };
  }
  if (specifier === 'solid-js' || specifier === 'solid-js/store') {
    return { url: 'data:text/javascript,' + encodeURIComponent(MOCK_SOLID), shortCircuit: true };
  }
  if (specifier.startsWith('@opencode-ai/')) {
    return { url: 'data:text/javascript,export default {};', shortCircuit: true };
  }

  const { parentURL } = context;
  if (specifier.startsWith('./') || specifier.startsWith('../') || specifier === '.' || specifier === '..') {
    if (parentURL && parentURL.startsWith('file:')) {
      const parentDir = path.dirname(fileURLToPath(parentURL));
      const candidate = path.resolve(parentDir, specifier);
      if (fs.existsSync(candidate + '.ts')) {
        return defaultResolve(pathToFileURL(candidate + '.ts').href, context);
      }
      if (fs.existsSync(candidate + '.tsx')) {
        return defaultResolve(pathToFileURL(candidate + '.tsx').href, context);
      }
      if (fs.existsSync(candidate) && fs.statSync(candidate).isDirectory()) {
        if (fs.existsSync(path.join(candidate, 'index.tsx'))) {
          return defaultResolve(pathToFileURL(path.join(candidate, 'index.tsx')).href, context);
        }
        if (fs.existsSync(path.join(candidate, 'index.ts'))) {
          return defaultResolve(pathToFileURL(path.join(candidate, 'index.ts')).href, context);
        }
      }
      if (fs.existsSync(path.join(candidate, 'index.ts'))) {
        return defaultResolve(pathToFileURL(path.join(candidate, 'index.ts')).href, context);
      }
      if (fs.existsSync(path.join(candidate, 'index.tsx'))) {
        return defaultResolve(pathToFileURL(path.join(candidate, 'index.tsx')).href, context);
      }
      if (fs.existsSync(candidate) && !fs.statSync(candidate).isDirectory()) {
        return defaultResolve(pathToFileURL(candidate).href, context);
      }
    }
  }
  return defaultResolve(specifier, context);
}

export async function load(url, context, defaultLoad) {
  if (url.startsWith('file:') && (url.endsWith('.tsx') || url.endsWith('.ts') || url.endsWith('.json'))) {
    const filePath = fileURLToPath(url);
    if (url.endsWith('.json')) {
      const jsonContent = fs.readFileSync(filePath, 'utf-8');
      return {
        format: 'json',
        source: jsonContent,
        shortCircuit: true,
      };
    }
    const source = fs.readFileSync(filePath, 'utf-8');
    const result = esbuild.transformSync(source, {
      loader: url.endsWith('.tsx') ? 'tsx' : 'ts',
      format: 'esm',
      sourcemap: 'inline',
    });
    return {
      format: 'module',
      source: result.code,
      shortCircuit: true,
    };
  }
  return defaultLoad(url, context);
}
