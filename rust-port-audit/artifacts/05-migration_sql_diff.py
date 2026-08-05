import glob, os, re

REF = "/root/opencode-rs/reference/packages/core/src/database/migration"
RUST = "/root/opencode-rs/crates/oc-database/src/migration"

def canonical(sql: str) -> str:
    sql = re.sub(r"--[^\n]*", "", sql)
    sql = re.sub(r"^\s*(--[^\n]*)", "", sql)
    return re.sub(r"\s+", "", sql)

def extract_templates(text):
    """Yield contents of backtick template literals, honoring \\` escapes."""
    out = []
    i = 0
    n = len(text)
    while i < n:
        if text[i] == "`":
            j = i + 1
            buf = []
            while j < n:
                if text[j] == "\\":
                    if j + 1 < n and text[j+1] == "`":
                        buf.append("`")
                        j += 2
                        continue
                    buf.append(text[j]); j += 1; continue
                if text[j] == "`":
                    break
                buf.append(text[j]); j += 1
            out.append("".join(buf))
            i = j + 1
        else:
            i += 1
    return out

def rust_strings(text):
    """Extract Rust double-quoted strings passed to run_batch (or run_exec)."""
    out = []
    i = 0
    n = len(text)
    while i < n:
        if text.startswith('run_batch(', i) or text.startswith('run_exec(', i):
            j = text.find('"', i)
            jj = j + 1
            buf = []
            while jj < n:
                c = text[jj]
                if c == "\\":
                    if jj+1 < n:
                        nxt = text[jj+1]
                        if nxt == "n": buf.append("\n"); jj+=2; continue
                        if nxt == '"': buf.append('"'); jj+=2; continue
                        if nxt == "\\": buf.append("\\"); jj+=2; continue
                        if nxt == "t": buf.append("\t"); jj+=2; continue
                        buf.append(nxt); jj+=2; continue
                if c == '"':
                    break
                buf.append(c); jj += 1
            out.append("".join(buf))
            i = jj + 1
        else:
            i += 1
    return out

ref_files = sorted(glob.glob(os.path.join(REF, "*.ts")))
ok, diffs = 0, []
for rf in ref_files:
    base = os.path.basename(rf)[:-3]
    rs = os.path.join(RUST, "m" + base.replace("-", "_") + ".rs")
    if not os.path.exists(rs):
        diffs.append((base, "MISSING RUST FILE"))
        continue
    ref_sql = canonical("".join(extract_templates(open(rf).read())))
    rust_sql = canonical("".join(rust_strings(open(rs).read())))
    if ref_sql == rust_sql:
        ok += 1
    else:
        diffs.append((base, "SQL MISMATCH"))
        # locate first divergence
        i = 0
        while i < min(len(ref_sql), len(rust_sql)) and ref_sql[i] == rust_sql[i]:
            i += 1
        diffs.append(("  at", f"char {i}: ref={ref_sql[max(0,i-60):i+120]!r}  rust={rust_sql[max(0,i-60):i+120]!r}"))

print(f"MATCH: {ok}/{len(ref_files)}")
for d in diffs:
    print(d)
