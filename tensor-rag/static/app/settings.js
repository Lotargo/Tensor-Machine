import { state } from './state.js';
import {
    askConfirmation,
    escapeHtml,
    escapeHtmlAttr,
    fetchJson,
    hideReconnectOverlay,
    saveUiPref,
    showNotice,
    showReconnectOverlay,
    t,
    waitForReconnect,
} from './core.js';
import { fetchDocuments } from './library.js';

const PROVIDER_DEFAULTS = {
    openai: { baseUrl: 'https://api.openai.com/v1', model: 'gpt-5.5' },
    gemini: { baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', model: 'gemini-3.5-flash' },
    openrouter: { baseUrl: 'https://openrouter.ai/api/v1', model: '' },
    zen: { baseUrl: 'https://opencode.ai/zen/v1', model: 'big-pickle' },
    local: { baseUrl: 'http://localhost:11434/v1', model: '' },
    custom: { baseUrl: '', model: '' },
};

const PROVIDER_MODELS = {
    gemini: ['gemini-3.5-flash', 'gemini-3.1-flash-lite', 'gemini-3.1-pro-preview'],
    openai: ['gpt-5.5', 'gpt-5.4', 'gpt-5.4-mini'],
    zen: ['big-pickle', 'mimo-v2.5-free', 'deepseek-v4-flash-free', 'nemotron-3-ultra-free'],
};

let aiAutosaveTimer = null;
let aiAutosaveInFlight = false;
let aiAutosaveQueued = false;
let optimizeJobId = null;
let optimizeSource = null;

export function initSettings() {
    const modal = document.getElementById('settings-modal');
    document.getElementById('open-settings-btn').addEventListener('click', openSettingsModal);
    document.getElementById('settings-close-btn').addEventListener('click', closeSettingsModal);
    document.getElementById('settings-cancel-btn').addEventListener('click', closeSettingsModal);
    modal.addEventListener('click', (event) => {
        if (event.target === modal) closeSettingsModal();
    });

    document.getElementById('save-settings-btn').addEventListener('click', () => saveSettings({ scope: 'core', andRestart: false }));
    document.getElementById('save-restart-btn').addEventListener('click', () => saveSettings({ scope: 'core', andRestart: true }));
    document.getElementById('optimize-library-btn').addEventListener('click', optimizeLibrary);
    document.getElementById('cancel-optimize-btn').addEventListener('click', cancelOptimizeLibrary);
    document.getElementById('reindex-btn').addEventListener('click', reindexLibrary);
    document.getElementById('hard-reset-btn').addEventListener('click', hardReset);
}

export async function loadSettings() {
    const data = await fetchJson('/settings');
    state.settings = data.settings || [];
    state.collectionHealth = data.collection_health || null;
    renderSettingsPanels();
    document.dispatchEvent(new CustomEvent('tensor-rag:settings-loaded'));
}

async function openSettingsModal() {
    try {
        await loadSettings();
        document.getElementById('settings-modal').style.display = 'flex';
    } catch (error) {
        await showNotice(t('settings_failed_title'), t('settings_failed_message', { message: error.message }), 'danger');
    }
}

function closeSettingsModal() {
    document.getElementById('settings-modal').style.display = 'none';
}

export function renderSettingsPanels() {
    renderAiSettings();
    renderCoreSettings();
}

function renderAiSettings() {
    const form = document.getElementById('ai-settings-form');
    const aiSettings = state.settings.filter((setting) => setting.category === 'ai');
    form.innerHTML = '';
    aiSettings.forEach((setting) => form.appendChild(createSettingField(setting, 'ai')));
    setupModelSelector(form);
    bindProviderPresetBehavior(form);
    bindAiAutosave(form);
    setAiSaveState('idle');
}

function renderCoreSettings() {
    const form = document.getElementById('settings-form');
    const health = document.getElementById('settings-health');
    form.innerHTML = '';

    form.appendChild(createLocalUiCard());
    state.settings
        .filter((setting) => setting.category !== 'ai')
        .forEach((setting) => form.appendChild(createSettingField(setting, 'core')));

    const collectionHealth = state.collectionHealth;
    if (collectionHealth) {
        health.className = 'settings-health' + (collectionHealth.exists && !collectionHealth.dimension_matches ? ' is-warning' : '');
        health.innerHTML = collectionHealth.exists
            ? `<strong>${escapeHtml(t('qdrant_collection'))}</strong> ${escapeHtml(collectionHealth.collection_name)}<br><strong>${escapeHtml(t('configured_dim'))}</strong> ${collectionHealth.configured_dim}<br><strong>${escapeHtml(t('actual_dim'))}</strong> ${collectionHealth.actual_dim ?? t('unknown')}`
            : `<strong>${escapeHtml(t('collection_not_found'))}</strong>`;
    }
}

function createLocalUiCard() {
    const card = document.createElement('div');
    card.className = 'settings-local-card';
    card.innerHTML = `
        <div class="settings-local-card-header">
            <strong>${escapeHtml(t('local_ui_settings'))}</strong>
            <p>${escapeHtml(t('local_ui_desc'))}</p>
        </div>
        <div class="settings-local-grid">
            <div class="settings-field">
                <label for="ui-language-select">${escapeHtml(t('language_label'))}</label>
                <div class="settings-input-row">
                    <select id="ui-language-select">
                        <option value="ru">${escapeHtml(t('language_ru'))}</option>
                        <option value="en">${escapeHtml(t('language_en'))}</option>
                        <option value="zh">${escapeHtml(t('language_zh'))}</option>
                    </select>
                </div>
            </div>
            <div class="settings-field">
                <label for="ui-theme-select">${escapeHtml(t('theme_label'))}</label>
                <div class="settings-input-row">
                    <select id="ui-theme-select">
                        <option value="light">${escapeHtml(t('theme_light'))}</option>
                        <option value="dark">${escapeHtml(t('theme_dark'))}</option>
                        <option value="system">${escapeHtml(t('theme_system'))}</option>
                    </select>
                </div>
            </div>
        </div>
    `;

    const languageSelect = card.querySelector('#ui-language-select');
    const themeSelect = card.querySelector('#ui-theme-select');
    languageSelect.value = state.uiPrefs.language;
    themeSelect.value = state.uiPrefs.theme;
    languageSelect.addEventListener('change', () => {
        saveUiPref('language', languageSelect.value);
        document.dispatchEvent(new CustomEvent('tensor-rag:ui-pref-changed', { detail: { key: 'language' } }));
    });
    themeSelect.addEventListener('change', () => {
        saveUiPref('theme', themeSelect.value);
        document.dispatchEvent(new CustomEvent('tensor-rag:ui-pref-changed', { detail: { key: 'theme' } }));
    });

    return card;
}

function createSettingField(setting, section) {
    const wrapper = document.createElement('div');
    wrapper.className = 'settings-field';
    const inputId = `${section}-${setting.key}`;

    let control = '';
    if (setting.field_type === 'select') {
        control = `
            <select id="${inputId}" data-key="${escapeHtml(setting.key)}">
                ${setting.options.map((option) => `<option value="${escapeHtmlAttr(option.value)}" ${option.value === setting.value ? 'selected' : ''}>${escapeHtml(option.label)}</option>`).join('')}
            </select>
        `;
    } else if (setting.field_type === 'textarea') {
        control = `<textarea id="${inputId}" data-key="${escapeHtml(setting.key)}" rows="5" placeholder="${escapeHtmlAttr(setting.placeholder || '')}">${escapeHtml(setting.value || '')}</textarea>`;
    } else {
        const inputType = setting.field_type === 'password' ? 'password' : setting.field_type;
        control = `<input type="${inputType}" id="${inputId}" data-key="${escapeHtml(setting.key)}" value="${escapeHtmlAttr(setting.value || '')}" placeholder="${escapeHtmlAttr(setting.placeholder || '')}">`;
    }

    wrapper.innerHTML = `
        <label for="${inputId}">${escapeHtml(setting.label)}</label>
        <div class="settings-input-row${setting.field_type === 'password' ? ' has-secret' : ''}">
            ${control}
            ${setting.field_type === 'password' ? `<button type="button" class="secret-toggle-btn" data-secret-toggle="${inputId}"><i class="fa-solid fa-eye"></i></button>` : ''}
        </div>
        <small>${escapeHtml(setting.description)}</small>
    `;
    return wrapper;
}

const PROVIDER_APIKEYS_STORAGE = 'tensor-rag:provider-api-keys';

function loadProviderApiKeys() {
    try {
        return JSON.parse(localStorage.getItem(PROVIDER_APIKEYS_STORAGE)) || {};
    } catch {
        return {};
    }
}

function saveProviderApiKey(provider, apiKey) {
    const keys = loadProviderApiKeys();
    if (apiKey) {
        keys[provider] = apiKey;
    } else {
        delete keys[provider];
    }
    localStorage.setItem(PROVIDER_APIKEYS_STORAGE, JSON.stringify(keys));
}

function bindProviderPresetBehavior(form) {
    const providerSelect = form.querySelector('[data-key="AI_PROVIDER"]');
    const baseUrlInput = form.querySelector('[data-key="AI_BASE_URL"]');
    const modelInput = form.querySelector('[data-key="AI_MODEL"]');
    const apiKeyInput = form.querySelector('[data-key="AI_API_KEY"]');
    if (!providerSelect || !baseUrlInput || !modelInput) return;

    if (apiKeyInput) {
        const currentProvider = providerSelect.value;
        const storedKeys = loadProviderApiKeys();
        if (storedKeys[currentProvider]) {
            apiKeyInput.value = storedKeys[currentProvider];
        }
    }

    let previousProvider = providerSelect.value;

    providerSelect.addEventListener('change', () => {
        const defaults = PROVIDER_DEFAULTS[providerSelect.value] || PROVIDER_DEFAULTS.openai;
        if (!baseUrlInput.value || Object.values(PROVIDER_DEFAULTS).some((item) => item.baseUrl === baseUrlInput.value)) {
            baseUrlInput.value = defaults.baseUrl;
        }

        if (apiKeyInput && previousProvider !== providerSelect.value) {
            saveProviderApiKey(previousProvider, apiKeyInput.value);
            const storedKeys = loadProviderApiKeys();
            apiKeyInput.value = storedKeys[providerSelect.value] || '';
            apiKeyInput.dispatchEvent(new Event('input', { bubbles: true }));
        }

        previousProvider = providerSelect.value;
    });
}

function setupModelSelector(form) {
    const providerSelect = form.querySelector('[data-key="AI_PROVIDER"]');
    const modelInput = form.querySelector('[data-key="AI_MODEL"]');
    if (!providerSelect || !modelInput) return;

    const modelField = modelInput.closest('.settings-field');
    if (!modelField) return;

    function rebuildModelWidget() {
        const provider = providerSelect.value;
        const models = PROVIDER_MODELS[provider];
        const inputRow = modelField.querySelector('.settings-input-row');
        const currentModel = modelInput.value;

        const existingSelect = inputRow.querySelector('.model-select');
        const existingCustomWrap = inputRow.querySelector('.model-custom-wrap');
        if (existingSelect) existingSelect.remove();
        if (existingCustomWrap) existingCustomWrap.remove();

        if (models && models.length) {
            modelInput.style.display = 'none';

            const select = document.createElement('select');
            select.id = modelInput.id;
            select.className = 'model-select';

            models.forEach((m) => {
                const opt = document.createElement('option');
                opt.value = m;
                opt.textContent = m;
                select.appendChild(opt);
            });

            const customOpt = document.createElement('option');
            customOpt.value = '__custom__';
            customOpt.textContent = 'Custom...';
            select.appendChild(customOpt);

            const isCustom = !models.includes(currentModel);
            if (isCustom) {
                select.value = '__custom__';
            } else {
                select.value = currentModel;
            }

            inputRow.insertBefore(select, modelInput);

            let customWrap = inputRow.querySelector('.model-custom-wrap');
            if (!customWrap) {
                customWrap = document.createElement('div');
                customWrap.className = 'model-custom-wrap';
                customWrap.style.width = '100%';
                customWrap.style.display = 'none';
                const customInput = document.createElement('input');
                customInput.type = 'text';
                customInput.placeholder = 'Enter model identifier';
                customInput.style.width = '100%';
                customInput.style.marginTop = '6px';
                customWrap.appendChild(customInput);
                inputRow.insertBefore(customWrap, modelInput.nextSibling);
            }

            if (isCustom) {
                customWrap.style.display = 'block';
                customWrap.querySelector('input').value = currentModel;
            }

            select.addEventListener('change', () => {
                if (select.value === '__custom__') {
                    customWrap.style.display = 'block';
                    modelInput.value = customWrap.querySelector('input').value;
                } else {
                    customWrap.style.display = 'none';
                    modelInput.value = select.value;
                }
                modelInput.dispatchEvent(new Event('change', { bubbles: true }));
            });

            customWrap.querySelector('input').addEventListener('input', (e) => {
                modelInput.value = e.target.value;
                modelInput.dispatchEvent(new Event('input', { bubbles: true }));
            });

            if (select.value !== '__custom__') {
                modelInput.value = select.value;
            }
        } else {
            modelInput.style.display = '';
        }
    }

    rebuildModelWidget();
    providerSelect.addEventListener('change', () => {
        const defaults = PROVIDER_DEFAULTS[providerSelect.value] || PROVIDER_DEFAULTS.openai;
        const models = PROVIDER_MODELS[providerSelect.value];

        if (models && models.length) {
            const currentVal = modelInput.value;
            if (!currentVal || models.includes(currentVal) || Object.values(PROVIDER_DEFAULTS).some((d) => d.model === currentVal)) {
                modelInput.value = defaults.model || models[0];
            }
        } else {
            if (!modelInput.value || Object.values(PROVIDER_DEFAULTS).some((d) => d.model === modelInput.value)) {
                modelInput.value = defaults.model || '';
            }
        }

        rebuildModelWidget();
    });
}

function bindAiAutosave(form) {
    form.querySelectorAll('[data-key]').forEach((input) => {
        const eventName = input.tagName === 'SELECT' ? 'change' : 'input';
        input.addEventListener(eventName, () => scheduleAiAutosave(eventName === 'change' ? 80 : 500));
        if (eventName !== 'change') {
            input.addEventListener('change', () => scheduleAiAutosave(80));
        }
    });
}

function scheduleAiAutosave(delayMs) {
    setAiSaveState('saving');
    clearTimeout(aiAutosaveTimer);
    aiAutosaveTimer = setTimeout(() => {
        saveAiSettingsSilently();
    }, delayMs);
}

async function saveAiSettingsSilently() {
    if (aiAutosaveInFlight) {
        aiAutosaveQueued = true;
        return;
    }

    aiAutosaveInFlight = true;
    try {
        await fetchJson('/settings', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ values: collectValues('ai') }),
        });
        const data = await fetchJson('/settings');
        state.settings = data.settings || [];
        state.collectionHealth = data.collection_health || null;
        document.dispatchEvent(new CustomEvent('tensor-rag:settings-loaded'));
        setAiSaveState('saved');
    } catch (_error) {
        setAiSaveState('error');
    } finally {
        aiAutosaveInFlight = false;
        if (aiAutosaveQueued) {
            aiAutosaveQueued = false;
            scheduleAiAutosave(120);
        }
    }
}

function setAiSaveState(status) {
    const element = document.getElementById('ai-settings-save-state');
    if (!element) return;

    const messages = {
        ru: {
            idle: 'Изменения применяются автоматически.',
            saving: 'Сохраняем изменения...',
            saved: 'Настройки применены.',
            error: 'Не удалось применить изменения.',
        },
        en: {
            idle: 'Changes apply automatically.',
            saving: 'Saving changes...',
            saved: 'Settings applied.',
            error: 'Could not apply changes.',
        },
    };
    const lang = state.uiPrefs.language === 'ru' ? 'ru' : 'en';
    element.dataset.state = status;
    element.textContent = messages[lang][status] || messages[lang].idle;
}

function collectValues(scope) {
    const selectors = scope === 'ai'
        ? '#ai-settings-form [data-key]'
        : '#settings-form [data-key]';
    const values = {};
    document.querySelectorAll(selectors).forEach((input) => {
        values[input.dataset.key] = input.value;
    });
    return values;
}

async function saveSettings({ scope, andRestart }) {
    try {
        await fetchJson('/settings', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ values: collectValues(scope) }),
        });

        if (andRestart) {
            closeSettingsModal();
            showReconnectOverlay(t('restarting_title'), t('restarting_message'));
            await fetchJson('/settings/restart', { method: 'POST' });
            await waitForReconnect();
            await fetchDocuments();
            await loadSettings();
            return;
        }

        await loadSettings();
        if (scope !== 'ai') {
            await showNotice(t('settings_saved_title'), t('settings_saved_message'), 'success');
        }
    } catch (error) {
        hideReconnectOverlay();
        await showNotice(t('save_failed_title'), t('save_failed_message', { message: error.message }), 'danger');
    }
}

async function reindexLibrary() {
    const confirmed = await askConfirmation(t('reindex_title'), t('reindex_desc'), t('reindex_btn_text'), t('dialog_cancel'), 'warning');
    if (!confirmed) return;

    try {
        showReconnectOverlay(t('reindex_progress_title'), t('reindex_progress_message'));
        await fetchJson('/reindex', { method: 'POST' });
        await fetchDocuments();
        hideReconnectOverlay();
        closeSettingsModal();
        await showNotice(t('reindex_success_title'), t('reindex_success_message'), 'success');
    } catch (error) {
        hideReconnectOverlay();
        await showNotice(t('reindex_failed_title'), t('reindex_failed_message', { message: error.message }), 'danger');
    }
}

async function optimizeLibrary() {
    const confirmed = await askConfirmation(
        'Оптимизация библиотеки',
        'Проверить документы, пересчитать сохранённую семантическую раскладку и обновить статистику PostgreSQL?',
        'Оптимизировать',
        t('dialog_cancel'),
        'warning'
    );
    if (!confirmed) return;

    try {
        setOptimizeUiState('running');
        const response = await fetchJson('/maintenance/optimize', { method: 'POST' });
        optimizeJobId = response.job_id;
        subscribeOptimizeProgress(optimizeJobId);
    } catch (error) {
        setOptimizeUiState('idle');
        await showNotice('Оптимизация не запущена', error.message, 'danger');
    }
}

async function cancelOptimizeLibrary() {
    if (!optimizeJobId) return;
    try {
        document.getElementById('cancel-optimize-btn').disabled = true;
        await fetchJson(`/maintenance/cancel/${encodeURIComponent(optimizeJobId)}`, { method: 'POST' });
    } catch (error) {
        await showNotice('Не удалось отменить', error.message, 'danger');
    }
}

function subscribeOptimizeProgress(jobId) {
    if (optimizeSource) optimizeSource.close();
    optimizeSource = new EventSource(`/maintenance/progress/${encodeURIComponent(jobId)}`);
    optimizeSource.addEventListener('maintenance-progress', async (event) => {
        try {
            const progress = JSON.parse(event.data);
            renderOptimizeProgress(progress);
            if (progress.status === 'completed' || progress.step === 'cancelled' || progress.status === 'failed') {
                optimizeSource?.close();
                optimizeSource = null;
                optimizeJobId = null;
                setOptimizeUiState('idle');
                await fetchDocuments();
                await loadSettings();
                if (progress.status === 'completed') {
                    await showNotice('Оптимизация завершена', progress.message || 'Библиотека обновлена.', 'success');
                }
            }
        } catch (error) {
            console.error('Failed to parse maintenance progress:', error);
        }
    });
    optimizeSource.onerror = () => {
        console.warn('Maintenance progress stream disconnected.');
    };
}

function setOptimizeUiState(stateName) {
    const startBtn = document.getElementById('optimize-library-btn');
    const cancelBtn = document.getElementById('cancel-optimize-btn');
    const progress = document.getElementById('optimize-progress');
    if (!startBtn || !cancelBtn || !progress) return;

    if (stateName === 'running') {
        startBtn.disabled = true;
        cancelBtn.disabled = false;
        cancelBtn.style.display = 'inline-flex';
        progress.style.display = 'flex';
        renderOptimizeProgress({
            step: 'prepare',
            status: 'active',
            message: 'Подготовка оптимизации...',
            current: 0,
            total: 0,
            checked: 0,
            reclustered: 0,
            updated_rows: 0,
            warnings: [],
        });
    } else {
        startBtn.disabled = false;
        cancelBtn.disabled = false;
        cancelBtn.style.display = 'none';
    }
}

function renderOptimizeProgress(progress) {
    const panel = document.getElementById('optimize-progress');
    const title = document.getElementById('optimize-progress-title');
    const count = document.getElementById('optimize-progress-count');
    const fill = document.getElementById('optimize-progress-fill');
    const meta = document.getElementById('optimize-progress-meta');
    const warnings = document.getElementById('optimize-progress-warnings');
    if (!panel || !title || !count || !fill || !meta || !warnings) return;

    panel.style.display = 'flex';
    const current = Number(progress.current || 0);
    const total = Number(progress.total || 0);
    const ratio = total > 0 ? Math.min(100, Math.round((current / total) * 100)) : 0;
    title.textContent = progress.message || 'Оптимизация библиотеки';
    count.textContent = total > 0 ? `${current}/${total}` : '';
    fill.style.width = `${ratio}%`;
    meta.textContent = `Проверено: ${progress.checked || 0}. Пересчитано документов: ${progress.reclustered || 0}. Обновлено сегментов: ${progress.updated_rows || 0}.`;

    const items = Array.isArray(progress.warnings) ? progress.warnings : [];
    if (items.length) {
        warnings.style.display = 'block';
        warnings.innerHTML = items.slice(-6).map((item) => `<div>${escapeHtml(item)}</div>`).join('');
    } else {
        warnings.style.display = 'none';
        warnings.innerHTML = '';
    }
}

async function hardReset() {
    const confirmed = await askConfirmation(
        t('hard_reset_title'),
        t('hard_reset_message'),
        t('hard_reset_confirm'),
        t('dialog_cancel'),
        'danger'
    );
    if (!confirmed) return;

    try {
        showReconnectOverlay(t('hard_reset_progress_title'), t('hard_reset_progress_message'));
        await fetchJson('/hard-reset', { method: 'POST' });
        await fetchDocuments();
        hideReconnectOverlay();
        closeSettingsModal();
        await showNotice(t('hard_reset_success_title'), t('hard_reset_success_message'), 'success');
    } catch (error) {
        hideReconnectOverlay();
        await showNotice(t('hard_reset_failed_title'), t('hard_reset_failed_message', { message: error.message }), 'danger');
    }
}
