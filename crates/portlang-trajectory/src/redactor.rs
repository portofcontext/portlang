use portlang_core::{Action, Trajectory};

/// Redacts secret values from trajectory data before it is written to disk.
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    /// Create a redactor from a list of secret values.
    /// Secrets should be pre-sorted longest-first (as returned by `Field::collect_secret_candidates`).
    pub fn new(secrets: Vec<String>) -> Self {
        Self { secrets }
    }

    /// Replace all secret occurrences in `s` with `[REDACTED]`.
    pub fn redact(&self, s: &str) -> String {
        let mut result = s.to_string();
        for secret in &self.secrets {
            result = result.replace(secret.as_str(), "[REDACTED]");
        }
        result
    }

    /// Return a cloned trajectory with all secret values replaced.
    pub fn redact_trajectory(&self, trajectory: &Trajectory) -> Trajectory {
        let mut t = trajectory.clone();
        t.goal = self.redact(&t.goal);
        t.system_prompt = self.redact(&t.system_prompt);
        t.tool_definitions = self.redact(&t.tool_definitions);
        for step in &mut t.steps {
            step.result = self.redact(&step.result);
            match &mut step.action {
                Action::ToolCall { input, .. } => {
                    *input = redact_json(input, self);
                }
                Action::TextOutput { text } => {
                    *text = self.redact(text);
                }
                Action::Stop => {}
            }
        }
        if let Some(ref mut output) = t.structured_output {
            *output = redact_json(output, self);
        }
        t
    }
}

fn redact_json(value: &serde_json::Value, r: &Redactor) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(r.redact(s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| redact_json(v, r)).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_json(v, r)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Cost, TrajectoryStep};
    use serde_json::json;

    fn make_step(action: Action, result: &str) -> TrajectoryStep {
        TrajectoryStep::new(1, action, result.to_string(), false, Cost::ZERO, 0)
    }

    #[test]
    fn test_redact_string() {
        let r = Redactor::new(vec!["sk-secret-key-abc".to_string()]);
        assert_eq!(
            r.redact("token: sk-secret-key-abc here"),
            "token: [REDACTED] here"
        );
    }

    #[test]
    fn test_redact_string_no_match() {
        let r = Redactor::new(vec!["sk-secret-key-abc".to_string()]);
        assert_eq!(r.redact("nothing sensitive"), "nothing sensitive");
    }

    #[test]
    fn test_redact_empty_secrets_is_noop() {
        let r = Redactor::new(vec![]);
        let s = "this has no secrets configured";
        assert_eq!(r.redact(s), s);
    }

    #[test]
    fn test_redact_trajectory_goal_and_system_prompt() {
        let secret = "tok-super-secret-99";
        let r = Redactor::new(vec![secret.to_string()]);

        let mut traj = Trajectory::new("field".to_string());
        traj.goal = format!("use token {secret} to do something");
        traj.system_prompt = format!("auth: {secret}");
        traj.tool_definitions = format!("bearer {secret}");

        let redacted = r.redact_trajectory(&traj);
        assert!(!redacted.goal.contains(secret));
        assert!(!redacted.system_prompt.contains(secret));
        assert!(!redacted.tool_definitions.contains(secret));
        assert_eq!(redacted.goal, "use token [REDACTED] to do something");
    }

    #[test]
    fn test_redact_step_result() {
        let secret = "api-key-xyz-789";
        let r = Redactor::new(vec![secret.to_string()]);

        let mut traj = Trajectory::new("field".to_string());
        traj.add_step(make_step(
            Action::stop(),
            &format!("response contained {secret}"),
        ));

        let redacted = r.redact_trajectory(&traj);
        assert!(!redacted.steps[0].result.contains(secret));
        assert_eq!(redacted.steps[0].result, "response contained [REDACTED]");
    }

    #[test]
    fn test_redact_tool_call_input_json() {
        let secret = "db-password-secret-42";
        let r = Redactor::new(vec![secret.to_string()]);

        let input = json!({
            "query": format!("SELECT * WHERE key = '{secret}'"),
            "nested": { "auth": secret },
            "list": [secret, "safe-value"]
        });
        let mut traj = Trajectory::new("field".to_string());
        traj.add_step(make_step(
            Action::tool_call("run_query".into(), input),
            "ok",
        ));

        let redacted = r.redact_trajectory(&traj);
        let serialized = serde_json::to_string(&redacted.steps[0].action).unwrap();
        assert!(!serialized.contains(secret));
        assert!(serialized.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_text_output_action() {
        let secret = "bearer-token-secret-55";
        let r = Redactor::new(vec![secret.to_string()]);

        let mut traj = Trajectory::new("field".to_string());
        traj.add_step(make_step(
            Action::text(format!("I used {secret} to authenticate")),
            "ok",
        ));

        let redacted = r.redact_trajectory(&traj);
        match &redacted.steps[0].action {
            Action::TextOutput { text } => {
                assert!(!text.contains(secret));
                assert_eq!(text, "I used [REDACTED] to authenticate");
            }
            _ => panic!("expected TextOutput"),
        }
    }

    #[test]
    fn test_redact_structured_output() {
        let secret = "output-secret-key-77";
        let r = Redactor::new(vec![secret.to_string()]);

        let mut traj = Trajectory::new("field".to_string());
        traj.set_structured_output(json!({ "token": secret, "count": 3 }));

        let redacted = r.redact_trajectory(&traj);
        let output = redacted.structured_output.unwrap();
        assert_eq!(output["token"], "[REDACTED]");
        assert_eq!(output["count"], 3); // non-string values untouched
    }

    #[test]
    fn test_original_trajectory_is_not_mutated() {
        let secret = "immutable-secret-key-88";
        let r = Redactor::new(vec![secret.to_string()]);

        let mut traj = Trajectory::new("field".to_string());
        traj.add_step(make_step(Action::stop(), &format!("result: {secret}")));

        let _redacted = r.redact_trajectory(&traj);
        assert!(traj.steps[0].result.contains(secret)); // original unchanged
    }
}
