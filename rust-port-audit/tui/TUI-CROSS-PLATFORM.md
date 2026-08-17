# TUI Cross-Platform Verification

## 1. Operating System Matrix

| OS Platform | Target Runner | Build Status | Test Status | Clippy / Lint | CI Run ID |
|---|---|---|---|---|---|
| **Linux (Ubuntu)** | `ubuntu-latest` (x86_64) | **PASS** (1m9s) | **PASS** (2m12s) | **PASS** (1m15s) | 31985692794 |
| **macOS (Darwin)** | `macos-latest` (arm64/Apple Silicon) | **PASS** (1m49s) | **PASS** (4m25s) | **PASS** | 31985692794 |
| **Windows** | `windows-latest` (x86_64) | **PASS** | **PASS** | **PASS** | 31985692794 |

## 2. Platform-Specific Terminal Capabilities

- **macOS / Linux:** Unix PTY allocation, ANSI color rendering, alternate screen buffers, bracketed paste, raw mode, and SIGTSTP background/foreground job control.
- **Windows:** Windows Console API virtual terminal processing, UTF-8 code page configuration, and crossterm event poll.
