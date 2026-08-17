#!/usr/bin/env python3
"""Real Reference-vs-Rust Differential Execution Engine.

Executes the vendored OpenCode v1.18.13 reference modules (node + esbuild loader
importing real code under reference/) and the opencode-rs production functions
(cargo example harness), compares canonical JSON outputs for equality, and
records machine-verifiable paired artifacts under rust-port-audit/tui/differential/.

matched = exit codes are 0 AND the parsed canonical JSON outputs are EQUAL.
"""

import hashlib
import json
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DIFF_DIR = REPO_ROOT / "rust-port-audit" / "tui" / "differential"
LOADER_PATH = REPO_ROOT / "scripts" / "ts_loader.mjs"

NODE_REGISTER_FLAG = f"data:text/javascript,import {{ register }} from 'node:module'; import {{ pathToFileURL }} from 'node:url'; register('{LOADER_PATH.as_posix()}', pathToFileURL('{REPO_ROOT.as_posix()}/'));"

CANON_JS = """
function canon(v){
  if (v === undefined) return "null";
  if (Array.isArray(v)) return "["+v.map(canon).join(",")+"]";
  if (v && typeof v === "object") {
    return "{"+Object.keys(v).sort().map(k=>JSON.stringify(k)+":"+canon(v[k])).join(",")+"}";
  }
  return JSON.stringify(v);
}
const rgb = (c) => ({r:c.r,g:c.g,b:c.b});
"""

def ref_eval(body: str) -> str:
    return CANON_JS + body

SCENARIOS = [
    {
        "id": "001-prompt-history-parse",
        "name": "Prompt History JSONL Parse and Corruption Recovery",
        "area": "Prompt History",
        "ref_source": "reference/packages/tui/src/prompt/history.tsx",
        "behavior": "parsePromptHistory recovers valid entries around corruption, caps at MAX_HISTORY_ENTRIES",
        "ref_eval": ref_eval("""
import { parsePromptHistory } from "./reference/packages/tui/src/prompt/history.tsx";
const corrupt = '{"input":"one","parts":[]}\\nnot-json\\n{"input":"two","parts":[]}\\n';
const overflow = Array.from({length:55},(_,i)=>`{"input":"${i}","parts":[]}`).join("\\n")+"\\n";
const parsed = parsePromptHistory(corrupt).map(e=>({input:e.input,parts:e.parts??[]}));
const over = parsePromptHistory(overflow);
console.log(canon({corrupt:parsed, overflow_len:over.length, overflow_first:over[0]?.input ?? null, empty_len:parsePromptHistory("").length}));
"""),
    },
    {
        "id": "002-prompt-history-dedup",
        "name": "Prompt History Consecutive Deduplication",
        "area": "Prompt History",
        "ref_source": "reference/packages/tui/src/prompt/history.tsx",
        "behavior": "isDuplicateEntry dedupes identical consecutive entries, differentiates differing parts",
        "ref_eval": ref_eval("""
import { isDuplicateEntry } from "./reference/packages/tui/src/prompt/history.tsx";
const h1 = {input:"hello",parts:[]};
const h2 = {input:"hello",parts:[]};
const h3 = {input:"world",parts:[]};
const a = {input:"describe this",parts:[{type:"file",mime:"image/png",filename:"a.png"}]};
const b = {input:"describe this",parts:[{type:"file",mime:"image/png",filename:"b.png"}]};
console.log(canon([isDuplicateEntry(undefined,h1),isDuplicateEntry(h1,h2),isDuplicateEntry(h1,h3),isDuplicateEntry(a,b)]));
"""),
    },
    {
        "id": "003-prompt-paste-placeholders",
        "name": "Pasted Text Placeholder Expansion",
        "area": "Prompt Parts",
        "ref_source": "reference/packages/tui/src/prompt/part.ts",
        "behavior": "expandPastedTextPlaceholders replaces placeholder text with pasted content",
        "ref_eval": ref_eval("""
import { expandPastedTextPlaceholders } from "./reference/packages/tui/src/prompt/part.ts";
const parts = [{type:"text",text:"line1\\nline2",source:{text:{value:"[Pasted ~2 lines]",start:0,end:17}}}];
console.log(canon([expandPastedTextPlaceholders("[Pasted ~2 lines] tail",parts), expandPastedTextPlaceholders("plain tail",[])]));
"""),
    },
    {
        "id": "004-prompt-stash",
        "name": "Prompt Stash Parse with Cap",
        "area": "Prompt Stash",
        "ref_source": "reference/packages/tui/src/prompt/stash.tsx",
        "behavior": "parsePromptStash skips corrupt lines and keeps newest MAX_STASH_ENTRIES",
        "ref_eval": ref_eval("""
import { parsePromptStash, MAX_STASH_ENTRIES } from "./reference/packages/tui/src/prompt/stash.tsx";
const text = '{"input":"one"}\\nbad\\n{"input":"two"}\\n' + Array.from({length:MAX_STASH_ENTRIES+3},(_,i)=>`{"input":"overflow${i}"}`).join("\\n") + "\\n";
const parsed = parsePromptStash(text);
console.log(canon({len:parsed.length, first:parsed[0]?.input ?? null, max:MAX_STASH_ENTRIES}));
"""),
    },
    {
        "id": "005-keymap-leader",
        "name": "Keymap Leader Token and Base Mode Constants",
        "area": "Keymap",
        "ref_source": "reference/packages/tui/src/keymap.tsx",
        "behavior": "LEADER_TOKEN and OPENCODE_BASE_MODE constants",
        "ref_eval": ref_eval("""
import { LEADER_TOKEN, OPENCODE_BASE_MODE } from "./reference/packages/tui/src/keymap.tsx";
console.log(canon({leader_token:LEADER_TOKEN, base_mode:OPENCODE_BASE_MODE}));
"""),
    },
    {
        "id": "006-keymap-chord-timeout",
        "name": "Keybind Leader Default",
        "area": "Keybind Config",
        "ref_source": "reference/packages/tui/src/config/keybind.ts",
        "behavior": "TuiKeybind.LeaderDefault defines the default leader key",
        "ref_eval": ref_eval("""
import { LeaderDefault } from "./reference/packages/tui/src/config/keybind.ts";
console.log(canon({leader_default:LeaderDefault}));
"""),
    },
    {
        "id": "007-theme-presets",
        "name": "Registered Theme Preset Names",
        "area": "Themes",
        "ref_source": "reference/packages/tui/src/theme/index.ts",
        "behavior": "DEFAULT_THEMES registers the full preset name set",
        "ref_eval": ref_eval("""
import { DEFAULT_THEMES } from "./reference/packages/tui/src/theme/index.ts";
console.log(canon(Object.keys(DEFAULT_THEMES).slice().sort()));
"""),
    },
    {
        "id": "008-theme-preset-data",
        "name": "Theme Preset Raw Definitions (opencode/dracula/nord)",
        "area": "Themes",
        "ref_source": "reference/packages/tui/src/theme/assets/*.json",
        "behavior": "Preset defs/theme tables match the reference asset JSON",
        "ref_eval": ref_eval("""
import { DEFAULT_THEMES } from "./reference/packages/tui/src/theme/index.ts";
const pick = (n) => ({defs:DEFAULT_THEMES[n].defs ?? null, theme:DEFAULT_THEMES[n].theme ?? null});
console.log(canon({opencode:pick("opencode"), dracula:pick("dracula"), nord:pick("nord")}));
"""),
    },
    {
        "id": "009-theme-resolve",
        "name": "Theme Resolution Anchor Colors (dark)",
        "area": "Themes",
        "ref_source": "reference/packages/tui/src/theme/index.ts",
        "behavior": "resolveTheme resolves preset anchors identically",
        "ref_eval": ref_eval("""
import { DEFAULT_THEMES, resolveTheme } from "./reference/packages/tui/src/theme/index.ts";
const fields = ["primary","secondary","accent","error","warning","success","text","background"];
const anchors = (n,m) => { const t = resolveTheme(DEFAULT_THEMES[n], m); const o = {}; for (const f of fields) o[f] = rgb(t[f]); return o; };
const out = {};
for (const n of Object.keys(DEFAULT_THEMES).sort()) { out[n] = {dark: anchors(n,"dark"), light: anchors(n,"light")}; }
console.log(canon(out));
"""),
    },
    {
        "id": "010-format-duration",
        "name": "Duration Formatting Boundaries",
        "area": "Formatting",
        "ref_source": "reference/packages/tui/src/util/format.ts",
        "behavior": "formatDuration thresholds for s/m/h/d/weeks",
        "ref_eval": ref_eval("""
import { formatDuration } from "./reference/packages/tui/src/util/format.ts";
const cases = [0,1,45,59,60,61,3599,3600,86399,86400,604799,604800,1209600];
console.log(canon(cases.map(formatDuration)));
"""),
    },
    {
        "id": "011-format-collapse",
        "name": "Tool Output Collapse",
        "area": "Formatting",
        "ref_source": "reference/packages/tui/src/util/collapse-tool-output.ts",
        "behavior": "collapseToolOutput short/long/wide-truncation cases",
        "ref_eval": ref_eval("""
import { collapseToolOutput } from "./reference/packages/tui/src/util/collapse-tool-output.ts";
const long = Array.from({length:20},(_,i)=>`line ${i+1}`).join("\\n");
const short = collapseToolOutput("hello\\nworld",10,100);
const l = collapseToolOutput(long+"\\n",5,80);
const wide = collapseToolOutput("abcdefghij",10,5);
console.log(canon({short:{output:short.output,overflow:short.overflow}, long:{output:l.output,overflow:l.overflow}, wide:{output:wide.output,overflow:wide.overflow}}));
"""),
    },
    {
        "id": "012-clipboard-lookup",
        "name": "Clipboard Command Lookup Matrix",
        "area": "Clipboard",
        "ref_source": "reference/packages/tui/src/clipboard.ts",
        "behavior": "copyCommand selects the right binary per OS/Wayland/availability",
        "ref_eval": ref_eval("""
import { copyCommand } from "./reference/packages/tui/src/clipboard.ts";
const m = (os,wl,present) => copyCommand(os,wl,(n)=>present.includes(n));
console.log(canon([m("darwin",false,["osascript"]), m("linux",true,["wl-copy"]), m("linux",false,["xclip"]), m("linux",false,["xsel"]), m("win32",false,["powershell.exe"]), m("linux",false,[])]));
"""),
    },
    {
        "id": "013-editor-normalize",
        "name": "External Editor Prompt Normalization",
        "area": "Editor Integration",
        "ref_source": "reference/packages/tui/src/editor.ts",
        "behavior": "normalizePromptContent strips single trailing newline for one-line prompts only",
        "ref_eval": ref_eval("""
import { normalizePromptContent } from "./reference/packages/tui/src/editor.ts";
const cases = ["hello\\n","hello\\r\\n","a\\nb\\n","a\\nb",""];
console.log(canon(cases.map(normalizePromptContent)));
"""),
    },
    {
        "id": "014-patch-metadata",
        "name": "Apply-Patch File Metadata Parsing",
        "area": "Display Utils",
        "ref_source": "reference/packages/session-ui/src/components/apply-patch-file.ts",
        "behavior": "patchFile extracts relativePath/additions/deletions from server metadata",
        "ref_eval": ref_eval("""
import { patchFile } from "./reference/packages/session-ui/src/components/apply-patch-file.ts";
const f = patchFile({filePath:"/tmp/a.ts",relativePath:"a.ts",type:"update",patch:"Index: a.ts\\n--- a.ts\\n+++ a.ts\\n@@ -1,2 +1,2 @@\\n one\\n-two\\n+three\\n",additions:1,deletions:1});
console.log(canon(f ? [{relativePath:f.relativePath, additions:f.additions, deletions:f.deletions}] : null));
"""),
    },
    {
        "id": "015-locale-duration",
        "name": "Locale Duration Formatting",
        "area": "Locale",
        "ref_source": "reference/packages/tui/src/util/locale.ts",
        "behavior": "duration formats ms ranges",
        "ref_eval": ref_eval("""
import { duration } from "./reference/packages/tui/src/util/locale.ts";
const cases = [0,5000,65000,3723000,86400000];
console.log(canon(cases.map(duration)));
"""),
    },
    {
        "id": "016-prompt-interaction",
        "name": "Prompt Input Interaction State Machine",
        "area": "Prompt Interaction",
        "ref_source": "reference/packages/session-ui/src/v2/components/prompt-input/machine.ts",
        "behavior": "transitionPromptInputV2 state transitions for mode/popover/input triggers",
        "ref_eval": ref_eval("""
import { createPromptInputV2InteractionState, transitionPromptInputV2 } from "./reference/packages/session-ui/src/v2/components/prompt-input/machine.ts";
const empty = {prompt:[],context:{items:[]},cursor:undefined};
const persisted = {prompt:[{type:"text",content:"fix the bug"}],context:{items:[]},cursor:undefined};
const s = () => createPromptInputV2InteractionState();
const t = (e,p) => transitionPromptInputV2(s(),e,p);
const out = [
  t({type:"mode.shell"},empty),
  t({type:"mode.normal"},empty),
  t({type:"drag.enter"},empty),
  t({type:"focus.editor"},empty),
  t({type:"input.changed",value:"!"},empty),
  t({type:"input.changed",value:"fix @par"},empty),
  t({type:"input.changed",value:"/fix"},empty),
  t({type:"commands.open"},persisted),
  t({type:"commands.open"},empty),
  t({type:"popover.query",value:"re"},empty),
];
console.log(canon(out.map(x=>({state:x.state,commands:x.commands,handled:x.handled}))));
"""),
    },
    {
        "id": "017-logo",
        "name": "Home Logo Left Column",
        "area": "Home Screen",
        "ref_source": "reference/packages/tui/src/logo.ts",
        "behavior": "logo.left rows match the reference banner",
        "ref_eval": ref_eval("""
import { logo } from "./reference/packages/tui/src/logo.ts";
console.log(canon(logo.left));
"""),
    },
    {
        "id": "018-locale-titlecase",
        "name": "Locale Titlecase",
        "area": "Locale",
        "ref_source": "reference/packages/tui/src/util/locale.ts",
        "behavior": "titlecase capitalizes words",
        "ref_eval": ref_eval("""
import { titlecase } from "./reference/packages/tui/src/util/locale.ts";
const cases = ["patch metadata hunk","hello WORLD","MiXeD case Words"];
console.log(canon(cases.map(titlecase)));
"""),
    },
    {
        "id": "019-locale-truncate",
        "name": "Locale Truncation",
        "area": "Locale",
        "ref_source": "reference/packages/tui/src/util/locale.ts",
        "behavior": "truncate and truncateMiddle boundaries",
        "ref_eval": ref_eval("""
import { truncate, truncateMiddle } from "./reference/packages/tui/src/util/locale.ts";
console.log(canon([truncate("a very long line of text",10), truncate("short",10), truncateMiddle("abcdefghijklmnop",8)]));
"""),
    },
    {
        "id": "020-clipboard-wayland",
        "name": "Clipboard Wayland Selection",
        "area": "Clipboard",
        "ref_source": "reference/packages/tui/src/clipboard.ts",
        "behavior": "wl-copy selected when Wayland and present",
        "ref_eval": ref_eval("""
import { copyCommand } from "./reference/packages/tui/src/clipboard.ts";
console.log(canon(copyCommand("linux",true,(n)=>n==="wl-copy")));
"""),
    },
    {
        "id": "021-clipboard-macos",
        "name": "Clipboard macOS osascript",
        "area": "Clipboard",
        "ref_source": "reference/packages/tui/src/clipboard.ts",
        "behavior": "osascript selected on darwin",
        "ref_eval": ref_eval("""
import { copyCommand } from "./reference/packages/tui/src/clipboard.ts";
console.log(canon(copyCommand("darwin",false,(n)=>n==="osascript")));
"""),
    },
    {
        "id": "022-clipboard-x11",
        "name": "Clipboard X11 Fallback",
        "area": "Clipboard",
        "ref_source": "reference/packages/tui/src/clipboard.ts",
        "behavior": "xclip then xsel fallback on X11",
        "ref_eval": ref_eval("""
import { copyCommand } from "./reference/packages/tui/src/clipboard.ts";
console.log(canon({xclip:copyCommand("linux",false,(n)=>n==="xclip"), xsel:copyCommand("linux",false,(n)=>n==="xsel")}));
"""),
    },
    {
        "id": "023-clipboard-none",
        "name": "Clipboard Unavailable Returns undefined",
        "area": "Clipboard",
        "ref_source": "reference/packages/tui/src/clipboard.ts",
        "behavior": "no provider available yields undefined",
        "ref_eval": ref_eval("""
import { copyCommand } from "./reference/packages/tui/src/clipboard.ts";
console.log(canon(copyCommand("linux",false,()=>false)));
"""),
    },
    {
        "id": "024-patch-metadata-multi",
        "name": "Apply-Patch Multi-File Parsing",
        "area": "Display Utils",
        "ref_source": "reference/packages/session-ui/src/components/apply-patch-file.ts",
        "behavior": "patchFiles parses multiple file entries",
        "ref_eval": ref_eval("""
import { patchFiles } from "./reference/packages/session-ui/src/components/apply-patch-file.ts";
const files = patchFiles([
  {filePath:"/x/a.rs",relativePath:"a.rs",type:"update",patch:"@@ -1 +1 @@\\n-a\\n+b\\n",additions:1,deletions:1},
  {filePath:"/x/c.md",relativePath:"c.md",type:"add",patch:"@@ -0,0 +1 @@\\n+new\\n",additions:1,deletions:0},
]);
console.log(canon(files.map(f=>({relativePath:f.relativePath, additions:f.additions, deletions:f.deletions}))));
"""),
    },
    {
        "id": "025-theme-preset-data-2",
        "name": "Theme Preset Raw Definitions (catppuccin/gruvbox/tokyonight)",
        "area": "Themes",
        "ref_source": "reference/packages/tui/src/theme/assets/*.json",
        "behavior": "Preset defs/theme tables match the reference asset JSON",
        "ref_eval": ref_eval("""
import { DEFAULT_THEMES } from "./reference/packages/tui/src/theme/index.ts";
const pick = (n) => ({defs:DEFAULT_THEMES[n].defs ?? null, theme:DEFAULT_THEMES[n].theme ?? null});
console.log(canon({catppuccin:pick("catppuccin"), gruvbox:pick("gruvbox"), tokyonight:pick("tokyonight")}));
"""),
    },
    {
        "id": "026-editor-multiline",
        "name": "Editor Multiline Normalization",
        "area": "Editor Integration",
        "ref_source": "reference/packages/tui/src/editor.ts",
        "behavior": "normalizePromptContent preserves multiline endings",
        "ref_eval": ref_eval("""
import { normalizePromptContent } from "./reference/packages/tui/src/editor.ts";
const cases = ["first\\nsecond\\nthird\\n","single\\n","trailing\\n\\n\\n"];
console.log(canon(cases.map(normalizePromptContent)));
"""),
    },
]

RUST_CMD = ["cargo", "run", "-q", "-p", "oc-tui", "--example", "diff_scenarios", "--"]


def sha256_str(data: str) -> str:
    return hashlib.sha256(data.encode("utf-8")).hexdigest()


NOT_FOUND = object()


def parse_last_json(stdout: str):
    """Parse the last stdout line that is a valid JSON value."""
    for line in reversed(stdout.strip().splitlines()):
        line = line.strip()
        try:
            return json.loads(line)
        except Exception:
            continue
    return NOT_FOUND


def canonical_str(obj) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def run_differential() -> int:
    DIFF_DIR.mkdir(parents=True, exist_ok=True)
    passed = 0
    failed = 0
    failures = []

    print(f"=== Executing {len(SCENARIOS)} Real Reference-vs-Rust Differential Scenarios ===")

    for sc in SCENARIOS:
        sc_dir = DIFF_DIR / sc["id"]
        sc_dir.mkdir(parents=True, exist_ok=True)

        ref_cmd = ["node", "--import", NODE_REGISTER_FLAG, "-e", sc["ref_eval"]]
        rust_cmd = RUST_CMD + [sc["id"]]

        ref_t0 = time.time()
        ref_proc = subprocess.run(ref_cmd, cwd=str(REPO_ROOT), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        ref_duration_ms = int((time.time() - ref_t0) * 1000)

        rust_t0 = time.time()
        rust_proc = subprocess.run(rust_cmd, cwd=str(REPO_ROOT), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        rust_duration_ms = int((time.time() - rust_t0) * 1000)

        ref_out = parse_last_json(ref_proc.stdout)
        rust_out = parse_last_json(rust_proc.stdout)

        # The reference prints the bare payload; the Rust harness wraps it as
        # {"scenario": id, "result": payload}. Compare payload-to-payload.
        rust_payload = rust_out.get("result") if isinstance(rust_out, dict) and "result" in rust_out else rust_out
        outputs_equal = (
            ref_out is not NOT_FOUND
            and rust_payload is not NOT_FOUND
            and ref_out == rust_payload
        )
        matched = ref_proc.returncode == 0 and rust_proc.returncode == 0 and outputs_equal

        ref_canon = canonical_str(ref_out) if ref_out is not NOT_FOUND else ""
        rust_canon = canonical_str(rust_payload) if rust_payload is not NOT_FOUND else ""
        ref_hash = sha256_str(ref_canon)
        rust_hash = sha256_str(rust_canon)

        status = "PASS" if matched else "FAIL"
        if matched:
            passed += 1
            print(f"  [{sc['id']}] {sc['name']} -> PASS (ref {ref_duration_ms}ms, rust {rust_duration_ms}ms)")
        else:
            failed += 1
            reason = []
            if ref_proc.returncode != 0:
                reason.append(f"ref exit {ref_proc.returncode}: {ref_proc.stderr.strip()[:200]}")
            if rust_proc.returncode != 0:
                reason.append(f"rust exit {rust_proc.returncode}: {rust_proc.stderr.strip()[:200]}")
            if not outputs_equal:
                reason.append(f"outputs differ:\n    ref : {ref_canon[:300]}\n    rust: {rust_canon[:300]}")
            failures.append((sc["id"], reason))
            print(f"  [{sc['id']}] {sc['name']} -> FAIL", file=sys.stderr)
            for r in reason:
                print(f"    {r}", file=sys.stderr)

        input_hash = sha256_str(sc["ref_eval"])

        with open(sc_dir / "scenario.json", "w", encoding="utf-8") as f:
            json.dump({
                "scenario_id": sc["id"],
                "name": sc["name"],
                "area": sc["area"],
                "behavior": sc["behavior"],
                "reference_source": sc["ref_source"],
                "reference_command": ref_cmd[:2] + ["--import", "<ts_loader>", "-e", "<real import eval; see reference-frame.txt>"],
                "rust_command": rust_cmd,
            }, f, indent=2)

        with open(sc_dir / "reference-frame.txt", "w", encoding="utf-8") as f:
            f.write("=== OpenCode v1.18.13 Vendored Reference Process Frame ===\n")
            f.write(f"Scenario: {sc['id']} - {sc['name']}\n")
            f.write(f"Reference Source File: {sc['ref_source']}\n")
            f.write(f"Executed Command: node --import <ts_loader.mjs> -e '<imports {sc['ref_source']} — full eval below>'\n")
            f.write(f"Eval:\n{sc['ref_eval']}\n")
            f.write(f"Exit Code: {ref_proc.returncode}\n")
            f.write(f"Duration: {ref_duration_ms} ms\n")
            f.write(f"Timestamp: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(ref_t0))}\n")
            f.write(f"SHA-256 Output Hash: {ref_hash}\n")
            f.write(f"Output:\n{ref_canon}\n")
            if ref_proc.stderr.strip():
                f.write(f"Stderr:\n{ref_proc.stderr.strip()}\n")

        with open(sc_dir / "rust-frame.txt", "w", encoding="utf-8") as f:
            f.write("=== opencode-rs Cargo Process Execution Frame ===\n")
            f.write(f"Scenario: {sc['id']} - {sc['name']}\n")
            f.write(f"Executed Command: {' '.join(rust_cmd)}\n")
            f.write(f"Exit Code: {rust_proc.returncode}\n")
            f.write(f"Duration: {rust_duration_ms} ms\n")
            f.write(f"Timestamp: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(rust_t0))}\n")
            f.write(f"SHA-256 Output Hash: {rust_hash}\n")
            f.write(f"Output:\n{rust_canon}\n")
            if rust_proc.stderr.strip():
                f.write(f"Stderr:\n{rust_proc.stderr.strip()}\n")

        with open(sc_dir / "result.json", "w", encoding="utf-8") as f:
            json.dump({
                "scenario_id": sc["id"],
                "status": status,
                "matched": matched,
                "outputs_equal": outputs_equal,
                "reference_source": sc["ref_source"],
                "reference_exit_code": ref_proc.returncode,
                "rust_exit_code": rust_proc.returncode,
                "reference_output": ref_out,
                "rust_output": rust_out,
                "input_sha256": input_hash,
                "reference_output_sha256": ref_hash,
                "rust_output_sha256": rust_hash,
                "hashes_equal": ref_hash == rust_hash,
                "reference_duration_ms": ref_duration_ms,
                "rust_duration_ms": rust_duration_ms,
                "executed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            }, f, indent=2)

    print(f"\nSummary: {passed} passed, {failed} failed.")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(run_differential())
