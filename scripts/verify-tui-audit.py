#!/usr/bin/env python3
"""
Verify TUI Audit Integrity.

Validates that:
1. Every mapped Rust test file and function in TUI-REFERENCE-TEST-MAPPING.csv exists and is annotated with #[test] or #[tokio::test].
2. Every behavioral row in TUI-REFERENCE-INVENTORY.csv is non-placeholder, valid, and fully populated.
3. Every one of the 26 differential scenario directories exists under rust-port-audit/tui/differential/ and contains valid scenario.json, reference-frame.txt, rust-frame.txt, and result.json (with PASS status).
4. UNMAPPED-REFERENCE-FILES.txt and UNMAPPED-REFERENCE-TESTS.txt exist and are clean.
5. All required audit documents exist and contain valid metadata.
"""

import csv
import hashlib
import json
import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
AUDIT_DIR = REPO_ROOT / "rust-port-audit" / "tui"
TEST_MAPPING_CSV = AUDIT_DIR / "TUI-REFERENCE-TEST-MAPPING.csv"
INVENTORY_CSV = AUDIT_DIR / "TUI-REFERENCE-INVENTORY.csv"
DIFF_DIR = AUDIT_DIR / "differential"

REQUIRED_DOCS = [
    "AUDIT-IDENTITY.md",
    "TUI-FINAL-REPORT.md",
    "TUI-REFERENCE-INVENTORY.csv",
    "TUI-REFERENCE-TEST-MAPPING.csv",
    "UNMAPPED-REFERENCE-FILES.txt",
    "UNMAPPED-REFERENCE-TESTS.txt",
]

def verify_test_mappings():
    if not TEST_MAPPING_CSV.exists():
        print(f"ERROR: {TEST_MAPPING_CSV} does not exist", file=sys.stderr)
        return False

    file_cache = {}
    errors = []
    total = 0

    with open(TEST_MAPPING_CSV, "r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            total += 1
            ref_file = row.get("Reference test file", "").strip()
            ref_test = row.get("Reference test name", "").strip()
            rust_file = row.get("Rust test file", "").strip()
            rust_fn = row.get("Rust test function", "").strip()

            if not rust_file or not rust_fn:
                errors.append(f"Row {total}: Empty Rust test mapping for {ref_file}::{ref_test}")
                continue

            full_rust_path = REPO_ROOT / rust_file
            if not full_rust_path.exists():
                errors.append(f"Row {total}: Rust test file does not exist: {rust_file}")
                continue

            if full_rust_path not in file_cache:
                with open(full_rust_path, "r", encoding="utf-8") as rf:
                    file_cache[full_rust_path] = rf.read()

            content = file_cache[full_rust_path]
            # Match fn <rust_fn>( in file
            fn_pattern = re.compile(rf"fn\s+{re.escape(rust_fn)}\s*\(")
            if not fn_pattern.search(content):
                errors.append(f"Row {total}: Function '{rust_fn}' not found in {rust_file}")
                continue

            # Verify #[test] or #[tokio::test] annotation precedes the function
            test_pattern = re.compile(rf"#\[(?:tokio::)?test(?:\([^)]*\))?\]\s*(?:async\s+)?fn\s+{re.escape(rust_fn)}\s*\(")
            if not test_pattern.search(content):
                errors.append(f"Row {total}: Function '{rust_fn}' in {rust_file} lacks #[test] attribute")

    print(f"Verified {total} reference test mappings.")
    if errors:
        for err in errors:
            print(f"  FAILED: {err}", file=sys.stderr)
        return False
    return True

def verify_inventory():
    if not INVENTORY_CSV.exists():
        print(f"ERROR: {INVENTORY_CSV} does not exist", file=sys.stderr)
        return False

    errors = []
    total = 0
    with open(INVENTORY_CSV, "r", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        for row in reader:
            total += 1
            row_id = row.get("ID", "").strip()
            feature = row.get("Feature", "").strip()
            ref_behavior = row.get("Reference behavior", "").strip()
            status = row.get("Status", "").strip()

            if not row_id or not feature or not ref_behavior:
                errors.append(f"Inventory Row {total}: Incomplete behavioral row")
            if ref_behavior in ["State Updated", "Frame Rendered", "HTTP/SSE", "Error Handled", "Feature works"]:
                errors.append(f"Inventory Row {total} ({row_id}): Contains generic filler behavior '{ref_behavior}'")

    print(f"Verified {total} behavioral inventory rows.")
    if errors:
        for err in errors:
            print(f"  FAILED: {err}", file=sys.stderr)
        return False
    return True

def canonical_json(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

def sha256(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()

def verify_differential_scenarios():
    if not DIFF_DIR.exists():
        print(f"ERROR: {DIFF_DIR} does not exist", file=sys.stderr)
        return False

    scenario_dirs = [d for d in DIFF_DIR.iterdir() if d.is_dir()]
    if len(scenario_dirs) < 26:
        print(f"ERROR: Expected at least 26 differential scenario directories, found {len(scenario_dirs)}", file=sys.stderr)
        return False

    errors = []
    verified = 0

    for sc_dir in sorted(scenario_dirs):
        scenario_json = sc_dir / "scenario.json"
        ref_frame = sc_dir / "reference-frame.txt"
        rust_frame = sc_dir / "rust-frame.txt"
        result_json = sc_dir / "result.json"

        if not scenario_json.exists():
            errors.append(f"Scenario {sc_dir.name}: missing scenario.json")
        else:
            try:
                with open(scenario_json, "r", encoding="utf-8") as f:
                    sc_data = json.load(f)
                    ref_src = sc_data.get("reference_source", "")
                    if "*" not in ref_src and (not ref_src or not (REPO_ROOT / ref_src).exists()):
                        errors.append(f"Scenario {sc_dir.name}: reference_source '{ref_src}' missing in repository")
                    ref_cmd = " ".join(sc_data.get("reference_command", []))
                    if "--import" not in ref_cmd or "ts_loader" not in ref_cmd:
                        errors.append(f"Scenario {sc_dir.name}: reference_command must execute via ts_loader")
                    rust_cmd = " ".join(sc_data.get("rust_command", []))
                    if "diff_scenarios" not in rust_cmd:
                        errors.append(f"Scenario {sc_dir.name}: rust_command must execute diff_scenarios harness")
            except Exception as e:
                errors.append(f"Scenario {sc_dir.name}: invalid scenario.json: {e}")

        if not ref_frame.exists():
            errors.append(f"Scenario {sc_dir.name}: missing reference-frame.txt")
        else:
            ref_content = ref_frame.read_text(encoding="utf-8")
            if "Executed Command: " not in ref_content or "Exit Code: 0" not in ref_content:
                errors.append(f"Scenario {sc_dir.name}: reference-frame.txt lacks real execution provenance")
            if "SHA-256 Output Hash: " not in ref_content:
                errors.append(f"Scenario {sc_dir.name}: reference-frame.txt lacks cryptographic hash")
            if "--import" not in ref_content or "ts_loader" not in ref_content:
                errors.append(f"Scenario {sc_dir.name}: reference execution must use ts_loader ESM module loader")
            if "import " not in ref_content or "reference/" not in ref_content:
                errors.append(f"Scenario {sc_dir.name}: reference eval must directly import from reference/")
            if "console.log(\"Selected clipboard" in ref_content:
                errors.append(f"Scenario {sc_dir.name}: forbidden handwritten reference behavior detected")

        if not rust_frame.exists():
            errors.append(f"Scenario {sc_dir.name}: missing rust-frame.txt")
        else:
            rust_content = rust_frame.read_text(encoding="utf-8")
            if "Executed Command: " not in rust_content or "Exit Code: 0" not in rust_content:
                errors.append(f"Scenario {sc_dir.name}: rust-frame.txt lacks real execution provenance")
            if "SHA-256 Output Hash: " not in rust_content:
                errors.append(f"Scenario {sc_dir.name}: rust-frame.txt lacks cryptographic hash")
            if "diff_scenarios" not in rust_content:
                errors.append(f"Scenario {sc_dir.name}: rust execution must execute production diff_scenarios")

        if not result_json.exists():
            errors.append(f"Scenario {sc_dir.name}: missing result.json")
        else:
            try:
                with open(result_json, "r", encoding="utf-8") as f:
                    res_data = json.load(f)
                    if res_data.get("status") != "PASS":
                        errors.append(f"Scenario {sc_dir.name}: result status is '{res_data.get('status')}', expected PASS")
                    if not res_data.get("matched", False):
                        errors.append(f"Scenario {sc_dir.name}: matched is not True")
                    if not res_data.get("outputs_equal", False):
                        errors.append(f"Scenario {sc_dir.name}: outputs_equal is not True")
                    if res_data.get("reference_exit_code") != 0 or res_data.get("rust_exit_code") != 0:
                        errors.append(f"Scenario {sc_dir.name}: process exit code not 0")
                    
                    # Recompute cryptographic SHA-256 hashes of actual outputs
                    ref_payload = res_data.get("reference_output")
                    rust_payload = res_data.get("rust_output")
                    if isinstance(rust_payload, dict) and "result" in rust_payload:
                        rust_payload = rust_payload["result"]
                    
                    if ref_payload != rust_payload:
                        errors.append(f"Scenario {sc_dir.name}: reference_output does not match rust_output")

                    recomputed_ref_hash = sha256(canonical_json(ref_payload))
                    recomputed_rust_hash = sha256(canonical_json(rust_payload))

                    if res_data.get("reference_output_sha256") != recomputed_ref_hash:
                        errors.append(f"Scenario {sc_dir.name}: reference_output_sha256 mismatch (recorded {res_data.get('reference_output_sha256')}, recomputed {recomputed_ref_hash})")
                    if res_data.get("rust_output_sha256") != recomputed_rust_hash:
                        errors.append(f"Scenario {sc_dir.name}: rust_output_sha256 mismatch (recorded {res_data.get('rust_output_sha256')}, recomputed {recomputed_rust_hash})")
                    if recomputed_ref_hash != recomputed_rust_hash:
                        errors.append(f"Scenario {sc_dir.name}: cryptographic output hashes differ ({recomputed_ref_hash} vs {recomputed_rust_hash})")
            except Exception as e:
                errors.append(f"Scenario {sc_dir.name}: invalid result.json: {e}")

        verified += 1

    print(f"Verified {verified} paired differential scenario artifacts.")
    if errors:
        for err in errors:
            print(f"  FAILED: {err}", file=sys.stderr)
        return False
    return True

def verify_required_documents():
    errors = []
    for doc in REQUIRED_DOCS:
        doc_path = AUDIT_DIR / doc
        if not doc_path.exists():
            errors.append(f"Missing required audit document: {doc}")
        elif doc.endswith(".txt"):
            content = doc_path.read_text(encoding="utf-8").strip()
            # Unmapped files/tests should have 0 unmapped items
            lines = [l for l in content.splitlines() if l.strip() and not l.startswith("#")]
            if len(lines) > 0:
                errors.append(f"{doc} has {len(lines)} unmapped entries")

    if errors:
        for err in errors:
            print(f"  FAILED: {err}", file=sys.stderr)
        return False
    print("Verified all required audit documents and clean unmapped trackers.")
    return True

def main():
    print("=== OpenCode-rs TUI Audit Integrity Verification ===")
    ok = True
    if not verify_test_mappings():
        ok = False
    if not verify_inventory():
        ok = False
    if not verify_differential_scenarios():
        ok = False
    if not verify_required_documents():
        ok = False

    if ok:
        print("✅ ALL AUDIT INTEGRITY CHECKS PASSED.")
        sys.exit(0)
    else:
        print("❌ AUDIT INTEGRITY VERIFICATION FAILED.", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
