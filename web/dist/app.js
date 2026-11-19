"use strict";
function byId(id) {
    const element = document.getElementById(id);
    if (element === null) {
        throw new Error("missing element " + id);
    }
    return element;
}
const ui = {
    pill: byId("status-pill"),
    detail: byId("status-detail"),
    version: byId("status-version"),
    payer: byId("wallet-payer"),
    receiver: byId("wallet-receiver"),
    dirs: byId("dirs"),
    guard: byId("guard-count"),
    refresh: byId("refresh"),
    form: byId("run-form"),
    profile: byId("profile"),
    payBox: byId("pay-box"),
    pay: byId("pay"),
    amount: byId("amount"),
    confirm: byId("confirm"),
    clearGuard: byId("clear-guard"),
    submit: byId("start"),
    message: byId("message"),
    runLine: byId("run-line"),
    steps: byId("steps"),
    stepsEmpty: byId("steps-empty"),
    reportHtml: byId("report-html"),
    reportJson: byId("report-json"),
    history: byId("history"),
    historyEmpty: byId("history-empty"),
};
let stream = null;
let currentRun = null;
const rows = new Map();
function label(value) {
    return value.replace(/_/g, " ");
}
function statusClass(status) {
    return "status status-" + status.toLowerCase().replace(/_/g, "-");
}
function setMessage(text, tone) {
    ui.message.textContent = text;
    ui.message.className = tone === "error" ? "message message-error" : "message";
    ui.message.hidden = text.length === 0;
}
function walletText(facts) {
    if (!facts.parses) {
        return facts.source === "none" ? "not configured" : "does not parse";
    }
    return "ready from " + facts.source;
}
function setReady(status) {
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
async function errorText(response) {
    try {
        const body = (await response.json());
        return body.error.message + " (" + body.error.code + ")";
    }
    catch (error) {
        return "request failed with status " + String(response.status);
    }
}
async function refreshStatus() {
    try {
        const response = await fetch("/api/status");
        if (!response.ok) {
            setMessage(await errorText(response), "error");
            return;
        }
        const status = (await response.json());
        setReady(status);
    }
    catch (error) {
        setMessage("The server did not answer.", "error");
    }
}
function clearSteps() {
    rows.clear();
    ui.steps.replaceChildren();
    ui.stepsEmpty.hidden = false;
}
function renderStep(payload) {
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
function closeStream() {
    if (stream !== null) {
        stream.close();
        stream = null;
    }
}
function showReport(runId) {
    ui.reportHtml.href = "/api/runs/" + encodeURIComponent(runId) + "/report.html";
    ui.reportJson.href = "/api/runs/" + encodeURIComponent(runId) + "/report.json";
    ui.reportHtml.hidden = false;
    ui.reportJson.hidden = false;
}
function finish(payload) {
    closeStream();
    ui.runLine.textContent =
        "Run " + payload.run_id + " finished with exit code " + String(payload.exit_code) + ".";
    showReport(payload.run_id);
    currentRun = null;
    void refreshStatus();
    void loadHistory();
}
function watch(runId) {
    closeStream();
    clearSteps();
    currentRun = runId;
    ui.reportHtml.hidden = true;
    ui.reportJson.hidden = true;
    ui.runLine.textContent = "Run " + runId + " in progress.";
    const source = new EventSource("/api/runs/" + encodeURIComponent(runId) + "/events");
    stream = source;
    source.addEventListener("step", (event) => {
        const message = event;
        renderStep(JSON.parse(message.data));
    });
    source.addEventListener("done", (event) => {
        const message = event;
        finish(JSON.parse(message.data));
    });
    source.addEventListener("error", () => {
        if (currentRun === runId) {
            ui.runLine.textContent = "The event stream closed before the run finished.";
        }
    });
}
async function startRun() {
    setMessage("", "info");
    const profile = ui.profile.value;
    const pay = profile === "l402-e2e" && ui.pay.checked;
    let amount = null;
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
        const accepted = (await response.json());
        watch(accepted.run_id);
    }
    catch (error) {
        setMessage("The server did not answer.", "error");
    }
    finally {
        ui.submit.disabled = false;
    }
}
function historyEntry(run) {
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
async function loadHistory() {
    try {
        const response = await fetch("/api/runs");
        if (!response.ok) {
            return;
        }
        const runs = (await response.json());
        ui.history.replaceChildren();
        for (const run of runs) {
            ui.history.append(historyEntry(run));
        }
        ui.historyEmpty.hidden = runs.length > 0;
    }
    catch (error) {
        ui.historyEmpty.hidden = false;
    }
}
function syncForm() {
    const pays = ui.profile.value === "l402-e2e";
    ui.payBox.hidden = !pays;
    if (!pays) {
        ui.pay.checked = false;
    }
    ui.confirm.disabled = !ui.pay.checked;
    ui.amount.disabled = !ui.pay.checked;
}
function init() {
    ui.refresh.addEventListener("click", () => {
        void refreshStatus();
        void loadHistory();
    });
    ui.form.addEventListener("submit", (event) => {
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
export {};
