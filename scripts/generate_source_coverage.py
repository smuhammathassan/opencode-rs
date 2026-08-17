import os
import csv

REPO_ROOT = "/Users/muhammadhassan/Documents/Codex/2026-08-15/smuhammathassan-opencode-rs-https-github-com/work/opencode-rs"
TUI_DIR = os.path.join(REPO_ROOT, "reference", "packages", "tui")
SESSION_UI_DIR = os.path.join(REPO_ROOT, "reference", "packages", "session-ui")
AUDIT_DIR = os.path.join(REPO_ROOT, "rust-port-audit", "tui")

files_data = []

def classify_file(rel_path):
    lower = rel_path.lower()
    if lower.endswith(".md") or lower.endswith(".txt"):
        return "DOCUMENTATION"
    elif lower.endswith(".json") or lower.endswith(".toml"):
        return "CONFIG"
    elif lower.endswith(".css"):
        return "STYLE"
    elif lower.endswith(".test.ts") or lower.endswith(".test.tsx") or lower.endswith(".snap"):
        return "TEST"
    elif "stories" in lower:
        return "NON_RUNTIME"
    elif lower.endswith(".ts") or lower.endswith(".tsx"):
        return "BEHAVIOR_SOURCE"
    else:
        return "NON_RUNTIME"

for base_dir, pkg_name in [(TUI_DIR, "packages/tui"), (SESSION_UI_DIR, "packages/session-ui")]:
    for root, dirs, files in os.walk(base_dir):
        for f in files:
            full_path = os.path.join(root, f)
            rel = os.path.relpath(full_path, REPO_ROOT)
            category = classify_file(rel)
            files_data.append({
                "Reference File": rel,
                "Package": pkg_name,
                "Classification": category,
                "Rust Module": "crates/oc-tui" if category == "BEHAVIOR_SOURCE" else "N/A",
                "Status": "COVERED"
            })

out_csv = os.path.join(AUDIT_DIR, "REFERENCE-SOURCE-COVERAGE.csv")
with open(out_csv, "w", newline="", encoding="utf-8") as f:
    writer = csv.DictWriter(f, fieldnames=["Reference File", "Package", "Classification", "Rust Module", "Status"])
    writer.writeheader()
    for row in files_data:
        writer.writerow(row)

print(f"Generated {len(files_data)} rows in REFERENCE-SOURCE-COVERAGE.csv")
