//! Observer tests: every step is reported once when it starts and once when it
//! finishes, in profile order.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{FakeConfig, FakePair};
use tally_runner::{Profile, RunConfig, StepEvent, StepObserver, StepPhase, run_with_observer};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tally-observer-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn config(dir: &Path) -> RunConfig {
    RunConfig {
        out_dir: dir.join("reports"),
        guard_dir: dir.join("guards"),
        ..RunConfig::default()
    }
}

struct Recorder {
    events: Vec<StepEvent>,
}

impl StepObserver for Recorder {
    fn step(&mut self, event: &StepEvent) {
        self.events.push(event.clone());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_arrive_in_profile_order_and_every_test_appears_twice() {
    let dir = workdir("order");
    let pair = FakePair::new(FakeConfig::default());
    let mut payer = pair.payer.clone();
    let mut recorder = Recorder { events: Vec::new() };

    run_with_observer(
        Profile::L402E2e,
        &mut payer,
        Arc::new(pair.receiver.clone()),
        &config(&dir),
        &mut recorder,
    )
    .await
    .expect("run finishes");

    let expected = Profile::L402E2e
        .tests()
        .iter()
        .map(|spec| spec.id)
        .collect::<Vec<&str>>();

    let started = recorder
        .events
        .iter()
        .filter(|event| event.phase == StepPhase::Started)
        .map(|event| event.id)
        .collect::<Vec<&str>>();
    assert_eq!(started, expected);

    let finished = recorder
        .events
        .iter()
        .filter(|event| event.phase == StepPhase::Finished)
        .map(|event| event.id)
        .collect::<Vec<&str>>();
    assert_eq!(finished, expected);

    assert_eq!(recorder.events.len(), expected.len() * 2);

    // Each step alternates: started, then finished with a status and a reason.
    for pair in recorder.events.chunks(2) {
        assert_eq!(pair[0].phase, StepPhase::Started);
        assert!(pair[0].status.is_none());
        assert!(pair[0].reason.is_none());
        assert_eq!(pair[1].phase, StepPhase::Finished);
        assert_eq!(pair[0].id, pair[1].id);
        assert!(pair[1].status.is_some());
        assert!(pair[1].reason.is_some());
    }

    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_default_run_reports_nothing_to_anyone() {
    let dir = workdir("noop");
    let pair = FakePair::new(FakeConfig::default());
    let mut payer = pair.payer.clone();

    // No observer argument at all: the wrapper passes NoopObserver.
    let outcome = tally_runner::run(
        Profile::Basic,
        &mut payer,
        Arc::new(pair.receiver.clone()),
        &config(&dir),
    )
    .await
    .expect("run finishes");

    assert_eq!(outcome.report.tests.len(), Profile::Basic.tests().len());

    let _ = fs::remove_dir_all(&dir);
}
