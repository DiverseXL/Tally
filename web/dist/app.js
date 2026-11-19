"use strict";

// Shows whether the server is ready. The dashboard task replaces this file with
// the compiled TypeScript app; both read the same /api/status route.
async function refreshStatus() {
  const target = document.getElementById("status");
  if (target === null) {
    return;
  }

  try {
    const response = await fetch("/api/status");
    const body = await response.json();
    target.textContent = body.ready ? "Tally is ready." : "Tally is not ready.";
  } catch (error) {
    target.textContent = "Tally is not reachable.";
  }
}

refreshStatus();
