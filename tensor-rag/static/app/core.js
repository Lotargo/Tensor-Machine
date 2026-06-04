import { state, storageKeys } from './state.js';

const TEXT = {
    ru: {
        page_title: 'Tensor Machine: Document Portal',
        library: 'Библиотека',
        search_placeholder: 'Поиск в библиотеке...',
        clear_library: 'Очистить библиотеку',
        header_title: 'Панель управления документами',
        header_subtitle: 'Загружайте и просматривайте проиндексированные документы RAG',
        system_settings: 'Системные настройки',
        upload_document: 'Загрузить документ',
        toggle_library: 'Библиотека',
        toggle_ai: 'AI',
        dropzone_text: 'Перетащите файл сюда или нажмите кнопку <strong>Загрузить</strong>',
        dropzone_subtext: 'Поддерживаются изображения, PDF, TXT, Markdown и DOCX',
        processing_file: 'Обработка файла:',
        step_upload: 'Загрузка файла',
        step_ocr: 'Mistral OCR',
        step_chunk: 'Токенизация и чанкование',
        step_embedding: 'Генерация эмбеддингов E5',
        step_db: 'Индексация в Qdrant и PostgreSQL',
        selected_document: 'Выбранный документ',
        metadata: 'Метаданные',
        print: 'Печать',
        download: 'Скачать',
        delete: 'Удалить',
        visual_layout: 'Визуальный макет документа',
        rendered_content: 'Содержимое документа (Markdown / LaTeX / Mermaid)',
        empty_title: 'Документ не выбран',
        empty_text: 'Выберите документ слева или загрузите новый файл для просмотра',
        chat_title: 'Чат с RAG',
        chat_subtitle: 'Модель получает контекст через MCP retrieval.',
        chat_placeholder: 'Спросите что-нибудь по библиотеке документов...',
        chat_send: 'Отправить',
        chat_scope_label: 'Ограничивать ответы выбранным документом',
        chat_rag_label: 'Использовать RAG-поиск по библиотеке',
        chat_scope_none: 'Документ не выбран',
        chat_scope_active: 'Контекст будет ограничен выбранным документом',
        chat_scope_general: 'RAG-поиск отключён. Чат работает без поиска по документам',
        chat_fullscreen: 'Полноэкранный чат',
        chat_modal_meta: 'Чат с RAG-контекстом через MCP',
        clear_chat: 'Очистить чат',
        chat_empty: 'Задайте вопрос, и ассистент ответит на основе найденных фрагментов из библиотеки.',
        chat_thinking_search: 'Ищу релевантные фрагменты',
        chat_thinking_reason: 'Размышляю над контекстом',
        ai_settings_title: 'AI настройки',
        ai_settings_subtitle: 'OpenAI-compatible провайдеры, включая Gemini.',
        save_ai_settings: 'Сохранить AI настройки',
        settings_title: 'Настройки движка',
        settings_subtitle: 'Изменения сохраняются в .env, а для системных параметров требуется перезапуск.',
        settings_close: 'Закрыть',
        settings_save: 'Сохранить',
        settings_save_restart: 'Сохранить и перезапустить',
        local_ui_settings: 'Настройки интерфейса',
        local_ui_desc: 'Эти параметры сохраняются только в браузере.',
        language_label: 'Язык',
        theme_label: 'Тема',
        language_ru: 'Русский',
        language_en: 'English',
        language_zh: 'Chinese',
        theme_light: 'Светлая',
        theme_dark: 'Тёмная',
        theme_system: 'Системная',
        warning_dimension_mismatch: 'Qdrant collection "{name}" uses dimension {actual}, but the server expects {expected}. Search and indexing may fail until a hard reset is performed.',
        collection_not_found: 'Qdrant collection not found. It will be created on demand or by hard reset.',
        qdrant_collection: 'Qdrant collection:',
        configured_dim: 'Configured dim:',
        actual_dim: 'Actual dim:',
        unknown: 'unknown',
        library_empty: 'Библиотека пуста',
        chunks_label: 'Сегментов',
        length_label: 'Длина',
        tokens_label: 'Примерно токенов',
        chars_label: 'симв.',
        matches_label: 'совпадений',
        page_of: 'PAGE 1 OF 1',
        dialog_cancel: 'Отмена',
        delete_document_title: 'Удалить документ',
        delete_document_message: 'Удалить документ "{filename}" целиком?',
        delete_confirm: 'Удалить',
        delete_failed_title: 'Ошибка удаления',
        delete_failed_message: 'Не удалось удалить документ: {message}',
        clear_confirm_title: 'Очистить библиотеку',
        clear_confirm_message: 'Это удалит все документы, сегменты и эмбеддинги, но сохранит текущую схему коллекции Qdrant. Продолжить?',
        clear_confirm_button: 'Очистить',
        clear_success_title: 'Библиотека очищена',
        clear_success_message: 'Все документы были удалены.',
        clear_failed_title: 'Ошибка очистки',
        clear_failed_message: 'Не удалось очистить библиотеку: {message}',
        print_blocked_title: 'Печать заблокирована',
        print_blocked_message: 'Браузер заблокировал окно печати. Разрешите всплывающие окна для этой страницы.',
        settings_failed_title: 'Ошибка настроек',
        settings_failed_message: 'Не удалось загрузить настройки: {message}',
        settings_saved_title: 'Настройки сохранены',
        settings_saved_message: 'Настройки сохранены.',
        save_failed_title: 'Ошибка сохранения',
        save_failed_message: 'Не удалось сохранить настройки: {message}',
        restarting_title: 'Перезапуск сервера',
        restarting_message: 'Ожидаем, пока backend поднимется с новыми настройками.',
        reindex_title: 'Переиндексация',
        reindex_desc: 'Переиндексирует все существующие в PostgreSQL сегменты документов в Qdrant с текущей размерностью без повторного OCR.',
        reindex_btn_text: 'Переиндексировать',
        reindex_progress_title: 'Переиндексация выполняется',
        reindex_progress_message: 'Пересоздаём коллекцию Qdrant и заново строим эмбеддинги для всех сегментов.',
        reindex_success_title: 'Переиндексация завершена',
        reindex_success_message: 'Документы успешно переиндексированы.',
        reindex_failed_title: 'Ошибка переиндексации',
        reindex_failed_message: 'Не удалось переиндексировать документы: {message}',
        hard_reset_title: 'Hard reset',
        hard_reset_message: 'Hard reset удалит все данные PostgreSQL и пересоздаст коллекцию Qdrant. Продолжить?',
        hard_reset_confirm: 'Запустить hard reset',
        hard_reset_progress_title: 'Hard reset выполняется',
        hard_reset_progress_message: 'Пересоздаём коллекцию Qdrant и очищаем сохранённые данные.',
        hard_reset_success_title: 'Hard reset завершён',
        hard_reset_success_message: 'Коллекция пересоздана, сохранённые данные очищены.',
        hard_reset_failed_title: 'Ошибка hard reset',
        hard_reset_failed_message: 'Hard reset завершился ошибкой: {message}',
        reconnect_timeout_title: 'Таймаут переподключения',
        reconnect_timeout_message: 'Сервер не вернулся вовремя. Проверьте backend-логи.',
        ai_chat_failed_title: 'Ошибка AI чата',
        ai_chat_failed_message: 'Не удалось получить ответ модели: {message}',
        ai_settings_saved_title: 'AI настройки сохранены',
        ai_settings_saved_message: 'AI настройки сохранены и применяются для новых запросов без перезапуска.',
    },
    en: {
        page_title: 'Tensor Machine: Document Portal',
        library: 'Library',
        search_placeholder: 'Search library...',
        clear_library: 'Clear library',
        header_title: 'Document control panel',
        header_subtitle: 'Upload and browse indexed RAG documents',
        system_settings: 'System settings',
        upload_document: 'Upload document',
        toggle_library: 'Library',
        toggle_ai: 'AI',
        dropzone_text: 'Drop a file here or click <strong>Upload</strong>',
        dropzone_subtext: 'Images, PDF, TXT, Markdown, and DOCX are supported',
        processing_file: 'Processing file:',
        step_upload: 'File upload',
        step_ocr: 'Mistral OCR',
        step_chunk: 'Tokenization and chunking',
        step_embedding: 'E5 embedding generation',
        step_db: 'Indexing in Qdrant and PostgreSQL',
        selected_document: 'Selected document',
        metadata: 'Metadata',
        print: 'Print',
        download: 'Download',
        delete: 'Delete',
        visual_layout: 'Visual document layout',
        rendered_content: 'Document content (Markdown / LaTeX / Mermaid)',
        empty_title: 'No document selected',
        empty_text: 'Select a document on the left or upload a new file to preview it',
        chat_title: 'RAG chat',
        chat_subtitle: 'The model receives context via MCP retrieval.',
        chat_placeholder: 'Ask something about the document library...',
        chat_send: 'Send',
        chat_scope_label: 'Limit answers to the selected document',
        chat_rag_label: 'Use RAG retrieval from the library',
        chat_scope_none: 'No document selected',
        chat_scope_active: 'Context will be limited to the selected document',
        chat_scope_general: 'RAG retrieval is off. Chat works without document search',
        chat_fullscreen: 'Fullscreen chat',
        chat_modal_meta: 'Chat with MCP-grounded RAG context',
        clear_chat: 'Clear chat',
        chat_empty: 'Ask a question and the assistant will answer from retrieved library passages.',
        chat_thinking_search: 'Searching relevant passages',
        chat_thinking_reason: 'Reasoning over the context',
        ai_settings_title: 'AI settings',
        ai_settings_subtitle: 'OpenAI-compatible providers, including Gemini.',
        save_ai_settings: 'Save AI settings',
        settings_title: 'Engine settings',
        settings_subtitle: 'Changes are saved to .env, and system parameters require restart.',
        settings_close: 'Close',
        settings_save: 'Save',
        settings_save_restart: 'Save and restart',
        local_ui_settings: 'Interface settings',
        local_ui_desc: 'These preferences are stored only in this browser.',
        language_label: 'Language',
        theme_label: 'Theme',
        language_ru: 'Russian',
        language_en: 'English',
        language_zh: 'Chinese',
        theme_light: 'Light',
        theme_dark: 'Dark',
        theme_system: 'System',
        warning_dimension_mismatch: 'Qdrant collection "{name}" uses dimension {actual}, but the server expects {expected}. Search and indexing may fail until a hard reset is performed.',
        collection_not_found: 'Qdrant collection not found. It will be created on demand or by hard reset.',
        qdrant_collection: 'Qdrant collection:',
        configured_dim: 'Configured dim:',
        actual_dim: 'Actual dim:',
        unknown: 'unknown',
        library_empty: 'Library is empty',
        chunks_label: 'Chunks',
        length_label: 'Length',
        tokens_label: 'Approx tokens',
        chars_label: 'chars',
        matches_label: 'matches',
        page_of: 'PAGE 1 OF 1',
        dialog_cancel: 'Cancel',
        delete_document_title: 'Delete document',
        delete_document_message: 'Delete the entire document "{filename}"?',
        delete_confirm: 'Delete',
        delete_failed_title: 'Delete failed',
        delete_failed_message: 'Failed to delete the document: {message}',
        clear_confirm_title: 'Clear library',
        clear_confirm_message: 'This removes all documents, chunks, and embeddings but keeps the current Qdrant collection schema. Continue?',
        clear_confirm_button: 'Clear',
        clear_success_title: 'Library cleared',
        clear_success_message: 'All documents were removed.',
        clear_failed_title: 'Clear failed',
        clear_failed_message: 'Failed to clear the library: {message}',
        print_blocked_title: 'Print blocked',
        print_blocked_message: 'The browser blocked the print window. Allow pop-ups for this page.',
        settings_failed_title: 'Settings failed',
        settings_failed_message: 'Failed to load settings: {message}',
        settings_saved_title: 'Settings saved',
        settings_saved_message: 'Settings were saved.',
        save_failed_title: 'Save failed',
        save_failed_message: 'Failed to save settings: {message}',
        restarting_title: 'Restarting server',
        restarting_message: 'Waiting for the backend to come back with the new settings.',
        reindex_title: 'Reindex',
        reindex_desc: 'Reindexes all existing document segments in PostgreSQL to Qdrant with the current dimension, without running OCR again.',
        reindex_btn_text: 'Reindex',
        reindex_progress_title: 'Reindexing in progress',
        reindex_progress_message: 'Recreating the Qdrant collection and re-embedding all segments.',
        reindex_success_title: 'Reindex complete',
        reindex_success_message: 'Documents were reindexed successfully.',
        reindex_failed_title: 'Reindex failed',
        reindex_failed_message: 'Failed to reindex documents: {message}',
        hard_reset_title: 'Hard reset',
        hard_reset_message: 'Hard reset will delete all PostgreSQL data and recreate the Qdrant collection. Continue?',
        hard_reset_confirm: 'Run hard reset',
        hard_reset_progress_title: 'Hard reset in progress',
        hard_reset_progress_message: 'Recreating the Qdrant collection and clearing stored data.',
        hard_reset_success_title: 'Hard reset complete',
        hard_reset_success_message: 'The collection was recreated and stored data was cleared.',
        hard_reset_failed_title: 'Hard reset failed',
        hard_reset_failed_message: 'Hard reset failed: {message}',
        reconnect_timeout_title: 'Reconnect timeout',
        reconnect_timeout_message: 'The server did not come back in time. Check the backend logs.',
        ai_chat_failed_title: 'AI chat failed',
        ai_chat_failed_message: 'Failed to get a model response: {message}',
        ai_settings_saved_title: 'AI settings saved',
        ai_settings_saved_message: 'AI settings were saved and will apply to new chat requests immediately.',
    },
};

TEXT.zh = TEXT.en;

export function loadUiPrefs() {
    state.uiPrefs.language = localStorage.getItem(storageKeys.language) || 'ru';
    state.uiPrefs.theme = localStorage.getItem(storageKeys.theme) || 'system';
    state.uiPrefs.leftSidebarCollapsed = localStorage.getItem(storageKeys.leftSidebarCollapsed) === 'true';
    state.uiPrefs.rightSidebarCollapsed = localStorage.getItem(storageKeys.rightSidebarCollapsed) === 'true';
    state.uiPrefs.chatPanelHeight = Number(localStorage.getItem(storageKeys.chatPanelHeight) || 620);
}

export function saveUiPref(key, value) {
    localStorage.setItem(storageKeys[key], String(value));
    state.uiPrefs[key] = value;
}

export function t(key, vars = {}) {
    const set = TEXT[state.uiPrefs.language] || TEXT.en;
    const raw = set[key] || TEXT.en[key] || key;
    return Object.entries(vars).reduce((acc, [name, value]) => acc.replaceAll(`{${name}}`, value), raw);
}

export function resolveThemeMode() {
    if (state.uiPrefs.theme === 'system') {
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    return state.uiPrefs.theme;
}

export function applyTheme() {
    document.documentElement.setAttribute('data-theme', resolveThemeMode());
}

function setText(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.textContent = value;
}

function setHtml(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.innerHTML = value;
}

function setPlaceholder(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.setAttribute('placeholder', value);
}

export function applyTranslations() {
    document.documentElement.lang = state.uiPrefs.language;
    document.title = t('page_title');
    setText('#library-title', t('library'));
    setPlaceholder('#library-search', t('search_placeholder'));
    setHtml('#clear-db-btn', `<i class="fa-solid fa-trash-can"></i> ${t('clear_library')}`);
    setText('#header-title', t('header_title'));
    setText('#header-subtitle', t('header_subtitle'));
    setHtml('#toggle-library-btn', `<i class="fa-solid fa-panel-left"></i> ${t('toggle_library')}`);
    setHtml('#toggle-ai-btn', `<i class="fa-solid fa-panel-right"></i> ${t('toggle_ai')}`);
    setHtml('#open-settings-btn', `<i class="fa-solid fa-gear"></i> ${t('system_settings')}`);
    setHtml('#upload-document-btn', `<i class="fa-solid fa-cloud-arrow-up"></i> ${t('upload_document')}`);
    setHtml('#dropzone-text', t('dropzone_text'));
    setText('#dropzone-subtext', t('dropzone_subtext'));
    setHtml('#status-title', `${escapeHtml(t('processing_file'))} <span id="status-filename">${escapeHtml(document.getElementById('status-filename')?.textContent || 'doc.txt')}</span>`);
    const stepLabels = document.querySelectorAll('.step-label');
    if (stepLabels[0]) stepLabels[0].textContent = t('step_upload');
    if (stepLabels[1]) stepLabels[1].textContent = t('step_ocr');
    if (stepLabels[2]) stepLabels[2].textContent = t('step_chunk');
    if (stepLabels[3]) stepLabels[3].textContent = t('step_embedding');
    if (stepLabels[4]) stepLabels[4].textContent = t('step_db');
    setText('#visual-layout-title', t('visual_layout'));
    setText('#rendered-content-title', t('rendered_content'));
    setText('#empty-title', t('empty_title'));
    setText('#empty-text', t('empty_text'));
    setText('#chat-panel-title', t('chat_title'));
    setText('#chat-panel-subtitle', t('chat_subtitle'));
    setText('#chat-scope-label', t('chat_scope_label'));
    setText('#chat-rag-label', t('chat_rag_label'));
    setText('#chat-modal-title', t('chat_fullscreen'));
    setText('#chat-modal-meta', t('chat_modal_meta'));
    setPlaceholder('#chat-input', t('chat_placeholder'));
    setPlaceholder('#chat-modal-input', t('chat_placeholder'));
    setHtml('#send-chat-btn', `<i class="fa-solid fa-paper-plane"></i> ${t('chat_send')}`);
    setHtml('#send-chat-modal-btn', `<i class="fa-solid fa-paper-plane"></i> ${t('chat_send')}`);
    setText('#ai-settings-title', t('ai_settings_title'));
    setText('#ai-settings-subtitle', t('ai_settings_subtitle'));
    setHtml('#save-ai-settings-btn', `<i class="fa-solid fa-floppy-disk"></i> ${t('save_ai_settings')}`);
    setText('#settings-modal-title', t('settings_title'));
    setHtml('#settings-modal-subtitle', `${escapeHtml(t('settings_subtitle'))}`);
    setText('#settings-cancel-btn', t('settings_close'));
    setHtml('#save-settings-btn', `<i class="fa-solid fa-floppy-disk"></i> ${t('settings_save')}`);
    setHtml('#save-restart-btn', `<i class="fa-solid fa-rotate-right"></i> ${t('settings_save_restart')}`);
    setPlaceholder('#doc-modal-search', 'Search...');
    document.dispatchEvent(new CustomEvent('tensor-rag:translations-applied'));
}

let dialogResolver = null;

export function bindGlobalInfrastructure() {
    const dialogModal = document.getElementById('app-dialog-modal');
    dialogModal.addEventListener('click', (event) => {
        if (event.target === dialogModal) closeDialog(false);
    });
    document.getElementById('app-dialog-confirm-btn').addEventListener('click', () => closeDialog(true));
    document.getElementById('app-dialog-cancel-btn').addEventListener('click', () => closeDialog(false));
    document.addEventListener('click', handleSecretToggleClicks);
}

function handleSecretToggleClicks(event) {
    const button = event.target.closest('[data-secret-toggle]');
    if (!button) return;

    const input = document.getElementById(button.getAttribute('data-secret-toggle'));
    if (!input) return;
    const visible = input.type === 'password';
    input.type = visible ? 'text' : 'password';
    const icon = button.querySelector('i');
    if (icon) icon.className = visible ? 'fa-solid fa-eye-slash' : 'fa-solid fa-eye';
}

export function closeDialog(result) {
    document.getElementById('app-dialog-modal').style.display = 'none';
    if (dialogResolver) {
        const resolve = dialogResolver;
        dialogResolver = null;
        resolve(result);
    }
}

export function showDialog({ title, message, confirmText = 'OK', cancelText = '', variant = 'info' }) {
    const dialog = document.getElementById('app-dialog-modal');
    const content = dialog.querySelector('.app-dialog-content');
    const icon = document.getElementById('app-dialog-icon');
    const titleEl = document.getElementById('app-dialog-title');
    const messageEl = document.getElementById('app-dialog-message');
    const confirmBtn = document.getElementById('app-dialog-confirm-btn');
    const cancelBtn = document.getElementById('app-dialog-cancel-btn');

    content.classList.remove('is-danger', 'is-warning', 'is-success');
    if (variant === 'danger') content.classList.add('is-danger');
    if (variant === 'warning') content.classList.add('is-warning');
    if (variant === 'success') content.classList.add('is-success');

    icon.className = 'confirm-icon fa-solid ' + (
        variant === 'danger' ? 'fa-triangle-exclamation' :
        variant === 'warning' ? 'fa-circle-exclamation' :
        variant === 'success' ? 'fa-circle-check' :
        'fa-circle-info'
    );

    titleEl.textContent = title;
    messageEl.textContent = message;
    confirmBtn.textContent = confirmText;
    cancelBtn.textContent = cancelText || t('dialog_cancel');
    cancelBtn.style.display = cancelText ? 'inline-flex' : 'none';
    dialog.style.display = 'flex';

    return new Promise((resolve) => {
        dialogResolver = resolve;
    });
}

export function showNotice(title, message, variant = 'info') {
    return showDialog({ title, message, confirmText: 'OK', variant });
}

export function askConfirmation(title, message, confirmText, cancelText = t('dialog_cancel'), variant = 'warning') {
    return showDialog({ title, message, confirmText, cancelText, variant });
}

export async function fetchJson(url, options = {}) {
    const response = await fetch(url, options);
    let payload = null;
    try {
        payload = await response.json();
    } catch (_error) {
        payload = null;
    }

    if (!response.ok) {
        throw new Error(payload?.error || `HTTP error ${response.status}`);
    }
    return payload;
}

export function renderWarningBanner(message) {
    const banner = document.getElementById('app-warning-banner');
    if (!message) {
        banner.style.display = 'none';
        banner.textContent = '';
        return;
    }
    banner.textContent = message;
    banner.style.display = 'block';
}

export function buildLocalizedWarning(health) {
    if (!health || !health.exists || health.dimension_matches) return null;
    return t('warning_dimension_mismatch', {
        name: health.collection_name,
        actual: String(health.actual_dim ?? t('unknown')),
        expected: String(health.configured_dim),
    });
}

export function showReconnectOverlay(title, message) {
    document.getElementById('reconnect-title').textContent = title;
    document.getElementById('reconnect-message').textContent = message;
    document.getElementById('reconnect-overlay').style.display = 'flex';
}

export function hideReconnectOverlay() {
    document.getElementById('reconnect-overlay').style.display = 'none';
}

export async function waitForReconnect() {
    const startedAt = Date.now();
    while (Date.now() - startedAt < 45000) {
        try {
            await fetchJson('/documents');
            hideReconnectOverlay();
            return;
        } catch (_error) {
            await new Promise((resolve) => setTimeout(resolve, 1500));
        }
    }
    hideReconnectOverlay();
    throw new Error(t('reconnect_timeout_message'));
}

export function escapeHtml(value) {
    return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

export function escapeHtmlAttr(value) {
    return escapeHtml(value).replaceAll('`', '&#96;');
}
