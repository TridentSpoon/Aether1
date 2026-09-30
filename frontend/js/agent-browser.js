/**
 * Agent Browser: Discovery and Management UI for AI Agents
 *
 * Provides a grid-based interface for discovering, filtering, and selecting AI agents.
 * Uses Tauri IPC when available, falls back to HTTP API otherwise.
 *
 * Main entry point: renderAgentBrowser(containerId)
 */

const AgentBrowser = (() => {
    // State management
    let agents = [];
    let selectedAgents = new Set();
    let filteredAgents = [];
    let currentFilters = {
        type: null,
        capability: null,
        search: ''
    };

    // Agent type enum
    const AgentType = {
        BUILDER: 'builder',
        COLLECTOR: 'collector',
        REVIEWER: 'reviewer',
        PLANNER: 'planner'
    };

    /**
     * Fetches agents from the backend
     */
    async function fetchAgents() {
        if (typeof window.__TAURI_INTERNALS__ !== 'undefined') {
            agents = await window.__TAURI__.core.invoke('get_agents_rust');
        } else {
            const response = await apiFetch('/api/agents');
            if (!response.ok) throw new Error(`Could not load profiles (${response.status})`);
            agents = await response.json();
        }
        selectedAgents = new Set(agents.filter(agent => agent.selected).map(agent => agent.id));
        applyFilters();
        return agents;
    }

    /**
     * Persists selection state to backend
     */
    async function saveSelection() {
        const selectedIds = Array.from(selectedAgents);
        try {
            if (typeof window.__TAURI_INTERNALS__ !== 'undefined') {
                await window.__TAURI__.core.invoke('set_agent_selection_rust', { selectedIds });
            } else {
                const response = await apiFetch('/api/agents/selection', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ selected_ids: selectedIds })
                });
                if (!response.ok) throw new Error((await response.text()) || `Save failed (${response.status})`);
            }
        } catch (err) {
            console.error('[AgentBrowser] Failed to save selection:', err);
            throw err;
        }
    }

    /**
     * Toggles agent selection
     */
    function toggleAgentSelection(agentId) {
        if (selectedAgents.has(agentId)) {
            selectedAgents.delete(agentId);
        } else {
            selectedAgents.add(agentId);
        }

        // Update agent objects
        agents.forEach(a => {
            a.selected = selectedAgents.has(a.id);
        });

        saveSelection().catch(err => showStatus(`Could not save profile selection: ${err.message || err}`, true));
    }

    /**
     * Applies current filters to agents
     */
    function applyFilters() {
        let result = agents;

        // Type filter
        if (currentFilters.type) {
            result = result.filter(a => a.agent_type === currentFilters.type);
        }

        // Capability filter
        if (currentFilters.capability) {
            result = result.filter(a => a.capabilities.includes(currentFilters.capability));
        }

        // Search filter
        if (currentFilters.search.trim()) {
            const query = currentFilters.search.toLowerCase();
            result = result.filter(a =>
                a.name.toLowerCase().includes(query) ||
                a.description.toLowerCase().includes(query) ||
                a.capabilities.some(cap => cap.toLowerCase().includes(query))
            );
        }

        filteredAgents = result;
        return result;
    }

    /**
     * Gets all unique capabilities
     */
    function getAllCapabilities() {
        const caps = new Set();
        agents.forEach(a => {
            a.capabilities.forEach(c => caps.add(c));
        });
        return Array.from(caps).sort();
    }

    /**
     * Renders a single agent card
     */
    function renderAgentCard(agent) {
        const capabilityTags = agent.capabilities
            .map(cap => `<span class="agent-cap-tag">${escapeHtml(cap)}</span>`)
            .join('');

        const selectedClass = agent.selected ? 'selected' : '';
        const installedBadge = '<span class="agent-installed-badge">AETHER CODE profile</span>';

        return `
            <div class="agent-card ${selectedClass}" data-agent-id="${escapeHtml(agent.id)}" role="button" tabindex="0">
                <div class="agent-card-header">
                    <h3 class="agent-name">${escapeHtml(agent.name)}</h3>
                    ${installedBadge}
                </div>
                <p class="agent-description">${escapeHtml(agent.description)}</p>
                <div class="agent-meta">
                    <span class="agent-model">${escapeHtml(agent.model)}</span>
                    <span class="agent-type agent-type-${agent.agent_type}">${escapeHtml(capitalizeFirst(agent.agent_type))}</span>
                </div>
                <div class="agent-capabilities">
                    ${capabilityTags}
                </div>
                <div class="agent-card-footer">
                    <button class="agent-select-btn ${agent.selected ? 'selected' : ''}">
                        ${agent.selected ? '✓ Selected' : '+ Select'}
                    </button>
                </div>
            </div>
        `;
    }

    function showStatus(message, isError = false) {
        const status = document.getElementById('agent-run-status');
        if (!status) return;
        status.textContent = message;
        status.className = `text-xs ${isError ? 'text-red-300' : 'text-cyan-200'}`;
    }

    async function runSelectedAgents() {
        const task = document.getElementById('agent-task-input')?.value.trim();
        const status = document.getElementById('agent-run-status');
        const results = document.getElementById('agent-run-results');
        const button = document.getElementById('agent-run-button');
        if (!task) return showStatus('Describe the repository or change you want analyzed.', true);
        if (!selectedAgents.size) return showStatus('Select at least one profile first.', true);
        button.disabled = true;
        button.textContent = 'Analyzing…';
        status.textContent = 'Running selected profiles one at a time with the active AETHER CODE model…';
        status.className = 'text-xs text-cyan-200 animate-pulse';
        results.replaceChildren();
        try {
            let response;
            if (typeof window.__TAURI_INTERNALS__ !== 'undefined') {
                response = await window.__TAURI__.core.invoke('code_agents_ask_rust', {
                    task,
                    selectedIds: Array.from(selectedAgents),
                });
            } else {
                const resp = await apiFetch('/api/agents/run', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ task, selected_ids: Array.from(selectedAgents) }),
                });
                if (!resp.ok) throw new Error((await resp.text()) || `Analysis failed (${resp.status})`);
                response = await resp.json();
            }
            for (const result of response) {
                const article = document.createElement('article');
                article.className = 'border border-cyan-500/20 rounded p-3 bg-slate-950/60';
                const title = document.createElement('h3');
                title.className = 'font-mono text-cyan-200 text-sm mb-2';
                title.textContent = result.agent_name;
                const body = document.createElement('pre');
                body.className = 'text-sm text-slate-200 whitespace-pre-wrap font-sans';
                body.textContent = result.text;
                article.append(title, body);
                results.appendChild(article);
            }
            showStatus(`${response.length} profile${response.length === 1 ? '' : 's'} completed. Results are also in AETHER CODE history.`);
        } catch (err) {
            showStatus(err.message || String(err), true);
        } finally {
            button.disabled = false;
            button.textContent = 'Run selected profiles';
        }
    }

    /**
     * Renders the filter sidebar
     */
    function renderFilters(container) {
        const capabilities = getAllCapabilities();

        const typeOptions = [
            { value: 'builder', label: 'Builder' },
            { value: 'collector', label: 'Collector' },
            { value: 'reviewer', label: 'Reviewer' },
            { value: 'planner', label: 'Planner' }
        ];

        const typeFiltersHtml = typeOptions
            .map(opt => `
                <label class="filter-checkbox">
                    <input type="checkbox" value="${opt.value}" class="filter-type-input"
                        ${currentFilters.type === opt.value ? 'checked' : ''}>
                    <span>${opt.label}</span>
                </label>
            `)
            .join('');

        const capFiltersHtml = capabilities
            .map(cap => `
                <label class="filter-checkbox">
                    <input type="checkbox" value="${cap}" class="filter-capability-input"
                        ${currentFilters.capability === cap ? 'checked' : ''}>
                    <span>${escapeHtml(cap)}</span>
                </label>
            `)
            .join('');

        const html = `
            <div class="agent-filters">
                <div class="filter-section">
                    <h4 class="filter-heading">Agent Type</h4>
                    <div class="filter-options">
                        <label class="filter-checkbox">
                            <input type="radio" name="type-filter" value="" class="filter-type-radio"
                                ${!currentFilters.type ? 'checked' : ''}>
                            <span>All Types</span>
                        </label>
                        ${typeFiltersHtml}
                    </div>
                </div>

                <div class="filter-section">
                    <h4 class="filter-heading">Capabilities</h4>
                    <div class="filter-options">
                        <label class="filter-checkbox">
                            <input type="checkbox" value="" class="filter-capability-clear"
                                ${!currentFilters.capability ? 'checked' : ''}>
                            <span>All Capabilities</span>
                        </label>
                        ${capFiltersHtml}
                    </div>
                </div>
            </div>
        `;

        container.innerHTML = html;

        // Attach event listeners
        container.querySelectorAll('.filter-type-radio').forEach(radio => {
            radio.addEventListener('change', (e) => {
                currentFilters.type = e.target.value || null;
                applyFilters();
                renderAgentGrid(document.getElementById('agent-grid'));
            });
        });

        container.querySelectorAll('.filter-capability-input').forEach(input => {
            input.addEventListener('change', (e) => {
                currentFilters.capability = e.target.value;
                applyFilters();
                renderAgentGrid(document.getElementById('agent-grid'));
            });
        });

        container.querySelector('.filter-capability-clear').addEventListener('change', (e) => {
            if (e.target.checked) {
                currentFilters.capability = null;
                applyFilters();
                renderAgentGrid(document.getElementById('agent-grid'));
            }
        });
    }

    /**
     * Renders the agent grid
     */
    function renderAgentGrid(container) {
        const html = filteredAgents
            .map(agent => renderAgentCard(agent))
            .join('');

        container.innerHTML = html || '<div class="no-agents">No agents match your filters</div>';

        // A profile is selected only through its explicit button.
        container.querySelectorAll('.agent-card').forEach(card => {
            card.querySelector('.agent-select-btn')?.addEventListener('click', () => {
                toggleAgentSelection(card.dataset.agentId);
                renderAgentGrid(container);
                renderHeroSection(document.getElementById('agent-hero-section'));
            });
            card.addEventListener('keydown', (e) => {
                if ((e.key === 'Enter' || e.key === ' ') && e.target === card) {
                    e.preventDefault();
                    const agentId = card.getAttribute('data-agent-id');
                    toggleAgentSelection(agentId);
                    renderAgentGrid(container);
                    renderHeroSection(document.getElementById('agent-hero-section'));
                }
            });
        });
    }

    /**
     * Renders the search bar
     */
    function renderSearchBar(container) {
        const html = `
            <div class="agent-search">
                <input type="text" id="agent-search-input" class="agent-search-input"
                    placeholder="Search agents by name or capability..."
                    value="${escapeHtml(currentFilters.search)}">
                <span class="agent-search-icon">🔍</span>
            </div>
        `;

        container.innerHTML = html;

        const input = container.querySelector('#agent-search-input');
        input.addEventListener('input', (e) => {
            currentFilters.search = e.target.value;
            applyFilters();
            renderAgentGrid(document.getElementById('agent-grid'));
        });
    }

    /**
     * Renders the hero section with agent counts
     */
    function renderHeroSection(container) {
        const selectedCount = selectedAgents.size;
        const totalCount = agents.length;

        const html = `
            <div class="agent-hero">
                <h2 class="agent-hero-title">AETHER CODE Profiles</h2>
                <p class="agent-hero-subtitle">Choose focused repository analyses using the coding model configured in The Brain.</p>
                <div class="agent-stats">
                    <div class="stat">
                        <span class="stat-label">Available</span>
                        <span class="stat-value">${totalCount}</span>
                    </div>
                    <div class="stat">
                        <span class="stat-label">Selected</span>
                        <span class="stat-value">${selectedCount}</span>
                    </div>
                </div>
            </div>
        `;

        container.innerHTML = html;
    }

    /**
     * Main render function for the entire agent browser
     */
    async function renderAgentBrowser(containerId) {
        const container = document.getElementById(containerId);
        if (!container) {
            console.error('[AgentBrowser] Container not found:', containerId);
            return;
        }

        const html = `
            <div class="agent-browser">
                <section class="border border-cyan-500/30 rounded-lg p-4 mb-4 bg-slate-950/50">
                    <p class="text-sm text-slate-300 mb-2">Profiles run on AETHER CODE's active model. Code Builder edits the selected workspace when Edit is enabled; checks run only when Run is enabled. Reviewer, Planner, Test Analyst, and Repository Guide focus on analysis. These profiles share AETHER CODE's workspace and permissions; they are not separate Claude or GPT accounts.</p>
                    <label class="block text-xs font-mono text-cyan-300 mb-1" for="agent-task-input">REPOSITORY OR CHANGE TO ANALYZE</label>
                    <textarea id="agent-task-input" rows="3" maxlength="16000" placeholder="e.g. Review the current branch diff for regressions, or explain how GitHub PR #42 affects the local code." class="w-full bg-slate-900 border border-cyan-500/40 rounded p-2 text-sm text-cyan-100"></textarea>
                    <div class="flex items-center gap-3 mt-2"><button id="agent-run-button" type="button" class="cyber-btn text-xs">Run selected profiles</button><span id="agent-run-status" class="text-xs text-slate-400" role="status"></span></div>
                    <div id="agent-run-results" class="space-y-3 mt-3"></div>
                </section>
                <div id="agent-hero-section" class="agent-hero-section"></div>
                <div id="agent-search-section" class="agent-search-section"></div>
                <div class="agent-browser-main">
                    <aside id="agent-filters-section" class="agent-filters-section"></aside>
                    <main id="agent-grid" class="agent-grid"></main>
                </div>
            </div>
        `;

        container.innerHTML = html;

        document.getElementById('agent-run-button').addEventListener('click', runSelectedAgents);
        try {
            await fetchAgents();
        } catch (err) {
            showStatus(err.message || 'Could not load Agent Browser profiles.', true);
            return;
        }

        // Render all sections
        renderHeroSection(document.getElementById('agent-hero-section'));
        renderSearchBar(document.getElementById('agent-search-section'));
        renderFilters(document.getElementById('agent-filters-section'));
        renderAgentGrid(document.getElementById('agent-grid'));
    }

    /**
     * Utility: escape HTML special characters
     */
    function escapeHtml(text) {
        const map = {
            '&': '&amp;',
            '<': '&lt;',
            '>': '&gt;',
            '"': '&quot;',
            "'": '&#039;'
        };
        return text.replace(/[&<>"']/g, m => map[m]);
    }

    /**
     * Utility: capitalize first letter
     */
    function capitalizeFirst(str) {
        return str.charAt(0).toUpperCase() + str.slice(1);
    }

    // Public API
    return {
        render: renderAgentBrowser,
        getSelectedAgents: () => Array.from(selectedAgents),
        getAllAgents: () => agents,
        getFilters: () => ({ ...currentFilters })
    };
})();
