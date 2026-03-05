use portlang_core::{Action, Cost, RunOutcome, Trajectory, TrajectoryStep};
use portlang_trajectory::{
    diff_trajectories, FilesystemStore, ReplaySession, TrajectoryQuery, TrajectoryStore,
};
use tempfile::TempDir;

/// Test trajectory query functionality
#[test]
fn test_trajectory_query() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemStore::with_path(temp_dir.path());

    // Create test trajectories
    let mut traj1 = Trajectory::new("test-field".to_string());
    traj1.finish(RunOutcome::Converged {
        message: "Success".to_string(),
    });

    let mut traj2 = Trajectory::new("test-field".to_string());
    traj2.finish(RunOutcome::BudgetExhausted {
        reason: "Out of tokens".to_string(),
    });

    let mut traj3 = Trajectory::new("other-field".to_string());
    traj3.finish(RunOutcome::Converged {
        message: "Success".to_string(),
    });

    // Save trajectories
    store.save(&traj1).unwrap();
    store.save(&traj2).unwrap();
    store.save(&traj3).unwrap();

    // Test query for converged only
    let query = TrajectoryQuery::new().only_converged();
    let results = store.query(&query).unwrap();
    assert_eq!(results.len(), 2);

    // Test query for failed only
    let query = TrajectoryQuery::new().only_failed();
    let results = store.query(&query).unwrap();
    assert_eq!(results.len(), 1);

    // Test query by field name
    let query = TrajectoryQuery::new().field("test-field");
    let results = store.query(&query).unwrap();
    assert_eq!(results.len(), 2);

    // Test query with limit
    let query = TrajectoryQuery::new().limit(2);
    let results = store.query(&query).unwrap();
    assert_eq!(results.len(), 2);
}

/// Test trajectory replay session
#[test]
fn test_replay_session() {
    let mut trajectory = Trajectory::new("test-field".to_string());

    // Add test steps
    for i in 1..=5 {
        trajectory.add_step(TrajectoryStep::new(
            i,
            Action::TextOutput {
                text: format!("Step {}", i),
            },
            "OK".to_string(),
            false,
            Cost::from_microdollars(100),
            100,
        ));
    }

    trajectory.finish(RunOutcome::Converged {
        message: "Done".to_string(),
    });

    let mut session = ReplaySession::new(trajectory);

    // Test navigation
    assert_eq!(session.current_step_number(), 0);
    assert!(session.is_at_start());

    session.next();
    assert_eq!(session.current_step_number(), 1);

    session.goto(3);
    assert_eq!(session.current_step_number(), 3);

    session.prev();
    assert_eq!(session.current_step_number(), 2);

    session.reset();
    assert_eq!(session.current_step_number(), 0);
}

/// Test trajectory diff
#[test]
fn test_trajectory_diff() {
    // Create two similar trajectories
    let mut traj_a = Trajectory::new("field-a".to_string());
    let mut traj_b = Trajectory::new("field-b".to_string());

    // Add identical first steps
    for traj in [&mut traj_a, &mut traj_b] {
        traj.add_step(TrajectoryStep::new(
            1,
            Action::TextOutput {
                text: "Step 1".to_string(),
            },
            "OK".to_string(),
            false,
            Cost::from_microdollars(100),
            100,
        ));
    }

    // Add different second steps
    traj_a.add_step(TrajectoryStep::new(
        2,
        Action::TextOutput {
            text: "Step 2A".to_string(),
        },
        "OK".to_string(),
        false,
        Cost::from_microdollars(100),
        100,
    ));

    traj_b.add_step(TrajectoryStep::new(
        2,
        Action::Stop,
        "Stopped".to_string(),
        false,
        Cost::from_microdollars(100),
        100,
    ));

    traj_a.finish(RunOutcome::Converged {
        message: "Done A".to_string(),
    });
    traj_b.finish(RunOutcome::Converged {
        message: "Done B".to_string(),
    });

    // Compute diff
    let diff = diff_trajectories(&traj_a, &traj_b);

    // Should diverge at step 1 (second step, 0-indexed)
    assert_eq!(diff.divergence_point, Some(1));
    assert!(diff.divergence_reason.is_some());
}

/// Test list_all functionality
#[test]
fn test_list_all() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemStore::with_path(temp_dir.path());

    // Create trajectories in different fields
    let mut traj1 = Trajectory::new("field-a".to_string());
    traj1.finish(RunOutcome::Converged {
        message: "Done".to_string(),
    });

    let mut traj2 = Trajectory::new("field-b".to_string());
    traj2.finish(RunOutcome::Converged {
        message: "Done".to_string(),
    });

    let mut traj3 = Trajectory::new("field-c".to_string());
    traj3.finish(RunOutcome::Converged {
        message: "Done".to_string(),
    });

    store.save(&traj1).unwrap();
    store.save(&traj2).unwrap();
    store.save(&traj3).unwrap();

    // Test list_all
    let all = store.list_all().unwrap();
    assert_eq!(all.len(), 3);

    // Test list for specific field
    let field_a = store.list("field-a").unwrap();
    assert_eq!(field_a.len(), 1);
}

/// Test find_by_filename
#[test]
fn test_find_by_filename() {
    let temp_dir = TempDir::new().unwrap();
    let store = FilesystemStore::with_path(temp_dir.path());

    let mut traj = Trajectory::new("test-field".to_string());
    traj.finish(RunOutcome::Converged {
        message: "Done".to_string(),
    });

    let filename = traj.id.filename();
    store.save(&traj).unwrap();

    // Test finding by filename
    let found = store.find_by_filename(&filename).unwrap();
    assert_eq!(found.field_name, "test-field");

    // Test finding by filename without .json extension
    let filename_no_ext = filename.trim_end_matches(".json");
    let found = store.find_by_filename(filename_no_ext).unwrap();
    assert_eq!(found.field_name, "test-field");

    // Test not found
    let result = store.find_by_filename("nonexistent");
    assert!(result.is_err());
}
