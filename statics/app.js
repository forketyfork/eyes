const state = {
    page: 1,
    pageSize: 10,
    sort: "time",
    order: "desc",
    total: 0,
    totalGroups: 0,
    totalPages: 0,
    expanded: new Set(),
    details: new Map(),
    detailErrors: new Map(),
    detailRequests: new Map(),
};

const elements = {
    body: document.querySelector("#alerts-body"),
    tableView: document.querySelector("#table-view"),
    loading: document.querySelector("#loading-state"),
    error: document.querySelector("#error-state"),
    errorMessage: document.querySelector("#error-message"),
    empty: document.querySelector("#empty-state"),
    refresh: document.querySelector("#refresh-button"),
    retry: document.querySelector("#retry-button"),
    previous: document.querySelector("#previous-page"),
    next: document.querySelector("#next-page"),
    pageNumbers: document.querySelector("#page-numbers"),
    pageSize: document.querySelector("#page-size"),
    range: document.querySelector("#range-label"),
    total: document.querySelector("#total-count"),
    critical: document.querySelector("#critical-count"),
    warning: document.querySelector("#warning-count"),
    info: document.querySelector("#info-count"),
};

function escapeHtml(value) {
    return String(value ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#039;");
}

function words(value) {
    return String(value || "unknown").replaceAll("_", " ");
}

function formatTime(value) {
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return { relative: "Unknown", exact: value };
    const difference = Date.now() - date.valueOf();
    const minutes = Math.round(difference / 60000);
    let relative;
    if (Math.abs(minutes) < 1) relative = "Just now";
    else if (Math.abs(minutes) < 60) relative = `${Math.abs(minutes)}m ${minutes >= 0 ? "ago" : "ahead"}`;
    else if (Math.abs(minutes) < 1440) relative = `${Math.round(Math.abs(minutes) / 60)}h ${minutes >= 0 ? "ago" : "ahead"}`;
    else relative = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(date);
    return {
        relative,
        exact: new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date),
    };
}

function confidenceLevel(value) {
    const normalized = String(value || "").toLowerCase();
    if (normalized.includes("high")) return 3;
    if (normalized.includes("medium") || normalized.includes("moderate")) return 2;
    return 1;
}

function listMarkup(items, className = "") {
    if (!items?.length) return '<p class="detail-copy">No additional context was recorded.</p>';
    return `<ul class="detail-list ${className}">${items.map((item) => `<li>${escapeHtml(item)}</li>`).join("")}</ul>`;
}

function triggerEvidenceMarkup(alert) {
    const logs = alert.log_events || [];
    const metrics = alert.metrics_events || [];
    const diskEvents = alert.disk_events || [];
    const processes = [...new Set(logs.map((event) => event.process).filter(Boolean))];
    const processMarkup = processes.length
        ? `<div class="affected-apps"><span>Affected ${processes.length === 1 ? "app" : "apps"}</span>${processes.map((process) => `<strong>${escapeHtml(process)}</strong>`).join("")}</div>`
        : "";
    const logMarkup = logs.length
        ? `<div class="raw-events">${logs.map((event) => {
            const timestamp = formatTime(event.timestamp).exact;
            const source = [event.subsystem, event.category].filter(Boolean).join(" / ");
            return `<article class="raw-event raw-log">
                <div class="raw-event-meta">
                    <span>${escapeHtml(timestamp)}</span>
                    <span class="raw-event-level level-${escapeHtml(event.message_type)}">${escapeHtml(event.message_type)}</span>
                    <span>${escapeHtml(event.process || "Unknown process")}${event.process_id ? ` · PID ${event.process_id}` : ""}</span>
                    ${source ? `<span>${escapeHtml(source)}</span>` : ""}
                </div>
                <code>${escapeHtml(event.message)}</code>
            </article>`;
        }).join("")}</div>`
        : "";
    const metricMarkup = metrics.length
        ? `<div class="observation-grid">${metrics.map((event) => `<article class="observation-card">
            <span>${escapeHtml(formatTime(event.timestamp).exact)}</span>
            <strong>${Number(event.cpu_usage_percent).toFixed(1)}% CPU</strong>
            <p>${Number(event.memory_used_mb).toFixed(0)} MB memory · ${escapeHtml(words(event.memory_pressure))} pressure</p>
        </article>`).join("")}</div>`
        : "";
    const diskMarkup = diskEvents.length
        ? `<div class="observation-grid">${diskEvents.map((event) => `<article class="observation-card">
            <span>${escapeHtml(event.disk_name)}</span>
            <strong>${Number(event.read_kb_per_sec).toFixed(1)} KB/s read</strong>
            <p>${Number(event.write_kb_per_sec).toFixed(1)} KB/s write · ${Number(event.read_ops_per_sec + event.write_ops_per_sec).toFixed(1)} ops/s</p>
        </article>`).join("")}</div>`
        : "";
    if (!logs.length && !metrics.length && !diskEvents.length) {
        return `<section class="detail-section">
            <p class="detail-label">Trigger evidence</p>
            <p class="detail-copy context-unavailable">Raw trigger evidence was not captured for this older alert.</p>
        </section>`;
    }
    return `<section class="detail-section trigger-evidence">
        <p class="detail-label">Trigger evidence</p>
        ${processMarkup}
        ${logMarkup}
        ${metricMarkup}
        ${diskMarkup}
    </section>`;
}

function agentReviewsMarkup(alert) {
    const reviews = alert.agent_reviews || [];
    if (!reviews.length) return "";
    return `<section class="detail-section agent-review-section">
        <p class="detail-label">Agent history</p>
        <div class="agent-review-list">${reviews.map((review) => `<article class="agent-review agent-review-${escapeHtml(review.review_type)}">
            <div><strong>${escapeHtml(review.agent_name)}</strong><span>${escapeHtml(words(review.review_type))} · ${escapeHtml(formatTime(review.created_at).exact)}</span></div>
            <p>${escapeHtml(review.body)}</p>
        </article>`).join("")}</div>
    </section>`;
}

function similarAlertsMarkup(alert) {
    const similarAlerts = alert.similar_alerts || [];
    if (!similarAlerts.length) return "";
    return `<details class="similar-alerts-section">
        <summary>
            <span><strong>${similarAlerts.length} similar ${similarAlerts.length === 1 ? "alert" : "alerts"}</strong><small>Grouped under signal ${String(alert.id).padStart(4, "0")}</small></span>
            <span class="similar-alerts-action">Show grouped signals</span>
        </summary>
        <div class="similar-alert-list">${similarAlerts.map((similar) => {
            const time = formatTime(similar.assessed_at);
            const severity = ["critical", "warning", "info"].includes(similar.severity) ? similar.severity : "info";
            return `<article class="similar-alert-card">
                <header>
                    <span class="severity-badge severity-${severity}">${escapeHtml(severity)}</span>
                    <div><strong>${escapeHtml(similar.summary)}</strong><span>Signal ${String(similar.id).padStart(4, "0")} · ${escapeHtml(time.exact)}</span></div>
                    <span class="resolution-badge resolution-${escapeHtml(similar.resolution_status || "open")}">${escapeHtml(words(similar.resolution_status || "open"))}</span>
                </header>
                <div class="similar-alert-body">
                    <div><p class="detail-label">Why it was raised</p><p>${escapeHtml(similar.trigger_reason)}</p></div>
                    <div><p class="detail-label">Likely root cause</p><p>${escapeHtml(similar.root_cause || "No root cause was established.")}</p></div>
                </div>
                ${agentReviewsMarkup(similar)}
            </article>`;
        }).join("")}</div>
    </details>`;
}

function detailMarkup(alert) {
    if (alert.analysis_status !== "analyzed") {
        const failed = alert.analysis_status === "failed";
        const notDone = alert.analysis_status === "not_done";
        const retryable = failed || notDone;
        const calloutClass = failed ? "analysis-callout-failed" : notDone ? "analysis-callout-not-done" : "";
        const calloutMark = failed ? "!" : notDone ? "—" : "…";
        const calloutLabel = failed ? "Analysis failed" : notDone ? "Analysis not done" : "Analysis pending";
        const calloutMessage = notDone
            ? "Automatic analysis was not run. You can start it manually."
            : "Eyes is waiting for the AI analyzer to complete this assessment.";
        return `
            <div class="details-shell">
                <div class="details-clip">
                    <div class="details-panel candidate-details">
                        <div>
                            <section class="detail-section">
                                <p class="detail-label">Why this alert was raised</p>
                                <p class="detail-copy">${escapeHtml(alert.trigger_reason)}</p>
                            </section>
                            ${triggerEvidenceMarkup(alert)}
                            <section class="detail-section">
                                <p class="detail-label">Trigger context</p>
                                <div class="delivery-card">
                                    <div class="delivery-line"><span>Rule</span><strong>${escapeHtml(alert.triggered_by)}</strong></div>
                                    <div class="delivery-line"><span>Source</span><strong>${escapeHtml(alert.trigger_source || "System-wide")}</strong></div>
                                    <div class="delivery-line"><span>Expected severity</span><strong>${escapeHtml(alert.severity)}</strong></div>
                                </div>
                            </section>
                        </div>
                        <div>
                            <section class="analysis-callout ${calloutClass}">
                                <span class="analysis-callout-mark">${calloutMark}</span>
                                <div>
                                    <p class="detail-label">${calloutLabel}</p>
                                    <p>${escapeHtml(alert.analysis_failure || calloutMessage)}</p>
                                    ${retryable ? `<button class="analyze-button" type="button" data-analyze-id="${alert.id}">Analyze now</button>
                                    <p class="analysis-action-error" aria-live="polite" hidden></p>` : ""}
                                </div>
                            </section>
                            <section class="detail-section">
                                <p class="detail-label">Contributing observations</p>
                                <div class="quality-grid candidate-counts">
                                    <div class="quality-card"><span>Logs</span><strong>${alert.log_event_count}</strong></div>
                                    <div class="quality-card"><span>Metrics</span><strong>${alert.metrics_event_count}</strong></div>
                                    <div class="quality-card"><span>Disk samples</span><strong>${alert.disk_event_count}</strong></div>
                                </div>
                            </section>
                            ${agentReviewsMarkup(alert)}
                        </div>
                    </div>
                    ${similarAlertsMarkup(alert)}
                </div>
            </div>`;
    }
    const delivered = alert.delivered_at ? formatTime(alert.delivered_at).exact : "Not delivered";
    return `
        <div class="details-shell">
            <div class="details-clip">
                <div class="details-panel">
                    <div>
                        ${triggerEvidenceMarkup(alert)}
                        <section class="detail-section">
                            <p class="detail-label">Likely root cause</p>
                            <p class="detail-copy">${escapeHtml(alert.root_cause || "No root cause was established.")}</p>
                        </section>
                        <section class="detail-section">
                            <p class="detail-label">Recommended actions</p>
                            ${listMarkup(alert.recommendations)}
                        </section>
                        <section class="detail-section">
                            <p class="detail-label">Supporting evidence</p>
                            ${listMarkup(alert.evidence, "evidence-list")}
                        </section>
                    </div>
                    <div>
                        <section class="detail-section">
                            <p class="detail-label">Assessment quality</p>
                            <div class="quality-grid">
                                <div class="quality-card"><span>Observation</span><strong>${escapeHtml(alert.observation_confidence)}</strong></div>
                                <div class="quality-card"><span>Diagnosis</span><strong>${escapeHtml(alert.diagnosis_confidence)}</strong></div>
                            </div>
                        </section>
                        <section class="detail-section">
                            <p class="detail-label">Delivery record</p>
                            <div class="delivery-card">
                                <div class="delivery-line"><span>Status</span><strong>${escapeHtml(words(alert.status))}</strong></div>
                                <div class="delivery-line"><span>Resolution</span><strong>${escapeHtml(words(alert.resolution_status || "open"))}</strong></div>
                                <div class="delivery-line"><span>Delivered</span><strong>${escapeHtml(delivered)}</strong></div>
                                <div class="delivery-line"><span>Notification</span><strong>${escapeHtml(alert.notification_title || "Not created")}</strong></div>
                                ${alert.failure_message ? `<p class="failure-copy">${escapeHtml(alert.failure_message)}</p>` : ""}
                            </div>
                        </section>
                        <section class="detail-section">
                            <p class="detail-label">Known limitations</p>
                            ${listMarkup(alert.limitations, "limitations-list")}
                        </section>
                        ${agentReviewsMarkup(alert)}
                    </div>
                </div>
                ${similarAlertsMarkup(alert)}
            </div>
        </div>`;
}

function detailLoadingMarkup() {
    return `<div class="details-shell">
        <div class="details-clip">
            <div class="detail-load-state" role="status">
                <span class="detail-loader" aria-hidden="true"><i></i><i></i><i></i></span>
                <span>Loading alert evidence…</span>
            </div>
        </div>
    </div>`;
}

function detailErrorMarkup(id, message) {
    return `<div class="details-shell">
        <div class="details-clip">
            <div class="detail-load-state detail-load-error">
                <span>Could not load this alert: ${escapeHtml(message)}</span>
                <button type="button" data-retry-detail="${id}">Try again</button>
            </div>
        </div>
    </div>`;
}

function currentDetailMarkup(id) {
    if (state.details.has(id)) return detailMarkup(state.details.get(id));
    if (state.detailErrors.has(id)) return detailErrorMarkup(id, state.detailErrors.get(id));
    return detailLoadingMarkup();
}

function rowMarkup(alert, index) {
    const severity = ["critical", "warning", "info"].includes(alert.severity) ? alert.severity : "info";
    const analysisClass = words(alert.analysis_status).replaceAll(" ", "-");
    const analysisLabel = alert.analysis_status === "not_done" ? "Not done" : words(alert.analysis_status);
    const time = formatTime(alert.assessed_at);
    const level = confidenceLevel(alert.diagnosis_confidence);
    const expanded = state.expanded.has(alert.id);
    const analyzed = alert.analysis_status === "analyzed";
    const source = alert.trigger_source || alert.triggered_by;
    const similarCount = alert.similar_alert_count || 0;
    const confidenceMarkup = analyzed
        ? `<span class="confidence">${escapeHtml(alert.diagnosis_confidence)}</span>
           <span class="confidence-meter" aria-hidden="true"><i class="${level >= 1 ? "on" : ""}"></i><i class="${level >= 2 ? "on" : ""}"></i><i class="${level >= 3 ? "on" : ""}"></i></span>`
        : '<span class="confidence confidence-unavailable">—</span>';
    return `
        <tr class="alert-row${expanded ? " expanded" : ""}" data-alert-id="${alert.id}" style="--row-index:${index}" aria-selected="${expanded}">
            <td class="severity-cell"><span class="severity-badge severity-${severity}">${escapeHtml(severity)}</span></td>
            <td>
                <button class="alert-trigger" type="button" aria-expanded="${expanded}" aria-controls="alert-details-${alert.id}">
                    <span class="alert-title-line"><span class="alert-summary">${escapeHtml(alert.summary)}</span>${similarCount ? `<span class="group-count">+${similarCount} similar</span>` : ""}</span>
                    <span class="alert-id">${escapeHtml(source)} · Signal ${String(alert.id).padStart(4, "0")}</span>
                </button>
            </td>
            <td class="status-cell"><span class="status-stack"><span class="status-badge analysis-${analysisClass}">${escapeHtml(analysisLabel)}</span><span class="resolution-badge resolution-${escapeHtml(alert.resolution_status || "open")}">${escapeHtml(words(alert.resolution_status || "open"))}</span></span></td>
            <td class="confidence-cell">
                ${confidenceMarkup}
            </td>
            <td class="time-cell"><span>${escapeHtml(time.relative)}</span><small>${escapeHtml(time.exact)}</small></td>
        </tr>
        <tr class="details-row${expanded ? " open" : ""}" id="alert-details-${alert.id}" data-details-id="${alert.id}">
            <td colspan="5">${expanded ? currentDetailMarkup(alert.id) : ""}</td>
        </tr>`;
}

function renderAlerts(alerts) {
    alerts.forEach((alert) => {
        const cached = state.details.get(alert.id);
        if (cached && cached.updated_at !== alert.updated_at) {
            state.details.delete(alert.id);
            state.detailErrors.delete(alert.id);
        }
    });
    elements.body.innerHTML = alerts.map(rowMarkup).join("");
    elements.body.querySelectorAll(".alert-row").forEach((row) => {
        row.addEventListener("click", () => toggleAlert(Number(row.dataset.alertId)));
    });
    elements.body.querySelectorAll(".details-row").forEach((detailsRow) => {
        bindDetailActions(detailsRow);
        const id = Number(detailsRow.dataset.detailsId);
        if (state.expanded.has(id) && !state.details.has(id) && !state.detailErrors.has(id)) {
            loadAlertDetails(id);
        }
    });
}

function bindDetailActions(detailsRow) {
    detailsRow.querySelectorAll(".analyze-button").forEach((button) => {
        button.addEventListener("click", (event) => {
            event.stopPropagation();
            analyzeAlert(Number(button.dataset.analyzeId), button);
        });
    });
    detailsRow.querySelectorAll("[data-retry-detail]").forEach((button) => {
        button.addEventListener("click", () => {
            const id = Number(button.dataset.retryDetail);
            state.detailErrors.delete(id);
            loadAlertDetails(id);
        });
    });
}

function renderAlertDetails(id, markup) {
    if (!state.expanded.has(id)) return;
    const detailsRow = elements.body.querySelector(`[data-details-id="${id}"]`);
    if (!detailsRow) return;
    detailsRow.querySelector("td").innerHTML = markup;
    bindDetailActions(detailsRow);
}

async function loadAlertDetails(id) {
    if (state.details.has(id)) {
        renderAlertDetails(id, detailMarkup(state.details.get(id)));
        return;
    }
    if (state.detailRequests.has(id)) return state.detailRequests.get(id);

    state.detailErrors.delete(id);
    renderAlertDetails(id, detailLoadingMarkup());
    const request = (async () => {
        try {
            const response = await fetch(`/api/alerts/${id}`, { headers: { Accept: "application/json" } });
            if (!response.ok) {
                const error = await response.json().catch(() => ({}));
                throw new Error(error.message || `Request failed with status ${response.status}`);
            }
            const alert = await response.json();
            state.details.set(id, alert);
            renderAlertDetails(id, detailMarkup(alert));
        } catch (error) {
            state.detailErrors.set(id, error.message);
            renderAlertDetails(id, detailErrorMarkup(id, error.message));
        } finally {
            state.detailRequests.delete(id);
        }
    })();
    state.detailRequests.set(id, request);
    return request;
}

async function analyzeAlert(id, button) {
    const errorElement = button.parentElement.querySelector(".analysis-action-error");
    const originalLabel = button.textContent;
    button.disabled = true;
    button.classList.add("is-loading");
    button.textContent = "Queueing…";
    errorElement.hidden = true;
    try {
        const response = await fetch(`/api/alerts/${id}/analyze`, {
            method: "POST",
            headers: { Accept: "application/json" },
        });
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.message || `Request failed with status ${response.status}`);
        }
        state.details.delete(id);
        state.detailErrors.delete(id);
        await loadAlerts({ preserveView: true });
    } catch (error) {
        button.disabled = false;
        button.classList.remove("is-loading");
        button.textContent = originalLabel;
        errorElement.textContent = error.message;
        errorElement.hidden = false;
    }
}

function toggleAlert(id) {
    const row = elements.body.querySelector(`[data-alert-id="${id}"]`);
    const details = elements.body.querySelector(`[data-details-id="${id}"]`);
    const trigger = row.querySelector(".alert-trigger");
    const expanded = !state.expanded.has(id);
    if (expanded) {
        state.expanded.add(id);
        details.querySelector("td").innerHTML = currentDetailMarkup(id);
        bindDetailActions(details);
        void details.offsetHeight;
    } else {
        state.expanded.delete(id);
    }
    row.classList.toggle("expanded", expanded);
    row.setAttribute("aria-selected", expanded);
    details.classList.toggle("open", expanded);
    trigger.setAttribute("aria-expanded", expanded);
    if (expanded && !state.details.has(id) && !state.detailErrors.has(id)) {
        loadAlertDetails(id);
    }
    if (!expanded) {
        window.setTimeout(() => {
            if (!state.expanded.has(id) && details.isConnected) {
                details.querySelector("td").innerHTML = "";
            }
        }, 380);
    }
}

function pageWindow() {
    if (state.totalPages <= 5) return Array.from({ length: state.totalPages }, (_, index) => index + 1);
    const pages = new Set([1, state.totalPages, state.page - 1, state.page, state.page + 1]);
    const valid = [...pages].filter((page) => page > 0 && page <= state.totalPages).sort((a, b) => a - b);
    const result = [];
    valid.forEach((page, index) => {
        if (index && page - valid[index - 1] > 1) result.push("…");
        result.push(page);
    });
    return result;
}

function renderPagination(alertCount) {
    elements.previous.disabled = state.page <= 1;
    elements.next.disabled = state.page >= state.totalPages;
    elements.pageNumbers.innerHTML = pageWindow().map((page) => {
        if (page === "…") return '<span class="page-ellipsis">…</span>';
        return `<button type="button" data-page="${page}" class="${page === state.page ? "current" : ""}" ${page === state.page ? 'aria-current="page"' : ""}>${page}</button>`;
    }).join("");
    elements.pageNumbers.querySelectorAll("button").forEach((button) => {
        button.addEventListener("click", () => goToPage(Number(button.dataset.page)));
    });
    const start = state.totalGroups ? (state.page - 1) * state.pageSize + 1 : 0;
    const end = start ? start + alertCount - 1 : 0;
    elements.range.textContent = `${start}–${end} of ${state.totalGroups} groups · ${state.total} signals`;
}

function renderSort() {
    document.querySelectorAll("th button[data-sort]").forEach((button) => {
        const active = button.dataset.sort === state.sort;
        button.classList.toggle("active", active);
        button.dataset.order = active ? state.order : "";
        button.setAttribute("aria-sort", active ? (state.order === "asc" ? "ascending" : "descending") : "none");
    });
}

function renderCounts(counts) {
    elements.total.textContent = counts.total.toLocaleString();
    elements.critical.textContent = counts.critical.toLocaleString();
    elements.warning.textContent = counts.warning.toLocaleString();
    elements.info.textContent = counts.info.toLocaleString();
}

function show(view) {
    elements.loading.hidden = view !== "loading";
    elements.error.hidden = view !== "error";
    elements.empty.hidden = view !== "empty";
    elements.tableView.hidden = view !== "table";
}

async function loadAlerts({ preserveView = false } = {}) {
    if (!preserveView) show("loading");
    elements.refresh.classList.add("is-loading");
    elements.refresh.disabled = true;
    try {
        const parameters = new URLSearchParams({
            page: state.page,
            page_size: state.pageSize,
            sort: state.sort,
            order: state.order,
        });
        const response = await fetch(`/api/alerts?${parameters}`, { headers: { Accept: "application/json" } });
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.message || `Request failed with status ${response.status}`);
        }
        const data = await response.json();
        state.total = data.counts.total;
        state.totalGroups = data.groups_total;
        state.totalPages = data.total_pages;
        state.page = data.page;
        renderCounts(data.counts);
        renderSort();
        if (!data.alerts.length && state.page > 1 && data.total_pages > 0) {
            state.page = data.total_pages;
            return loadAlerts();
        }
        if (!data.alerts.length) {
            state.expanded.clear();
            show("empty");
            return;
        }
        renderAlerts(data.alerts);
        renderPagination(data.alerts.length);
        show("table");
    } catch (error) {
        elements.errorMessage.textContent = error.message;
        show("error");
    } finally {
        elements.refresh.classList.remove("is-loading");
        elements.refresh.disabled = false;
    }
}

function goToPage(page) {
    if (page === state.page || page < 1 || page > state.totalPages) return;
    state.page = page;
    state.expanded.clear();
    loadAlerts();
    document.querySelector("#alerts-title").scrollIntoView({ behavior: "smooth", block: "start" });
}

document.querySelectorAll("th button[data-sort]").forEach((button) => {
    button.addEventListener("click", () => {
        const sort = button.dataset.sort;
        if (sort === state.sort) state.order = state.order === "desc" ? "asc" : "desc";
        else {
            state.sort = sort;
            state.order = sort === "summary" || sort === "status" ? "asc" : "desc";
        }
        state.page = 1;
        state.expanded.clear();
        loadAlerts();
    });
});

elements.previous.addEventListener("click", () => goToPage(state.page - 1));
elements.next.addEventListener("click", () => goToPage(state.page + 1));
elements.pageSize.addEventListener("change", () => {
    state.pageSize = Number(elements.pageSize.value);
    state.page = 1;
    state.expanded.clear();
    loadAlerts();
});
elements.refresh.addEventListener("click", () => loadAlerts({ preserveView: true }));
elements.retry.addEventListener("click", () => loadAlerts());

loadAlerts();
