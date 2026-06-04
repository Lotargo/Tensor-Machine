import { bindGlobalInfrastructure, applyTheme, applyTranslations, loadUiPrefs, saveUiPref } from './core.js';
import { initLibrary, fetchDocuments, renderDocumentList, selectDocument } from './library.js';
import { initSettings, loadSettings, renderSettingsPanels } from './settings.js';
import { initChat } from './chat.js';
import { state } from './state.js';

document.addEventListener('DOMContentLoaded', () => {
    initApp();
});

async function initApp() {
    loadUiPrefs();
    applyTheme();
    bindGlobalInfrastructure();
    initSidebarToggles();
    initLibrary();
    initSettings();
    initChat();
    applyTranslations();
    syncSidebarState();

    await Promise.allSettled([
        fetchDocuments(),
        loadSettings(),
    ]);

    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
        if (state.uiPrefs.theme === 'system') {
            applyTheme();
        }
    });

    document.addEventListener('tensor-rag:ui-pref-changed', (event) => {
        if (event.detail.key === 'theme') {
            applyTheme();
        } else if (event.detail.key === 'language') {
            applyTranslations();
            renderSettingsPanels();
            renderDocumentList(state.documentList);
            if (state.selectedDocUuid) selectDocument(state.selectedDocUuid);
        }
    });

    document.addEventListener('tensor-rag:translations-applied', () => {
        if (state.selectedDocUuid) selectDocument(state.selectedDocUuid);
    });
}

function initSidebarToggles() {
    document.getElementById('toggle-library-btn').addEventListener('click', () => toggleSidebar('leftSidebarCollapsed'));
    document.getElementById('toggle-ai-btn').addEventListener('click', () => toggleSidebar('rightSidebarCollapsed'));
    document.getElementById('collapse-left-sidebar-btn').addEventListener('click', () => toggleSidebar('leftSidebarCollapsed'));
    document.getElementById('collapse-right-sidebar-btn').addEventListener('click', () => toggleSidebar('rightSidebarCollapsed'));
}

function toggleSidebar(key) {
    saveUiPref(key, !state.uiPrefs[key]);
    syncSidebarState();
}

function syncSidebarState() {
    const app = document.querySelector('.app-container');
    app.dataset.leftCollapsed = String(state.uiPrefs.leftSidebarCollapsed);
    app.dataset.rightCollapsed = String(state.uiPrefs.rightSidebarCollapsed);
}
