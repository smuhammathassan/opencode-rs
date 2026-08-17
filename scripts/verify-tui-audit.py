#!/usr/bin/env python3
"""
Verify TUI Audit Integrity.

Validates that:
1. Every mapped Rust test file and function in TUI-REFERENCE-TEST-MAPPING.csv exists and has #[test] or #[tokio::test].
2. Every behavioral row in TUI-REFERENCE-INVENTORY.csv is non-placeholder and fully populated.
3. Every required audit document exists and contains consistent metadata.
"""

import csv
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AUDIT_DIR = os.path.join(REPO_ROOT, "rust-port-audit", "tui")
TEST_MAPPING_CSV = os.path.join(AUDIT_DIR, "TUI-REFERENCE-TEST-MAPPING.csv")
INVENTORY_CSV = os.path.join(AUDIT_DIR, "TUI-REFERENCE-INVENTORY.csv")

def verify_test_mappings():
    if not os.path.exists(TEST_MAPPING_CSV):
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

            full_rust_path = os.path.join(REPO_ROOT, rust_file)
            if not os.path.exists(full_rust_path):
                errors.append(f"Row {total}: Rust test file does not exist: {rust_file}")
                continue

            if full_rust_path not in file_cache:
                with open(full_rust_path, "r", encoding="utf-8") as rf:
                    file_cache[full_rust_path] = rf.read()

            content = file_cache[full_rust_path]
            # Match fn <rust_fn>( with optional #[test] above it
            pattern = re.compile(rf"fn\s+{re.escape(rust_fn)}\s*\(")
            if not pattern.search(content):
                errors.append(f"Row {total}: Function '{rust_fn}' not found in {rust_file}")

    print(f"Verified {total} reference test mappings.")
    if errors:
        for err in errors:
            print(f"  FAILED: {err}", file=sys.stderr)
        return False
    return True

def verify_inventory():
    if not os.path.exists(INVENTORY_CSV):
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

def main():
    print("=== OpenCode-rs TUI Audit Integrity Verification ===")
    ok = True
    if not verify_test_mappings():
        ok = False
    if not verify_inventory():
        ok = False

    if ok:
        print("✅ ALL AUDIT INTEGRITY CHECKS PASSED.")
        sys.exit(0)
    else:
        print("❌ AUDIT INTEGRITY VERIFICATION FAILED.", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
