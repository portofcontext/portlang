use portlang_config::parse_field_from_file;
use portlang_core::Environment;
use std::fs;
use std::io::Write;
use tempfile::TempDir;

#[test]
fn test_real_world_field_with_relative_paths() {
    // Create a realistic directory structure
    let temp_dir = TempDir::new().unwrap();
    let field_dir = temp_dir.path();

    // Create workspace and tools directories
    fs::create_dir(field_dir.join("workspace")).unwrap();
    fs::create_dir(field_dir.join("tools")).unwrap();

    // Create a Python tool file
    let mut tool_file = fs::File::create(field_dir.join("tools/calculator.py")).unwrap();
    tool_file
        .write_all(
            b"
def add(x: int, y: int) -> int:
    \"\"\"Add two numbers\"\"\"
    return x + y
",
        )
        .unwrap();

    // Create field.toml with relative paths
    let field_path = field_dir.join("field.toml");
    let mut field_file = fs::File::create(&field_path).unwrap();
    field_file
        .write_all(
            b"
name = \"test-field\"
goal = \"Test relative path resolution\"

[model]
name = \"test-model\"

[environment]
type = \"local\"
root = \"./workspace\"

[[tool]]
type = \"python\"
script = \"./tools/calculator.py\"
function = \"add\"
",
        )
        .unwrap();
    drop(field_file);

    // Parse the field
    let field = parse_field_from_file(&field_path).unwrap();

    // Verify environment root is resolved correctly
    let Environment::Local { root } = &field.environment;
    let expected_root = field_dir.join("workspace");
    assert_eq!(
        root,
        &expected_root.to_string_lossy().to_string(),
        "Environment root should be resolved relative to field.toml"
    );

    // Verify tool script is resolved correctly
    assert_eq!(field.custom_tools.len(), 1);
    let script_path = field.custom_tools[0].script.as_ref().unwrap();
    let expected_script = field_dir.join("tools/calculator.py");
    assert_eq!(
        script_path,
        &expected_script.to_string_lossy().to_string(),
        "Tool script should be resolved relative to field.toml"
    );

    // Verify config_dir is set
    assert_eq!(field.config_dir, Some(field_dir.to_path_buf()));

    // Verify tool was auto-discovered
    assert_eq!(field.custom_tools[0].name, "add");
}

#[test]
fn test_field_portable_across_working_directories() {
    // Create a field in a subdirectory
    let temp_dir = TempDir::new().unwrap();
    let project_dir = temp_dir.path().join("my-project");
    fs::create_dir(&project_dir).unwrap();
    fs::create_dir(project_dir.join("workspace")).unwrap();

    let field_path = project_dir.join("field.toml");
    fs::write(
        &field_path,
        b"
name = \"portable\"
goal = \"Test portability\"

[model]
name = \"test\"

[environment]
type = \"local\"
root = \"./workspace\"
",
    )
    .unwrap();

    // Save current directory
    let original_dir = std::env::current_dir().unwrap();

    // Parse from parent directory
    std::env::set_current_dir(&temp_dir).unwrap();
    let field1 = parse_field_from_file("my-project/field.toml").unwrap();

    // Parse from project directory
    std::env::set_current_dir(&project_dir).unwrap();
    let field2 = parse_field_from_file("field.toml").unwrap();

    // Restore original directory
    std::env::set_current_dir(original_dir).unwrap();

    // Both should resolve to the same absolute path
    let Environment::Local { root: root1 } = &field1.environment;
    let Environment::Local { root: root2 } = &field2.environment;

    // On macOS, /var and /private/var are the same, so normalize for comparison
    let normalize = |p: &str| -> String { p.replace("/private/var", "/var") };

    assert_eq!(
        normalize(root1),
        normalize(root2),
        "Field should resolve to same paths regardless of CWD"
    );

    let expected = project_dir.join("workspace");
    assert_eq!(
        normalize(root1),
        normalize(&expected.to_string_lossy().to_string())
    );
}
