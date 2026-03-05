# Integration Tests

This directory contains integration test fields for portlang Phase 1.

## Test Fields

### 1. minimal-field
**Purpose**: Basic functionality test - read a file and output its contents.

**Expected outcome**: Converged

**To run**:
```bash
cd tests/integration/fields/minimal-field
portlang run field.toml
```

### 2. write-field
**Purpose**: Test file writing with boundary enforcement and verifiers.

**Expected outcome**: Converged (after creating output.txt)

**To run**:
```bash
cd tests/integration/fields/write-field
portlang run field.toml
```

### 3. boundary-violation
**Purpose**: Test boundary enforcement - agent should be blocked from writing to unauthorized files.

**Expected outcome**: May not converge cleanly, but should show boundary rejections in trajectory

**To run**:
```bash
cd tests/integration/fields/boundary-violation
portlang run field.toml
```

### 4. budget-exhaustion
**Purpose**: Test token/cost budget limits.

**Expected outcome**: BudgetExhausted

**To run**:
```bash
cd tests/integration/fields/budget-exhaustion
portlang run field.toml
```

## Prerequisites

Before running tests, ensure you have:

1. Built portlang:
   ```bash
   cargo build --release
   ```

2. Set your Anthropic API key:
   ```bash
   export ANTHROPIC_API_KEY=sk-...
   ```

3. The `portlang` binary is in your PATH or use the full path:
   ```bash
   ../../target/release/portlang run field.toml
   ```

## Checking Fields

You can validate field TOML files without running them:

```bash
portlang check field.toml
```

## Trajectories

After running a field, trajectories are saved to:
```
~/.portlang/trajectories/{field_name}/{timestamp}-{random}.json
```

You can inspect these JSON files to see:
- All steps taken
- Actions performed
- Tool call inputs/outputs
- Verifier results
- Cost and token usage
- Final outcome
