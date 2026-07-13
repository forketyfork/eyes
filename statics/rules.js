const elements = {
    body: document.querySelector("#rules-body"),
    table: document.querySelector("#rules-table"),
    loading: document.querySelector("#rules-loading"),
    error: document.querySelector("#rules-error"),
    errorMessage: document.querySelector("#rules-error-message"),
    empty: document.querySelector("#rules-empty"),
    count: document.querySelector("#rule-count"),
    refresh: document.querySelector("#refresh-rules"),
    retry: document.querySelector("#retry-rules"),
};

function escapeHtml(value) {
    return String(value ?? "")
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;")
        .replaceAll("'", "&#039;");
}

function formatTime(value) {
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return value || "Unknown";
    return new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
    }).format(date);
}

function selectorMarkup(rule) {
    const selectors = [
        ["Process", rule.process],
        ["Subsystem", rule.subsystem],
        ["Source", rule.trigger_source],
        ["Trigger", rule.triggered_by],
    ].filter(([, value]) => value);
    return selectors.map(([label, value]) => `<span class="rule-selector"><small>${label}</small>${escapeHtml(value)}</span>`).join("");
}

function ruleMarkup(rule, index) {
    return `<tr class="rule-row" style="--row-index:${index}">
        <td><span class="rule-order">${rule.id}</span></td>
        <td><span class="rule-target">Signal ${String(rule.target_alert_id).padStart(4, "0")}</span></td>
        <td><div class="rule-selectors">${selectorMarkup(rule)}</div></td>
        <td><code class="rule-regex">${escapeHtml(rule.message_regex)}</code></td>
        <td class="rule-created">${escapeHtml(formatTime(rule.created_at))}</td>
    </tr>`;
}

function show(view) {
    elements.loading.hidden = view !== "loading";
    elements.error.hidden = view !== "error";
    elements.empty.hidden = view !== "empty";
    elements.table.hidden = view !== "table";
}

async function loadRules({ preserveView = false } = {}) {
    if (!preserveView) show("loading");
    elements.refresh.classList.add("is-loading");
    elements.refresh.disabled = true;
    try {
        const response = await fetch("/api/auto-group-rules", { headers: { Accept: "application/json" } });
        if (!response.ok) {
            const error = await response.json().catch(() => ({}));
            throw new Error(error.message || `Request failed with status ${response.status}`);
        }
        const rules = await response.json();
        elements.count.textContent = rules.length ? String(rules.length) : "";
        if (!rules.length) {
            show("empty");
            return;
        }
        elements.body.innerHTML = rules.map(ruleMarkup).join("");
        show("table");
    } catch (error) {
        elements.errorMessage.textContent = error.message;
        show("error");
    } finally {
        elements.refresh.classList.remove("is-loading");
        elements.refresh.disabled = false;
    }
}

elements.refresh.addEventListener("click", () => loadRules({ preserveView: true }));
elements.retry.addEventListener("click", () => loadRules());

loadRules();
