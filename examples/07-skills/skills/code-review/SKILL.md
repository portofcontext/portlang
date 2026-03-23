---
name: code-review
description: Opinionated code review guide for Python covering correctness, style, and security. Use when asked to review, audit, or check Python code for bugs, security issues, or quality problems.
---

# Code Review Guide

## Workflow

1. **Run the pre-flight checker** on the file being reviewed:
   ```bash
   python3 scripts/check.py <path-to-file>
   ```
   Outputs a JSON array of automated findings (hardcoded secrets, SQL injection
   patterns, weak hashing). Use these as a starting point — the checker is not
   exhaustive; always read the code yourself.

2. **Read the file** and apply the full checklist below.

3. **Write the review** following the output format at the bottom.

## Review Checklist

### Correctness
- [ ] Logic is correct for all inputs, including edge cases (empty, None, 0, negative)
- [ ] Error paths are handled — don't silently swallow exceptions
- [ ] Resource cleanup happens in all exit paths (use `with` / context managers)
- [ ] No off-by-one errors in loops or slices

### Style
- [ ] Names are descriptive: variables explain what they hold, functions explain what they do
- [ ] Functions are short (< 30 lines) and do one thing
- [ ] No magic numbers — use named constants
- [ ] Type annotations on function signatures

### Security
- [ ] No secrets, tokens, or passwords hardcoded in source
- [ ] External inputs are validated and sanitised before use
- [ ] SQL queries use parameterized form — never built by string formatting
- [ ] Weak hashing (MD5, SHA-1) not used for passwords or security-sensitive data
- [ ] File paths not constructed from user input without sanitization

## Output Format

Write your review as `review.md`:

```
## Summary
One-sentence verdict.

## Issues
- [CRITICAL] <issue> — <why it matters>
- [WARNING]  <issue> — <why it matters>
- [STYLE]    <issue> — <suggestion>

## Verdict
PASS | NEEDS CHANGES | FAIL
```

Only list issues you actually found. Skip sections with no findings.
