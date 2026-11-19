//! Run event logs: one per run, replayed to late subscribers and pushed live to
//! the ones already listening.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tally_core::redact::Scrubber;
use tally_runner::{StepEvent, StepObserver, StepPhase};
use tokio::sync::broadcast;

/// How many finished run logs stay reachable through the event route.
pub const MAX_LOGS: usize = 16;

/// One step event, as the stream carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepPayload {
    /// The test id.
    pub id: String,
    /// `started` or `finished`.
    pub phase: &'static str,
    /// The status once finished.
    pub status: Option<&'static str>,
    /// The reason code once finished.
    pub reason: Option<String>,
}

/// The final event of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DonePayload {
    /// The run id, which is also the report directory name.
    pub run_id: String,
    /// The exit code the run produced.
    pub exit_code: i32,
}

/// One event in a run log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoggedEvent {
    /// A step started or finished.
    Step(StepPayload),
    /// The run is over.
    Done(DonePayload),
}

impl LoggedEvent {
    /// The SSE event name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Step(_) => "step",
            Self::Done(_) => "done",
        }
    }

    /// The JSON body of the SSE event.
    #[must_use]
    pub fn data(&self) -> String {
        let value = match self {
            Self::Step(payload) => serde_json::to_value(payload),
            Self::Done(payload) => serde_json::to_value(payload),
        };

        value
            .ok()
            .and_then(|value| serde_json::to_string(&value).ok())
            .unwrap_or_else(|| String::from("{}"))
    }
}

impl StepPayload {
    fn from_event(event: &StepEvent, scrubber: &Scrubber) -> Self {
        Self {
            id: String::from(event.id),
            phase: match event.phase {
                StepPhase::Started => "started",
                StepPhase::Finished => "finished",
            },
            status: event.status.map(tally_core::status::Status::as_str),
            reason: event.reason.as_deref().map(|reason| scrubber.scrub(reason)),
        }
    }
}

struct LogState {
    events: Vec<LoggedEvent>,
    done: bool,
}

/// The events of one run.
pub struct RunLog {
    id: String,
    state: Mutex<LogState>,
    tx: broadcast::Sender<LoggedEvent>,
}

impl RunLog {
    fn new(id: String) -> Self {
        let (tx, _receiver) = broadcast::channel(256);

        Self {
            id,
            state: Mutex::new(LogState {
                events: Vec::new(),
                done: false,
            }),
            tx,
        }
    }

    /// The run this log belongs to.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    fn push(&self, event: LoggedEvent) {
        let finished = matches!(event, LoggedEvent::Done(_));

        {
            let mut state = self.state.lock().expect("run log");
            if state.done {
                return;
            }
            state.events.push(event.clone());
            if finished {
                state.done = true;
            }
        }

        // Nobody listening is normal: the log is the source for late subscribers.
        let _ = self.tx.send(event);
    }

    /// Records the end of the run.
    pub fn finish(&self, run_id: &str, exit_code: i32) {
        self.push(LoggedEvent::Done(DonePayload {
            run_id: run_id.to_owned(),
            exit_code,
        }));
    }

    /// The events recorded so far.
    #[must_use]
    pub fn events(&self) -> Vec<LoggedEvent> {
        self.state.lock().expect("run log").events.clone()
    }

    /// The events so far plus a live receiver, taken under one lock so no event
    /// can slip between the replay and the subscription.
    #[must_use]
    pub fn snapshot_and_subscribe(
        &self,
    ) -> (Vec<LoggedEvent>, broadcast::Receiver<LoggedEvent>, bool) {
        let state = self.state.lock().expect("run log");
        let receiver = self.tx.subscribe();

        (state.events.clone(), receiver, state.done)
    }
}

/// Hands every step of a run to its [`RunLog`].
#[derive(Clone)]
pub struct EventSink {
    log: Arc<RunLog>,
    scrubber: Arc<Scrubber>,
}

impl EventSink {
    /// Builds a sink writing to one log.
    #[must_use]
    pub fn new(log: Arc<RunLog>, scrubber: Arc<Scrubber>) -> Self {
        Self { log, scrubber }
    }

    /// The log this sink writes to.
    #[must_use]
    pub fn log(&self) -> &Arc<RunLog> {
        &self.log
    }
}

impl StepObserver for EventSink {
    fn step(&mut self, event: &StepEvent) {
        self.log.push(LoggedEvent::Step(StepPayload::from_event(
            event,
            &self.scrubber,
        )));
    }
}

struct RunsState {
    current: Option<String>,
    logs: VecDeque<Arc<RunLog>>,
}

/// The runs this process started, and whether one is active.
pub struct Runs {
    state: Mutex<RunsState>,
}

impl Runs {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RunsState {
                current: None,
                logs: VecDeque::new(),
            }),
        }
    }

    /// Marks a run as active and returns its log, or `None` when one is already
    /// running. The check and the claim happen under one lock.
    pub fn begin(&self, id: &str) -> Option<Arc<RunLog>> {
        let mut state = self.state.lock().expect("runs");

        if state.current.is_some() {
            return None;
        }

        let log = Arc::new(RunLog::new(id.to_owned()));
        state.current = Some(id.to_owned());
        state.logs.push_front(Arc::clone(&log));
        while state.logs.len() > MAX_LOGS {
            state.logs.pop_back();
        }

        Some(log)
    }

    /// Marks the run with `id` as no longer active.
    pub fn finish(&self, id: &str) {
        let mut state = self.state.lock().expect("runs");

        if state.current.as_deref() == Some(id) {
            state.current = None;
        }
    }

    /// The id of the run in progress, if any.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.state.lock().expect("runs").current.clone()
    }

    /// The log of one run this process started.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<RunLog>> {
        self.state
            .lock()
            .expect("runs")
            .logs
            .iter()
            .find(|log| log.id() == id)
            .cloned()
    }
}

impl Default for Runs {
    fn default() -> Self {
        Self::new()
    }
}
