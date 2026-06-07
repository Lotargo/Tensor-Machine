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
    openai: { baseUrl: 'https://api.openai.com/v1', model: 'gpt-4.1-mini' },
    gemini: { baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', model: 'gemini-3.5-flash' },
    openrouter: { baseUrl: 'https://openrouter.ai/api/v1', model: 'openai/gpt-4.1-mini' },
    local: { baseUrl: 'http://localhost:11434/v1', model: 'llama3.1' },
    custom: { baseUrl: '', model: '' },
};

let aiAutosaveTimer = null;
let aiAutosaveInFlight = false;
let aiAutosaveQueued = false;

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

function bindProviderPresetBehavior(form) {
    const providerSelect = form.querySelector('[data-key="AI_PROVIDER"]');
    const baseUrlInput = form.querySelector('[data-key="AI_BASE_URL"]');
    const modelInput = form.querySelector('[data-key="AI_MODEL"]');
    if (!providerSelect || !baseUrlInput || !modelInput) return;

    providerSelect.addEventListener('change', () => {
        const defaults = PROVIDER_DEFAULTS[providerSelect.value] || PROVIDER_DEFAULTS.openai;
        if (!baseUrlInput.value || Object.values(PROVIDER_DEFAULTS).some((item) => item.baseUrl === baseUrlInput.value)) {
            baseUrlInput.value = defaults.baseUrl;
        }
        if (!modelInput.value || Object.values(PROVIDER_DEFAULTS).some((item) => item.model === modelInput.value)) {
            modelInput.value = defaults.model;
        }
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
        await loadSettings();
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
