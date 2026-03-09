# Structured Output Example

This example demonstrates how to use structured output validation with portlang.

## Running the Example

```bash
portlang run examples/structured-output-example/field.toml
```

## How It Works

1. The agent analyzes files in the workspace
2. It produces JSON output matching the defined schema
3. The output is validated against the schema automatically
4. If valid, it's written to `/workspace/output.json`
5. Verifiers run to check specific properties

## Verifier Approaches

### Basic Shell (No Dependencies)

The example uses basic shell commands that work in any container:

```toml
[[verifiers]]
name = "output-exists"
command = "test -f /workspace/output.json && cat /workspace/output.json"
```

### Python (More Flexible)

For complex validation, use Python:

```toml
[[verifiers]]
name = "validate-count"
command = """
python3 -c '
import json
with open(\"/workspace/output.json\") as f:
    data = json.load(f)
    assert data[\"file_count\"] > 0, \"No files found\"
    assert data[\"status\"] == \"success\", \"Status not success\"
    print(\"✓ Validation passed\")
'
"""
```

### jq (Best for JSON - Included by Default)

The default container includes jq, so you can use it directly:

```toml
[[verifiers]]
name = "status-is-success"
command = "jq -e '.status == \"success\"' /workspace/output.json"
```

jq is perfect for JSON validation and is the recommended approach for verifying structured output.

## Expected Output

The agent should produce JSON like:

```json
{
  "status": "success",
  "file_count": 1,
  "files": ["test.txt"],
  "summary": "Found 1 file in the workspace"
}
```

This gets validated against the schema, written to output.json, and then verified.
