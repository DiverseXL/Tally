"use strict";

// The Tally dashboard. Presentation only: it reads the server's status, asks it
// to start a profile and shows the steps it streams back. Every rule about
// statuses, payments and reports lives on the server.

interface WalletFacts {
  source: string;
  parses: boolean;
}

interface DoctorFacts {
  payer: WalletFacts;
  receiver: WalletFacts;
  out_dir_writable: boolean;
  guard_dir_writable: boolean;
  blocking_guard_files: number;
}

interface StatusPayload {
  version: string;
  environment: string;
  payment_amount_sats: number;
  doctor: DoctorFacts;
  ready: boolean;
  current_run: string | null;
  latest_run: string | null;
}

interface RunEntry {
  run_id: string;
  profile: string;
  exit_code: number;
  started_at: string;
}

interface StepPayload {
  id: string;
  phase: string;
  status: string | null;
  reason: string | null;
}

interface DonePayload {
  run_id: string;
  exit_code: number;
}

interface ErrorBody {
  error: {
    code: string;
    message: string;
  };
}

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (element === null) {
    throw new Error("missing element " + id);
  }
  return element as T;
}

const ui = {
  pill: byId<HTMLSpanElement>("status-pill"),
  detail: byId<HTMLElement>("status-detail"),
  version: byId<HTMLElement>("status-version"),
  payer: byId<HTMLElement>("wallet-payer"),
  receiver: byId<HTMLElement>("wallet-receiver"),
  dirs: byId<HTMLElement>("dirs"),
  guard: byId<HTMLElement>("guard-count"),
  refresh: byId<HTMLButtonElement>("refresh"),
  form: byId<HTMLFormElement>("run-form"),
  profile: byId<HTMLSelectElement>("profile"),
  payBox: byId<HTMLElement>("pay-box"),
  pay: byId<HTMLInputElement>("pay"),
  amount: byId<HTMLInputElement>("amount"),
  confirm: byId<HTMLInputElement>("confirm"),
  clearGuard: byId<HTMLInputElement>("clear-guard"),
  submit: byId<HTMLButtonElement>("start"),
  message: byId<HTMLParagraphElement>("message"),
  runLine: byId<HTMLParagraphElement>("run-line"),
  steps: byId<HTMLTableSectionElement>("steps"),
  stepsEmpty: byId<HTMLElement>("steps-empty"),
  reportHtml: byId<HTMLAnchorElement>("report-html"),
  reportJson: byId<HTMLAnchorElement>("report-json"),
  history: byId<HTMLUListElement>("history"),
  historyEmpty: byId<HTMLElement>("history-empty"),
};

let stream: EventSource | null = null;
let currentRun: string | null = null;
const rows = new Map<string, HTMLTableRowElement>();

function label(value: string): string {
  return value.replace(/_/g, " ");
}

function statusClass(status: string): string {
  return "status status-" + status.toLowerCase().replace(/_/g, "-");
}

function setMessage(text: string, tone: "info" | "error"): void {
  ui.message.textContent = text;
  ui.message.className = tone === "error" ? "message message-error" : "message";
  ui.message.hidden = text.length === 0;
}

function walletText(facts: WalletFacts): string {
  if (!facts.parses) {
    return facts.source === "none" ? "not configured" : "does not parse";
  }
  return "ready from " + facts.source;
}

function setReady(status: StatusPayload): void {
  ui.pill.textContent = status.ready ? "ready" : "not ready";
  ui.pill.className = status.ready ? "pill pill-ready" : "pill pill-blocked";
  ui.detail.textContent = status.ready
    ? "Both wallets parse and both directories are writable."
    : "A wallet is missing or unreadable, or a directory is not writable.";
  ui.version.textContent =
    "Tally " +
    status.version +
    ", environment " +
    status.environment +
    ", payment amount " +
    String(status.payment_amount_sats) +
    " sats";
  ui.payer.textContent = walletText(status.doctor.payer);
  ui.receiver.textContent = walletText(status.doctor.receiver);
  ui.dirs.textContent =
    (status.doctor.out_dir_writable ? "reports writable" : "reports not writable") +
    ", " +
    (status.doctor.guard_dir_writable
      ? "guard directory writable"
      : "guard directory not writable");
  ui.guard.textContent =
    status.doctor.blocking_guard_files === 1
      ? "1 guard file would block a payment"
      : String(status.doctor.blocking_guard_files) +
        " guard files would block a payment";
}

async function errorText(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as ErrorBody;
    return body.error.message + " (" + body.error.code + ")";
  } catch (error) {
    return "request failed with status " + String(response.status);
  }
}

async function refreshStatus(): Promise<void> {
  try {
    const response = await fetch("/api/status");
    if (!response.ok) {
      setMessage(await errorText(response), "error");
      return;
    }
    const status = (await response.json()) as StatusPayload;
    setReady(status);
  } catch (error) {
    setMessage("The server did not answer.", "error");
  }
}

function clearSteps(): void {
  rows.clear();
  ui.steps.replaceChildren();
  ui.stepsEmpty.hidden = false;
}

function renderStep(payload: StepPayload): void {
  ui.stepsEmpty.hidden = true;

  let row = rows.get(payload.id);
  if (row === undefined) {
    row = document.createElement("tr");

    const name = document.createElement("th");
    name.scope = "row";
    name.textContent = payload.id;

    const status = document.createElement("td");
    status.className = "status status-running";
    status.textContent = "in progress";

    const reason = document.createElement("td");
    reason.className = "reason";

    row.append(name, status, reason);
    rows.set(payload.id, row);
    ui.steps.append(row);
  }

  const status = row.children[1];
  const reason = row.children[2];
  if (!(status instanceof HTMLTableCellElement) || !(reason instanceof HTMLTableCellElement)) {
    return;
  }

  if (payload.phase === "finished") {
    const statusName = payload.status === null ? "NOT_TESTED" : payload.status;
    status.className = statusClass(statusName);
    status.textContent = label(statusName);
    reason.textContent = payload.reason === null ? "" : label(payload.reason);
  }
}

function closeStream(): void {
  if (stream !== null) {
    stream.close();
    stream = null;
  }
}

function showReport(runId: string): void {
  ui.reportHtml.href = "/api/runs/" + encodeURIComponent(runId) + "/report.html";
  ui.reportJson.href = "/api/runs/" + encodeURIComponent(runId) + "/report.json";
  ui.reportHtml.hidden = false;
  ui.reportJson.hidden = false;
}

function finish(payload: DonePayload): void {
  closeStream();
  ui.runLine.textContent =
    "Run " + payload.run_id + " finished with exit code " + String(payload.exit_code) + ".";
  showReport(payload.run_id);
  currentRun = null;
  void refreshStatus();
  void loadHistory();
}

function watch(runId: string): void {
  closeStream();
  clearSteps();
  currentRun = runId;
  ui.reportHtml.hidden = true;
  ui.reportJson.hidden = true;
  ui.runLine.textContent = "Run " + runId + " in progress.";

  const source = new EventSource(
    "/api/runs/" + encodeURIComponent(runId) + "/events",
  );
  stream = source;

  source.addEventListener("step", (event: Event) => {
    const message = event as MessageEvent<string>;
    renderStep(JSON.parse(message.data) as StepPayload);
  });

  source.addEventListener("done", (event: Event) => {
    const message = event as MessageEvent<string>;
    finish(JSON.parse(message.data) as DonePayload);
  });

  source.addEventListener("error", () => {
    if (currentRun === runId) {
      ui.runLine.textContent = "The event stream closed before the run finished.";
    }
  });
}

async function startRun(): Promise<void> {
  setMessage("", "info");

  const profile = ui.profile.value;
  const pay = profile === "l402-e2e" && ui.pay.checked;
  let amount: number | null = null;

  if (pay) {
    amount = Number(ui.amount.value);
    if (!Number.isInteger(amount) || amount <= 0) {
      setMessage("The payment amount must be a positive number of sats.", "error");
      return;
    }
  }

  const body = {
    profile,
    approve_payment_sats: amount,
    confirm: pay ? ui.confirm.value : null,
    clear_guard: ui.clearGuard.checked,
  };

  ui.submit.disabled = true;
  try {
    const response = await fetch("/api/runs", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-tally-request": "1",
      },
      body: JSON.stringify(body),
    });

    if (!response.ok) {
      setMessage(await errorText(response), "error");
      return;
    }

    const accepted = (await response.json()) as { run_id: string };
    watch(accepted.run_id);
  } catch (error) {
    setMessage("The server did not answer.", "error");
  } finally {
    ui.submit.disabled = false;
  }
}

function historyEntry(run: RunEntry): HTMLLIElement {
  const item = document.createElement("li");

  const summary = document.createElement("span");
  summary.className = "history-summary";
  summary.textContent =
    run.profile + ", exit code " + String(run.exit_code) + ", " + run.started_at;

  const view = document.createElement("button");
  view.type = "button";
  view.textContent = "Events";
  view.addEventListener("click", () => {
    watch(run.run_id);
  });

  const html = document.createElement("a");
  html.href = "/api/runs/" + encodeURIComponent(run.run_id) + "/report.html";
  html.textContent = "Report";

  const json = document.createElement("a");
  json.href = "/api/runs/" + encodeURIComponent(run.run_id) + "/report.json";
  json.textContent = "JSON";

  item.append(summary, view, html, json);
  return item;
}

async function loadHistory(): Promise<void> {
  try {
    const response = await fetch("/api/runs");
    if (!response.ok) {
      return;
    }
    const runs = (await response.json()) as RunEntry[];
    ui.history.replaceChildren();
    for (const run of runs) {
      ui.history.append(historyEntry(run));
    }
    ui.historyEmpty.hidden = runs.length > 0;
  } catch (error) {
    ui.historyEmpty.hidden = false;
  }
}

function syncForm(): void {
  const pays = ui.profile.value === "l402-e2e";
  ui.payBox.hidden = !pays;
  if (!pays) {
    ui.pay.checked = false;
  }
  ui.confirm.disabled = !ui.pay.checked;
  ui.amount.disabled = !ui.pay.checked;
}

function init(): void {
  ui.refresh.addEventListener("click", () => {
    void refreshStatus();
    void loadHistory();
  });

  ui.form.addEventListener("submit", (event: Event) => {
    event.preventDefault();
    void startRun();
  });

  ui.profile.addEventListener("change", syncForm);
  ui.pay.addEventListener("change", () => {
    syncForm();
    if (ui.pay.checked && ui.confirm.value.length === 0) {
      ui.confirm.placeholder = "PAY " + ui.amount.value + " SATS";
    }
  });

  syncForm();
  clearSteps();
  void refreshStatus();
  void loadHistory();
}

init();
