use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::PathBuf;

const COMMON_MODELS: &[&str] = &[
    "anthropic/claude-sonnet-4.6",
    "anthropic/claude-opus-4.6",
    "anthropic/claude-haiku-4.5",
    "openai/gpt-4o",
    "openai/gpt-4o-mini",
    "Custom...",
];

/// All arguments for `portlang new`, collected from CLI flags.
/// Passed in from main.rs after clap parses the command line.
pub struct NewArgs {
    pub path: Option<PathBuf>,
    pub interactive: bool,

    // Field metadata
    pub name: Option<String>,
    pub description: Option<String>,

    // Model
    pub model: String,
    pub temperature: f32,

    // Prompt
    pub goal: Option<String>,
    pub system: Option<String>,
    pub re_observation: Vec<String>,

    // Environment
    pub packages: Vec<String>,

    // Boundary
    pub allow_write: Vec<String>,
    pub network: String,
    pub max_steps: u64,
    pub max_cost: String,
    pub max_tokens: Option<u64>,

    // Tools as JSON strings. Each must have a "type" field: "python", "shell", or "mcp".
    //
    // Python:
    //   {"type":"python","file":"./tools/calc.py","function":"execute"}
    //
    // Shell:
    //   {"type":"shell","name":"run_sql","description":"Run a SQL query","command":"sqlite3 db.sqlite"}
    //   Optional: "input_schema": {...}
    //
    // MCP (stdio):
    //   {"type":"mcp","name":"stripe","command":"npx","args":["-y","@stripe/mcp"],
    //    "env":{"STRIPE_SECRET_KEY":"${STRIPE_SECRET_KEY}"}}
    //
    // MCP (http/sse):
    //   {"type":"mcp","name":"myserver","url":"https://example.com/mcp",
    //    "headers":{"Authorization":"Bearer ${TOKEN}"},"transport":"sse"}
    pub tools: Vec<String>,

    // Verifiers as JSON strings.
    //   {"name":"check-file","command":"test -f result.txt","trigger":"on_stop",
    //    "description":"result.txt must exist"}
    // trigger: "on_stop" | "always" | "on_tool:<tool_name>" (default: "on_stop")
    pub verifiers: Vec<String>,
}

pub fn new_command(args: NewArgs) -> Result<()> {
    if args.interactive {
        new_interactive(args.path)
    } else if args.name.is_none() {
        anyhow::bail!("--name is required (or use --interactive / -i)");
    } else if args.goal.is_none() && args.tools.is_empty() && args.verifiers.is_empty() {
        // Name provided but no other args → write a blank template with the given name
        new_template(args.path, args.name.unwrap())
    } else {
        new_from_args(args)
    }
}

// ─── Non-interactive: from CLI args ──────────────────────────────────────────

fn new_from_args(args: NewArgs) -> Result<()> {
    let name = args
        .name
        .ok_or_else(|| anyhow::anyhow!("--name is required (or use --interactive)"))?;
    let goal = args
        .goal
        .ok_or_else(|| anyhow::anyhow!("--goal is required (or use --interactive)"))?;

    let tools = args
        .tools
        .iter()
        .map(|s| parse_tool_json(s))
        .collect::<Result<Vec<_>>>()?;

    let verifiers = args
        .verifiers
        .iter()
        .map(|s| parse_verifier_json(s))
        .collect::<Result<Vec<_>>>()?;

    let content = build_toml(BuildArgs {
        name: &name,
        description: args.description.as_deref().unwrap_or(""),
        model: &args.model,
        temperature: args.temperature,
        goal: &goal,
        system: args.system.as_deref(),
        re_observation: &args.re_observation,
        packages: &args.packages,
        allow_write: &args.allow_write,
        network: &args.network,
        max_steps: args.max_steps,
        max_cost: &args.max_cost,
        max_tokens: args.max_tokens,
        tools: &tools,
        verifiers: &verifiers,
    });

    let path = resolve_output_path(args.path, Some(&name))?;
    write_field(&path, &content, &tools)?;

    println!("Created {}", path.display());
    println!("\nNext steps:");
    println!("  portlang check {}", path.display());
    println!("  portlang run {}", path.display());
    Ok(())
}

fn parse_tool_json(json: &str) -> Result<ToolConfig> {
    let v: serde_json::Value =
        serde_json::from_str(json).with_context(|| format!("Invalid tool JSON: {json}"))?;

    let tool_type = v["type"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Tool JSON missing \"type\" field: {json}"))?
        .to_string();

    let str_field = |key: &str| -> Option<String> { v[key].as_str().map(|s| s.to_string()) };

    let args: Vec<String> = v["args"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let env: HashMap<String, String> = v["env"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let headers: HashMap<String, String> = v["headers"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    Ok(ToolConfig {
        tool_type,
        file: str_field("file"),
        function: str_field("function"),
        name: str_field("name"),
        description: str_field("description"),
        command: str_field("command"),
        args,
        env,
        url: str_field("url"),
        headers,
        transport: str_field("transport"),
        scaffolded_file: None,
    })
}

fn parse_verifier_json(json: &str) -> Result<VerifierConfig> {
    let v: serde_json::Value =
        serde_json::from_str(json).with_context(|| format!("Invalid verifier JSON: {json}"))?;

    let name = v["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Verifier JSON missing \"name\": {json}"))?
        .to_string();
    let command = v["command"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Verifier JSON missing \"command\": {json}"))?
        .to_string();
    let trigger = v["trigger"].as_str().unwrap_or("on_stop").to_string();
    let description = v["description"].as_str().unwrap_or("").to_string();

    Ok(VerifierConfig {
        name,
        command,
        trigger,
        description,
    })
}

// ─── Template (no args at all) ───────────────────────────────────────────────

fn new_template(output: Option<PathBuf>, name: String) -> Result<()> {
    let path = resolve_output_path(output, Some(&name))?;

    let content = format!(
        "name = \"{name}\"\ndescription = \"A brief description of what this field does\"\n\n\
[model]\nname = \"anthropic/claude-sonnet-4.6\"\ntemperature = 1.0\n\n\
[prompt]\ngoal = \"\"\"\nWrite 'Hello, world!' to result.txt.\n\"\"\"\n\n\
[boundary]\nallow_write = [\"result.txt\"]\nmax_tokens = 50000\nmax_cost = \"$1.00\"\nmax_steps = 20\n\n\
[[verifier]]\nname = \"result-exists\"\ncommand = \"test -f result.txt\"\ntrigger = \"on_stop\"\ndescription = \"result.txt must exist\"\n",
        name = name
    );

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    println!("Created {}", path.display());
    println!("\nNext steps:");
    println!("  1. Edit {} to define your task", path.display());
    println!("  2. Run: portlang check {}", path.display());
    println!("  3. Run: portlang run {}", path.display());
    Ok(())
}

// ─── Interactive wizard ───────────────────────────────────────────────────────

fn new_interactive(output: Option<PathBuf>) -> Result<()> {
    let theme = ColorfulTheme::default();

    println!("Creating a new .field\n");

    // Name
    let name: String = Input::with_theme(&theme)
        .with_prompt("Field name")
        .with_initial_text("my-field")
        .validate_with(|s: &String| {
            if s.trim().is_empty() {
                Err("Name cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()?;

    // Description
    let description: String = Input::with_theme(&theme)
        .with_prompt("Description (optional, press Enter to skip)")
        .allow_empty(true)
        .interact_text()?;

    // Model
    let model_idx = Select::with_theme(&theme)
        .with_prompt("Model")
        .items(COMMON_MODELS)
        .default(0)
        .interact()?;

    let model_name = if model_idx == COMMON_MODELS.len() - 1 {
        Input::with_theme(&theme)
            .with_prompt("Model name (e.g. anthropic/claude-sonnet-4.6)")
            .interact_text()?
    } else {
        COMMON_MODELS[model_idx].to_string()
    };

    // Temperature
    let temperature: f32 = Input::with_theme(&theme)
        .with_prompt("Temperature (0.0 - 1.0)")
        .with_initial_text("1.0")
        .validate_with(|s: &String| {
            s.parse::<f32>()
                .map_err(|_| "Must be a number between 0.0 and 1.0")
                .and_then(|v| {
                    if (0.0..=1.0).contains(&v) {
                        Ok(())
                    } else {
                        Err("Must be between 0.0 and 1.0")
                    }
                })
        })
        .interact_text()?
        .parse()?;

    // Goal
    println!("\nGoal (the task for the agent — press Enter on an empty line when done):");
    let goal = read_multiline(&theme)?;

    // System prompt
    let want_system = Confirm::with_theme(&theme)
        .with_prompt("Add a system prompt?")
        .default(false)
        .interact()?;

    let system = if want_system {
        println!("\nSystem prompt (press Enter on an empty line when done):");
        Some(read_multiline(&theme)?)
    } else {
        None
    };

    // Tools
    let tools = collect_tools(&theme)?;

    // Boundary
    let allow_write_str: String = Input::with_theme(&theme)
        .with_prompt("Writable file patterns (space-separated, e.g. \"*.txt *.json\")")
        .with_initial_text("result.txt")
        .interact_text()?;
    let allow_write: Vec<String> = allow_write_str
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    let network_idx = Select::with_theme(&theme)
        .with_prompt("Network access")
        .items(&["allow", "deny"])
        .default(0)
        .interact()?;
    let network = if network_idx == 0 { "allow" } else { "deny" };

    let max_steps: u64 = Input::with_theme(&theme)
        .with_prompt("Max steps")
        .with_initial_text("20")
        .validate_with(|s: &String| s.parse::<u64>().map(|_| ()).map_err(|_| "Must be a number"))
        .interact_text()?
        .parse()?;

    let max_cost: String = Input::with_theme(&theme)
        .with_prompt("Max cost (e.g. \"$1.00\")")
        .with_initial_text("$1.00")
        .interact_text()?;

    // Verifiers
    let verifiers = collect_verifiers(&theme)?;

    let content = build_toml(BuildArgs {
        name: &name,
        description: &description,
        model: &model_name,
        temperature,
        goal: &goal,
        system: system.as_deref(),
        re_observation: &[],
        packages: &[],
        allow_write: &allow_write,
        network,
        max_steps,
        max_cost: &max_cost,
        max_tokens: None,
        tools: &tools,
        verifiers: &verifiers,
    });

    let path = resolve_output_path(output, Some(&name))?;
    write_field(&path, &content, &tools)?;

    println!("\nNext steps:");
    println!("  portlang check {}", path.display());
    println!("  portlang run {}", path.display());

    Ok(())
}

// ─── Internal types ───────────────────────────────────────────────────────────

struct ToolConfig {
    tool_type: String,
    // python
    file: Option<String>,
    function: Option<String>,
    // python & shell
    name: Option<String>,
    description: Option<String>,
    command: Option<String>,
    // mcp
    args: Vec<String>,
    env: HashMap<String, String>,
    url: Option<String>,
    headers: HashMap<String, String>,
    transport: Option<String>,
    // scaffolded file to write next to field.toml: (relative_path, content)
    scaffolded_file: Option<(String, String)>,
}

struct PythonParam {
    name: String,
    ty: String,
    description: String,
}

struct VerifierConfig {
    name: String,
    command: String,
    trigger: String,
    description: String,
}

// ─── Interactive tool/verifier collection ─────────────────────────────────────

fn collect_tools(theme: &ColorfulTheme) -> Result<Vec<ToolConfig>> {
    let mut tools = Vec::new();

    loop {
        let tool_type_idx = Select::with_theme(theme)
            .with_prompt(if tools.is_empty() {
                "Add a tool? (optional)"
            } else {
                "Add another tool?"
            })
            .items(&["No / done", "python", "shell", "mcp"])
            .default(0)
            .interact()?;

        if tool_type_idx == 0 {
            break;
        }

        let tool = match tool_type_idx {
            1 => configure_python_tool(theme)?,
            2 => {
                let name = Input::with_theme(theme)
                    .with_prompt("Tool name")
                    .interact_text()?;
                let description = Input::with_theme(theme)
                    .with_prompt("Tool description")
                    .interact_text()?;
                let command = Input::with_theme(theme)
                    .with_prompt("Shell command template")
                    .interact_text()?;
                ToolConfig {
                    tool_type: "shell".into(),
                    file: None,
                    function: None,
                    name: Some(name),
                    description: Some(description),
                    command: Some(command),
                    args: vec![],
                    env: HashMap::new(),
                    url: None,
                    headers: HashMap::new(),
                    transport: None,
                    scaffolded_file: None,
                }
            }
            3 => {
                let mcp_name = Input::with_theme(theme)
                    .with_prompt("MCP server name (used as namespace)")
                    .interact_text()?;
                let mcp_command: String = Input::with_theme(theme)
                    .with_prompt("Command (e.g. npx, uvx)")
                    .with_initial_text("npx")
                    .interact_text()?;
                let mcp_args_str: String = Input::with_theme(theme)
                    .with_prompt("Args (space-separated, e.g. \"-y @stripe/mcp\")")
                    .interact_text()?;
                let args: Vec<String> = mcp_args_str
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                let env_key: String = Input::with_theme(theme)
                    .with_prompt("Env var key (e.g. STRIPE_SECRET_KEY, or blank to skip)")
                    .allow_empty(true)
                    .interact_text()?;
                let mut env = HashMap::new();
                if !env_key.is_empty() {
                    env.insert(env_key.clone(), format!("${{{}}}", env_key));
                }
                ToolConfig {
                    tool_type: "mcp".into(),
                    file: None,
                    function: None,
                    name: Some(mcp_name),
                    description: None,
                    command: Some(mcp_command),
                    args,
                    env,
                    url: None,
                    headers: HashMap::new(),
                    transport: None,
                    scaffolded_file: None,
                }
            }
            _ => unreachable!(),
        };

        tools.push(tool);
    }

    Ok(tools)
}

fn configure_python_tool(theme: &ColorfulTheme) -> Result<ToolConfig> {
    let scaffold = Confirm::with_theme(theme)
        .with_prompt("Scaffold a new Python tool file?")
        .default(true)
        .interact()?;

    if !scaffold {
        let file: String = Input::with_theme(theme)
            .with_prompt("Python file path (e.g. ./tools/calc.py)")
            .interact_text()?;
        let function: String = Input::with_theme(theme)
            .with_prompt("Function name (leave blank to expose all)")
            .allow_empty(true)
            .interact_text()?;
        return Ok(ToolConfig {
            tool_type: "python".into(),
            file: Some(file),
            function: if function.is_empty() {
                None
            } else {
                Some(function)
            },
            name: None,
            description: None,
            command: None,
            args: vec![],
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            transport: None,
            scaffolded_file: None,
        });
    }

    let func_name: String = Input::with_theme(theme)
        .with_prompt("Function name (snake_case, becomes the tool name)")
        .validate_with(|s: &String| {
            if s.trim().is_empty() || s.contains(' ') {
                Err("Must be a valid snake_case identifier")
            } else {
                Ok(())
            }
        })
        .interact_text()?;

    let func_description: String = Input::with_theme(theme)
        .with_prompt("One-line description of what this tool does")
        .interact_text()?;

    let mut params: Vec<PythonParam> = Vec::new();
    const COMMON_TYPES: &[&str] = &["str", "int", "float", "bool", "list", "dict", "Custom..."];

    loop {
        let add_param = Confirm::with_theme(theme)
            .with_prompt(if params.is_empty() {
                "Add a parameter?"
            } else {
                "Add another parameter?"
            })
            .default(true)
            .interact()?;
        if !add_param {
            break;
        }

        let param_name: String = Input::with_theme(theme)
            .with_prompt("  Parameter name")
            .interact_text()?;

        let type_idx = Select::with_theme(theme)
            .with_prompt("  Type")
            .items(COMMON_TYPES)
            .default(0)
            .interact()?;
        let param_type = if type_idx == COMMON_TYPES.len() - 1 {
            Input::with_theme(theme)
                .with_prompt("  Custom type (e.g. list[str], dict[str, int])")
                .interact_text()?
        } else {
            COMMON_TYPES[type_idx].to_string()
        };

        let param_description: String = Input::with_theme(theme)
            .with_prompt("  Description")
            .interact_text()?;

        params.push(PythonParam {
            name: param_name,
            ty: param_type,
            description: param_description,
        });
    }

    let rel_path = format!("./tools/{}.py", func_name);
    let py_content = scaffold_python_tool(&func_name, &func_description, &params);

    Ok(ToolConfig {
        tool_type: "python".into(),
        file: Some(rel_path.clone()),
        function: Some(func_name),
        name: None,
        description: None,
        command: None,
        args: vec![],
        env: HashMap::new(),
        url: None,
        headers: HashMap::new(),
        transport: None,
        scaffolded_file: Some((rel_path, py_content)),
    })
}

fn scaffold_python_tool(func_name: &str, description: &str, params: &[PythonParam]) -> String {
    let mut s = String::new();

    let param_sig: Vec<String> = params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect();
    writeln!(s, "def {}({}) -> dict:", func_name, param_sig.join(", ")).unwrap();

    // Google-style docstring — parsed by portlang's tree-sitter extractor
    // to auto-discover name, description, and input_schema
    if params.is_empty() {
        writeln!(s, "    \"\"\"{}\"\"\"", description).unwrap();
    } else {
        writeln!(s, "    \"\"\"{}.", description).unwrap();
        writeln!(s).unwrap();
        writeln!(s, "    Args:").unwrap();
        for p in params {
            writeln!(s, "        {}: {}", p.name, p.description).unwrap();
        }
        writeln!(s).unwrap();
        writeln!(s, "    Returns:").unwrap();
        writeln!(s, "        dict with result").unwrap();
        writeln!(s, "    \"\"\"").unwrap();
    }

    writeln!(s).unwrap();
    writeln!(s, "    # TODO: implement this tool").unwrap();
    writeln!(
        s,
        "    raise NotImplementedError(\"implement {}\")",
        func_name
    )
    .unwrap();

    s
}

fn collect_verifiers(theme: &ColorfulTheme) -> Result<Vec<VerifierConfig>> {
    let mut verifiers = Vec::new();

    loop {
        let add = Confirm::with_theme(theme)
            .with_prompt(if verifiers.is_empty() {
                "Add a verifier? (optional)"
            } else {
                "Add another verifier?"
            })
            .default(false)
            .interact()?;

        if !add {
            break;
        }

        let name = Input::with_theme(theme)
            .with_prompt("Verifier name")
            .interact_text()?;
        let command = Input::with_theme(theme)
            .with_prompt("Shell command (exit 0 = pass)")
            .interact_text()?;
        let trigger_idx = Select::with_theme(theme)
            .with_prompt("Trigger")
            .items(&["on_stop", "always", "on_tool: (specify tool name)"])
            .default(0)
            .interact()?;
        let trigger = match trigger_idx {
            0 => "on_stop".to_string(),
            1 => "always".to_string(),
            _ => {
                let tool_name: String = Input::with_theme(theme)
                    .with_prompt("Tool name (e.g. bash, write, python)")
                    .interact_text()?;
                format!("on_tool:{}", tool_name)
            }
        };
        let description = Input::with_theme(theme)
            .with_prompt("Description (injected into context on failure)")
            .interact_text()?;

        verifiers.push(VerifierConfig {
            name,
            command,
            trigger,
            description,
        });
    }

    Ok(verifiers)
}

// ─── Multiline input helper ───────────────────────────────────────────────────

fn read_multiline(theme: &ColorfulTheme) -> Result<String> {
    let mut lines = Vec::new();

    loop {
        let line: String = Input::with_theme(theme)
            .with_prompt("  >")
            .allow_empty(true)
            .interact_text()?;

        if line.is_empty() && !lines.is_empty() {
            break;
        } else if !line.is_empty() {
            lines.push(line);
        }
    }

    Ok(lines.join("\n"))
}

// ─── File writing ─────────────────────────────────────────────────────────────

fn resolve_output_path(output: Option<PathBuf>, name: Option<&str>) -> Result<PathBuf> {
    let default_name = name
        .map(|n| format!("{}.field", n))
        .unwrap_or_else(|| "field.field".to_string());

    let path = output.unwrap_or_else(|| PathBuf::from(&default_name));

    let path = if path.is_dir() {
        path.join(&default_name)
    } else {
        path
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
    }

    if path.exists() {
        anyhow::bail!(
            "{} already exists. Remove it first or specify a different path.",
            path.display()
        );
    }

    Ok(path)
}

fn write_field(path: &PathBuf, content: &str, tools: &[ToolConfig]) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    println!("Created {}", path.display());

    let field_dir = path.parent().unwrap_or(std::path::Path::new("."));
    for tool in tools {
        if let Some((rel_path, py_content)) = &tool.scaffolded_file {
            let stripped = rel_path.strip_prefix("./").unwrap_or(rel_path);
            let dest = field_dir.join(stripped);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            std::fs::write(&dest, py_content)
                .with_context(|| format!("Failed to write {}", dest.display()))?;
            println!("Created {}", dest.display());
        }
    }

    Ok(())
}

// ─── TOML generation ─────────────────────────────────────────────────────────

struct BuildArgs<'a> {
    name: &'a str,
    description: &'a str,
    model: &'a str,
    temperature: f32,
    goal: &'a str,
    system: Option<&'a str>,
    re_observation: &'a [String],
    packages: &'a [String],
    allow_write: &'a [String],
    network: &'a str,
    max_steps: u64,
    max_cost: &'a str,
    max_tokens: Option<u64>,
    tools: &'a [ToolConfig],
    verifiers: &'a [VerifierConfig],
}

fn build_toml(a: BuildArgs<'_>) -> String {
    let mut s = String::new();

    writeln!(s, "name = {:?}", a.name).unwrap();
    if !a.description.is_empty() {
        writeln!(s, "description = {:?}", a.description).unwrap();
    }
    writeln!(s).unwrap();

    writeln!(s, "[model]").unwrap();
    writeln!(s, "name = {:?}", a.model).unwrap();
    writeln!(s, "temperature = {}", a.temperature).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "[prompt]").unwrap();
    writeln!(s, "goal = \"\"\"").unwrap();
    for line in a.goal.lines() {
        writeln!(s, "{}", line).unwrap();
    }
    writeln!(s, "\"\"\"").unwrap();
    if let Some(sys) = a.system {
        writeln!(s, "system = \"\"\"").unwrap();
        for line in sys.lines() {
            writeln!(s, "{}", line).unwrap();
        }
        writeln!(s, "\"\"\"").unwrap();
    }
    if !a.re_observation.is_empty() {
        let cmds: Vec<String> = a
            .re_observation
            .iter()
            .map(|c| format!("{:?}", c))
            .collect();
        writeln!(s, "re_observation = [{}]", cmds.join(", ")).unwrap();
    }
    writeln!(s).unwrap();

    // Environment — only emit if there's something non-default
    if !a.packages.is_empty() {
        writeln!(s, "[environment]").unwrap();
        let pkgs: Vec<String> = a.packages.iter().map(|p| format!("{:?}", p)).collect();
        writeln!(s, "packages = [{}]", pkgs.join(", ")).unwrap();
        writeln!(s).unwrap();
    }

    writeln!(s, "[boundary]").unwrap();
    if !a.allow_write.is_empty() {
        let pats: Vec<String> = a.allow_write.iter().map(|p| format!("{:?}", p)).collect();
        writeln!(s, "allow_write = [{}]", pats.join(", ")).unwrap();
    }
    writeln!(s, "network = {:?}", a.network).unwrap();
    writeln!(s, "max_steps = {}", a.max_steps).unwrap();
    writeln!(s, "max_cost = {:?}", a.max_cost).unwrap();
    if let Some(tok) = a.max_tokens {
        writeln!(s, "max_tokens = {}", tok).unwrap();
    }
    writeln!(s).unwrap();

    for tool in a.tools {
        writeln!(s, "[[tool]]").unwrap();
        writeln!(s, "type = {:?}", tool.tool_type).unwrap();
        match tool.tool_type.as_str() {
            "python" => {
                if let Some(ref file) = tool.file {
                    writeln!(s, "file = {:?}", file).unwrap();
                }
                if let Some(ref func) = tool.function {
                    writeln!(s, "function = {:?}", func).unwrap();
                }
                if let Some(ref n) = tool.name {
                    writeln!(s, "name = {:?}", n).unwrap();
                }
                if let Some(ref d) = tool.description {
                    writeln!(s, "description = {:?}", d).unwrap();
                }
            }
            "shell" => {
                if let Some(ref n) = tool.name {
                    writeln!(s, "name = {:?}", n).unwrap();
                }
                if let Some(ref d) = tool.description {
                    writeln!(s, "description = {:?}", d).unwrap();
                }
                if let Some(ref cmd) = tool.command {
                    writeln!(s, "command = {:?}", cmd).unwrap();
                }
            }
            "mcp" => {
                if let Some(ref n) = tool.name {
                    writeln!(s, "name = {:?}", n).unwrap();
                }
                if let Some(ref url) = tool.url {
                    writeln!(s, "url = {:?}", url).unwrap();
                }
                if let Some(ref cmd) = tool.command {
                    writeln!(s, "command = {:?}", cmd).unwrap();
                }
                if !tool.args.is_empty() {
                    let arg_list: Vec<String> =
                        tool.args.iter().map(|a| format!("{:?}", a)).collect();
                    writeln!(s, "args = [{}]", arg_list.join(", ")).unwrap();
                }
                if !tool.env.is_empty() {
                    let pairs: Vec<String> = tool
                        .env
                        .iter()
                        .map(|(k, v)| format!("{} = {:?}", k, v))
                        .collect();
                    writeln!(s, "env = {{ {} }}", pairs.join(", ")).unwrap();
                }
                if !tool.headers.is_empty() {
                    let pairs: Vec<String> = tool
                        .headers
                        .iter()
                        .map(|(k, v)| format!("{:?} = {:?}", k, v))
                        .collect();
                    writeln!(s, "headers = {{ {} }}", pairs.join(", ")).unwrap();
                }
                if let Some(ref t) = tool.transport {
                    writeln!(s, "transport = {:?}", t).unwrap();
                }
            }
            _ => {}
        }
        writeln!(s).unwrap();
    }

    for v in a.verifiers {
        writeln!(s, "[[verifier]]").unwrap();
        writeln!(s, "name = {:?}", v.name).unwrap();
        writeln!(s, "command = {:?}", v.command).unwrap();
        writeln!(s, "trigger = {:?}", v.trigger).unwrap();
        if !v.description.is_empty() {
            writeln!(s, "description = {:?}", v.description).unwrap();
        }
        writeln!(s).unwrap();
    }

    s
}
