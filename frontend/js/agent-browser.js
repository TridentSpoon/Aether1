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
        COLLECTOR: 'collector',
        REVIEWER: 'reviewer',
        PLANNER: 'planner'
    };

    /**
     * Fetches agents from the backend
     */
    async function fetchAgents() {
        try {
            if (typeof window.__TAURI_INTERNALS__ !== 'undefined') {
                // Tauri IPC
                const { invoke } = await import('https://cdn.jsdelivr.net/npm/@tauri-apps/api@next/index.js');
                const result = await invoke('get_agents_rust');
                agents = result;
            } else {
                // HTTP fallback
                const response = await fetch(`${API_BASE || ''}/api/agents`);
                if (!response.ok) throw new Error('Failed to fetch agents');
                agents = await response.json();
            }
            applyFilters();
            return agents;
        } catch (err) {
            console.error('[AgentBrowser] Failed to fetch agents:', err);
            // Use built-in agents as fallback
            agents = getBuiltInAgents();
            applyFilters();
            return agents;
        }
    }

    /**
     * Built-in agents for offline/fallback mode
     */
    function getBuiltInAgents() {
        return [
            {
                id: 'claude-opus-reviewer',
                name: 'Claude-Opus Code Reviewer',
                description: 'Deep code review specialist with architectural insights.',
                agent_type: 'reviewer',
                model: 'claude-opus',
                capabilities: ['code-review', 'testing'],
                installed: false,
                selected: false
            },
            {
                id: 'gpt4o-architect',
                name: 'GPT-4o Architect',
                description: 'System design and architecture specialist.',
                agent_type: 'planner',
                model: 'gpt-4o',
                capabilities: ['architecture', 'documentation'],
                installed: false,
                selected: false
            },
            {
                id: 'claude-sonnet-qa',
                name: 'Claude-Sonnet Quality Analyst',
                description: 'Quality assurance and performance specialist.',
                agent_type: 'reviewer',
                model: 'claude-sonnet',
                capabilities: ['testing', 'performance'],
                installed: false,
                selected: false
            },
            {
                id: 'haiku-context-optimizer',
                name: 'Haiku Context Optimizer',
                description: 'Context and knowledge management.',
                agent_type: 'collector',
                model: 'claude-haiku',
                capabilities: ['summarization', 'context'],
                installed: false,
                selected: false
            }
        ];
    }

    /**
     * Persists selection state to backend
     */
    async function saveSelection() {
        const selectedIds = Array.from(selectedAgents);
        try {
            if (typeof window.__TAURI_INTERNALS__ !== 'undefined') {
                const { invoke } = await import('https://cdn.jsdelivr.net/npm/@tauri-apps/api@next/index.js');
                await invoke('set_agent_selection_rust', { selectedIds });
            } else {
                await fetch(`${API_BASE || ''}/api/agents/selection`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ selected_ids: selectedIds })
                });
            }
        } catch (err) {
            console.error('[AgentBrowser] Failed to save selection:', err);
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

        saveSelection();
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
                a.description.toLowerCase().includes(query)
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
        const installedBadge = agent.installed ? '<span class="agent-installed-badge">Installed</span>' : '';

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

    /**
     * Renders the filter sidebar
     */
    function renderFilters(container) {
        const capabilities = getAllCapabilities();

        const typeOptions = [
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

        // Attach card click handlers
        container.querySelectorAll('.agent-card').forEach(card => {
            card.addEventListener('click', (e) => {
                if (e.target.closest('.agent-select-btn')) {
                    const agentId = card.getAttribute('data-agent-id');
                    toggleAgentSelection(agentId);
                    renderAgentGrid(container);
                }
            });

            card.addEventListener('keydown', (e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    const agentId = card.getAttribute('data-agent-id');
                    toggleAgentSelection(agentId);
                    renderAgentGrid(container);
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
                <h2 class="agent-hero-title">Agent Marketplace</h2>
                <p class="agent-hero-subtitle">Discover and install specialized AI agents</p>
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

        // Load agents first
        await fetchAgents();

        const html = `
            <div class="agent-browser">
                <div id="agent-hero-section" class="agent-hero-section"></div>
                <div id="agent-search-section" class="agent-search-section"></div>
                <div class="agent-browser-main">
                    <aside id="agent-filters-section" class="agent-filters-section"></aside>
                    <main id="agent-grid" class="agent-grid"></main>
                </div>
            </div>
        `;

        container.innerHTML = html;

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
