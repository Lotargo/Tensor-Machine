document.addEventListener('DOMContentLoaded', () => {
    initApp();
});

let documentList = [];
let rawChunks = [];
let selectedDocUuid = null;
let currentPreviewDoc = null;
let collectionHealth = null;
let uiPrefs = {
    language: 'ru',
    theme: 'system'
};

const UI_STORAGE_KEYS = {
    language: 'tensor-rag-ui-language',
    theme: 'tensor-rag-ui-theme'
};

const UI_TEXT = {
    ru: {
        page_title: 'Tensor Machine: Document Portal',
        library: 'Библиотека',
        search_placeholder: 'Поиск в библиотеке...',
        clear_library: 'Очистить библиотеку',
        header_title: 'Панель управления документами',
        header_subtitle: 'Загружайте и просматривайте проиндексированные документы RAG',
        settings: 'Настройки',
        upload_document: 'Загрузить документ',
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
        settings_title: 'Настройки движка',
        settings_subtitle: 'Изменения сохраняются в .env, а для применения требуется перезапуск.',
        settings_close: 'Закрыть',
        settings_save: 'Сохранить',
        settings_save_restart: 'Сохранить и перезапустить',
        hard_reset_desc: 'Полностью очищает PostgreSQL и заново создаёт коллекцию Qdrant с актуальной размерностью.',
        warning_dimension_mismatch: 'Коллекция Qdrant "{name}" имеет размерность {actual}, а сервер ожидает {expected}. Поиск и индексация могут завершаться ошибкой, пока не будет выполнен hard reset.',
        reconnect_title: 'Переподключение к серверу',
        reconnect_message: 'Ожидаем, пока backend вернётся после перезапуска.',
        dialog_title: 'Сообщение',
        dialog_message: 'Текст сообщения',
        dialog_cancel: 'Отмена',
        doc_search_placeholder: 'Поиск по словам в документе...',
        local_ui_settings: 'Настройки интерфейса',
        local_ui_desc: 'Эти параметры сохраняются только в браузере.',
        language_label: 'Язык',
        theme_label: 'Тема',
        language_ru: 'Русский',
        language_en: 'English',
        language_zh: '中文',
        theme_light: 'Светлая',
        theme_dark: 'Тёмная',
        theme_system: 'Системная',
        collection_not_found: 'Коллекция Qdrant не найдена. Она будет создана по требованию или через hard reset.',
        qdrant_collection: 'Коллекция Qdrant:',
        configured_dim: 'Ожидаемая размерность:',
        actual_dim: 'Фактическая размерность:',
        unknown: 'неизвестно',
        library_empty: 'Библиотека пуста',
        chunks_label: 'Сегментов',
        length_label: 'Длина',
        tokens_label: 'Примерно токенов',
        chars_label: 'симв.',
        matches_label: 'совпадений',
        print_blocked_title: 'Печать заблокирована',
        print_blocked_message: 'Браузер заблокировал окно печати. Разрешите всплывающие окна для этой страницы.',
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
        settings_failed_title: 'Ошибка настроек',
        settings_failed_message: 'Не удалось загрузить настройки: {message}',
        settings_saved_title: 'Настройки сохранены',
        settings_saved_message: 'Настройки сохранены. Перезапустите сервер, чтобы применить их.',
        save_failed_title: 'Ошибка сохранения',
        save_failed_message: 'Не удалось сохранить настройки: {message}',
        restarting_title: 'Перезапуск сервера',
        restarting_message: 'Ожидаем, пока backend поднимется с новыми настройками.',
        hard_reset_title: 'Hard reset',
        hard_reset_message: 'Hard reset удалит все данные PostgreSQL и пересоздаст коллекцию Qdrant. Продолжить?',
        hard_reset_confirm: 'Запустить hard reset',
        hard_reset_progress_title: 'Hard reset выполняется',
        hard_reset_progress_message: 'Пересоздаём коллекцию Qdrant и очищаем сохранённые данные.',
        hard_reset_success_title: 'Hard reset завершён',
        hard_reset_success_message: 'Коллекция пересоздана, сохранённые данные очищены.',
        hard_reset_failed_title: 'Ошибка hard reset',
        hard_reset_failed_message: 'Hard reset завершился ошибкой: {message}',
        reindex_desc: 'Переиндексирует все существующие в PostgreSQL сегменты документов в Qdrant с текущей размерностью без повторного OCR.',
        reindex_title: 'Переиндексация',
        reindex_btn_text: 'Переиндексировать',
        reindex_progress_title: 'Переиндексация выполняется',
        reindex_progress_message: 'Пересоздаём коллекцию Qdrant и заново строим эмбеддинги для всех сегментов.',
        reindex_success_title: 'Переиндексация завершена',
        reindex_success_message: 'Коллекция пересоздана с текущей размерностью, сегменты документов успешно переиндексированы.',
        reindex_failed_title: 'Ошибка переиндексации',
        reindex_failed_message: 'Не удалось переиндексировать документы: {message}',
        reconnect_timeout_title: 'Таймаут переподключения',
        reconnect_timeout_message: 'Сервер не вернулся вовремя. Проверьте backend-логи.',
        doc_preview_title: 'Документ',
        page_of: 'PAGE 1 OF 1'
    },
    en: {
        page_title: 'Tensor Machine: Document Portal',
        library: 'Library',
        search_placeholder: 'Search library...',
        clear_library: 'Clear library',
        header_title: 'Document control panel',
        header_subtitle: 'Upload and browse indexed RAG documents',
        settings: 'Settings',
        upload_document: 'Upload document',
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
        settings_title: 'Engine settings',
        settings_subtitle: 'Changes are saved to .env and require a restart to apply.',
        settings_close: 'Close',
        settings_save: 'Save',
        settings_save_restart: 'Save and restart',
        hard_reset_desc: 'Fully clears PostgreSQL and recreates the Qdrant collection with the current dimension.',
        warning_dimension_mismatch: 'Qdrant collection "{name}" uses dimension {actual}, but the server expects {expected}. Search and indexing may fail until a hard reset is performed.',
        reconnect_title: 'Reconnecting to server',
        reconnect_message: 'Waiting for the backend to return after restart.',
        dialog_title: 'Message',
        dialog_message: 'Message text',
        dialog_cancel: 'Cancel',
        doc_search_placeholder: 'Search within document...',
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
        print_blocked_title: 'Print blocked',
        print_blocked_message: 'The browser blocked the print window. Allow pop-ups for this page.',
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
        settings_failed_title: 'Settings failed',
        settings_failed_message: 'Failed to load settings: {message}',
        settings_saved_title: 'Settings saved',
        settings_saved_message: 'Settings were saved. Restart the server to apply them.',
        save_failed_title: 'Save failed',
        save_failed_message: 'Failed to save settings: {message}',
        restarting_title: 'Restarting server',
        restarting_message: 'Waiting for the backend to come back with the new settings.',
        hard_reset_title: 'Hard reset',
        hard_reset_message: 'Hard reset will delete all PostgreSQL data and recreate the Qdrant collection. Continue?',
        hard_reset_confirm: 'Run hard reset',
        hard_reset_progress_title: 'Hard reset in progress',
        hard_reset_progress_message: 'Recreating the Qdrant collection and clearing stored data.',
        hard_reset_success_title: 'Hard reset complete',
        hard_reset_success_message: 'The collection was recreated and stored data was cleared.',
        hard_reset_failed_title: 'Hard reset failed',
        hard_reset_failed_message: 'Hard reset failed: {message}',
        reindex_desc: 'Reindexes all existing document segments in PostgreSQL to Qdrant with the current dimension, without running OCR again.',
        reindex_title: 'Reindex',
        reindex_btn_text: 'Reindex',
        reindex_progress_title: 'Reindexing in progress',
        reindex_progress_message: 'Recreating Qdrant collection and re-embedding all document segments.',
        reindex_success_title: 'Reindexing complete',
        reindex_success_message: 'Collection recreated with the current dimension and segments successfully re-embedded.',
        reindex_failed_title: 'Reindexing failed',
        reindex_failed_message: 'Failed to reindex documents: {message}',
        reconnect_timeout_title: 'Reconnect timeout',
        reconnect_timeout_message: 'The server did not come back in time. Check the backend logs.',
        doc_preview_title: 'Document',
        page_of: 'PAGE 1 OF 1'
    },
    zh: {
        page_title: 'Tensor Machine: 文档门户',
        library: '资料库',
        search_placeholder: '搜索资料库...',
        clear_library: '清空资料库',
        header_title: '文档控制面板',
        header_subtitle: '上传并查看已建立索引的 RAG 文档',
        settings: '设置',
        upload_document: '上传文档',
        dropzone_text: '将文件拖到此处，或点击<strong>上传</strong>',
        dropzone_subtext: '支持图片、PDF、TXT、Markdown 和 DOCX',
        processing_file: '正在处理文件：',
        step_upload: '上传文件',
        step_ocr: 'Mistral OCR',
        step_chunk: '分词与切块',
        step_embedding: '生成 E5 向量',
        step_db: '索引到 Qdrant 和 PostgreSQL',
        selected_document: '已选中文档',
        metadata: '元数据',
        print: '打印',
        download: '下载',
        delete: '删除',
        visual_layout: '文档预览布局',
        rendered_content: '文档内容（Markdown / LaTeX / Mermaid）',
        empty_title: '未选择文档',
        empty_text: '请从左侧选择文档，或上传新文件进行预览',
        settings_title: '引擎设置',
        settings_subtitle: '修改会保存到 .env，应用前需要重启。',
        settings_close: '关闭',
        settings_save: '保存',
        settings_save_restart: '保存并重启',
        hard_reset_desc: '完全清空 PostgreSQL，并按当前维度重新创建 Qdrant 集合。',
        warning_dimension_mismatch: 'Qdrant 集合“{name}”当前维度为 {actual}，但服务器期望 {expected}。在执行 hard reset 前，搜索和索引可能会失败。',
        reconnect_title: '正在重新连接服务器',
        reconnect_message: '等待后端在重启后恢复。',
        dialog_title: '消息',
        dialog_message: '消息内容',
        dialog_cancel: '取消',
        doc_search_placeholder: '在文档中搜索...',
        local_ui_settings: '界面设置',
        local_ui_desc: '这些偏好仅保存在当前浏览器中。',
        language_label: '语言',
        theme_label: '主题',
        language_ru: '俄语',
        language_en: '英语',
        language_zh: '中文',
        theme_light: '浅色',
        theme_dark: '深色',
        theme_system: '跟随系统',
        collection_not_found: '未找到 Qdrant 集合。它会在需要时创建，或通过 hard reset 重建。',
        qdrant_collection: 'Qdrant 集合：',
        configured_dim: '配置维度：',
        actual_dim: '实际维度：',
        unknown: '未知',
        library_empty: '资料库为空',
        chunks_label: '分块数',
        length_label: '长度',
        tokens_label: '约 token 数',
        chars_label: '字符',
        matches_label: '匹配',
        print_blocked_title: '打印被拦截',
        print_blocked_message: '浏览器拦截了打印窗口。请允许此页面弹出窗口。',
        delete_document_title: '删除文档',
        delete_document_message: '要删除整个文档“{filename}”吗？',
        delete_confirm: '删除',
        delete_failed_title: '删除失败',
        delete_failed_message: '删除文档失败：{message}',
        clear_confirm_title: '清空资料库',
        clear_confirm_message: '这将删除所有文档、分块和向量，但保留当前 Qdrant 集合结构。是否继续？',
        clear_confirm_button: '清空',
        clear_success_title: '资料库已清空',
        clear_success_message: '所有文档都已删除。',
        clear_failed_title: '清空失败',
        clear_failed_message: '无法清空资料库：{message}',
        settings_failed_title: '设置加载失败',
        settings_failed_message: '无法加载设置：{message}',
        settings_saved_title: '设置已保存',
        settings_saved_message: '设置已保存。请重启服务器以应用更改。',
        save_failed_title: '保存失败',
        save_failed_message: '无法保存设置：{message}',
        restarting_title: '正在重启服务器',
        restarting_message: '等待后端使用新设置重新启动。',
        hard_reset_title: 'Hard reset',
        hard_reset_message: 'Hard reset 将删除所有 PostgreSQL 数据并重新创建 Qdrant 集合。是否继续？',
        hard_reset_confirm: '执行 hard reset',
        hard_reset_progress_title: '正在执行 hard reset',
        hard_reset_progress_message: '正在重建 Qdrant 集合并清空已保存数据。',
        hard_reset_success_title: 'Hard reset 完成',
        hard_reset_success_message: '集合已重建，保存的数据已清空。',
        hard_reset_failed_title: 'Hard reset 失败',
        hard_reset_failed_message: 'Hard reset 失败：{message}',
        reindex_desc: '以当前维度重新将 PostgreSQL 中的现有文档分块索引到 Qdrant，无需再次运行 OCR。',
        reindex_title: '重新索引',
        reindex_btn_text: '重新索引',
        reindex_progress_title: '正在重新索引',
        reindex_progress_message: '正在重建 Qdrant 集合并重新嵌入所有文档分块。',
        reindex_success_title: '重新索引完成',
        reindex_success_message: '已使用当前维度重建集合，并成功重新嵌入分块。',
        reindex_failed_title: '重新索引失败',
        reindex_failed_message: '无法重新索引文档：{message}',
        reconnect_timeout_title: '重连超时',
        reconnect_timeout_message: '服务器未能及时恢复。请检查后端日志。',
        doc_preview_title: '文档',
        page_of: '第 1 页 / 共 1 页'
    }
};

function loadUiPrefs() {
    uiPrefs.language = localStorage.getItem(UI_STORAGE_KEYS.language) || 'ru';
    uiPrefs.theme = localStorage.getItem(UI_STORAGE_KEYS.theme) || 'system';
}

function saveUiPref(key, value) {
    uiPrefs[key] = value;
    localStorage.setItem(UI_STORAGE_KEYS[key], value);
}

function currentText() {
    return UI_TEXT[uiPrefs.language] || UI_TEXT.ru;
}

function t(key, vars = {}) {
    const value = currentText()[key] ?? UI_TEXT.en[key] ?? key;
    return Object.entries(vars).reduce(
        (acc, [name, replacement]) => acc.replaceAll(`{${name}}`, replacement),
        value
    );
}

function resolveThemeMode() {
    if (uiPrefs.theme === 'system') {
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    return uiPrefs.theme;
}

function applyTheme() {
    document.documentElement.setAttribute('data-theme', resolveThemeMode());
}

function applyTranslations() {
    document.documentElement.lang = uiPrefs.language;
    document.title = t('page_title');

    const setText = (selector, value) => {
        const el = document.querySelector(selector);
        if (el) el.textContent = value;
    };
    const setHtml = (selector, value) => {
        const el = document.querySelector(selector);
        if (el) el.innerHTML = value;
    };
    const setPlaceholder = (selector, value) => {
        const el = document.querySelector(selector);
        if (el) el.setAttribute('placeholder', value);
    };

    setText('.sidebar-header h2', t('library'));
    setPlaceholder('#library-search', t('search_placeholder'));
    setHtml('#clear-db-btn', `<i class="fa-solid fa-trash-can"></i> ${t('clear_library')}`);
    setText('.header-info h1', t('header_title'));
    setText('.header-info p', t('header_subtitle'));
    setHtml('#open-settings-btn', `<i class="fa-solid fa-gear"></i> ${t('settings')}`);
    setHtml('.btn.btn-primary[onclick*="file-input"]', `<i class="fa-solid fa-cloud-arrow-up"></i> ${t('upload_document')}`);
    setHtml('#drop-zone p', t('dropzone_text'));
    setText('.upload-subtext', t('dropzone_subtext'));
    setHtml('#download-doc-btn', `<i class="fa-solid fa-download"></i> ${t('download')}`);
    setHtml('#print-doc-btn', `<i class="fa-solid fa-print"></i> ${t('print')}`);
    setHtml('#delete-doc-btn', `<i class="fa-solid fa-trash-can"></i> ${t('delete')}`);
    setText('.canvas-pane .pane-title', t('visual_layout'));
    setText('.rendered-pane .pane-title', t('rendered_content'));
    setText('#empty-state h3', t('empty_title'));
    setText('#empty-state p', t('empty_text'));
    setText('#doc-modal-title', currentPreviewDoc ? currentPreviewDoc.filename : t('doc_preview_title'));
    setPlaceholder('#doc-modal-search', t('doc_search_placeholder'));
    setText('#settings-modal .settings-header h3', t('settings_title'));
    setText('#settings-modal .settings-header p', t('settings_subtitle'));
    setHtml('#save-settings-btn', `<i class="fa-solid fa-floppy-disk"></i> ${t('settings_save')}`);
    setHtml('#save-restart-btn', `<i class="fa-solid fa-rotate-right"></i> ${t('settings_save_restart')}`);
    setText('#settings-cancel-btn', t('settings_close'));
    setText('#hard-reset-desc-text', t('hard_reset_desc'));
    setText('#reindex-title-text', t('reindex_title'));
    setText('#reindex-desc-text', t('reindex_desc'));
    setHtml('#reindex-btn', `<i class="fa-solid fa-arrows-rotate"></i> ${t('reindex_btn_text')}`);
    setText('#reconnect-title', t('reconnect_title'));
    setText('#reconnect-message', t('reconnect_message'));
    setText('#app-dialog-title', t('dialog_title'));
    setText('#app-dialog-message', t('dialog_message'));
    setText('#app-dialog-cancel-btn', t('dialog_cancel'));
    const statusHeading = document.querySelector('#status-panel h3');
    const statusFilename = document.getElementById('status-filename');
    if (statusHeading && statusFilename) {
        statusHeading.innerHTML = `${escapeHtml(t('processing_file'))} <span id="status-filename">${escapeHtml(statusFilename.textContent || 'doc.txt')}</span>`;
    }
    setText('.mini-doc-footer span:last-child', t('page_of'));

    const stepLabels = document.querySelectorAll('.step-label');
    if (stepLabels[0]) stepLabels[0].textContent = t('step_upload');
    if (stepLabels[1]) stepLabels[1].textContent = t('step_ocr');
    if (stepLabels[2]) stepLabels[2].textContent = t('step_chunk');
    if (stepLabels[3]) stepLabels[3].textContent = t('step_embedding');
    if (stepLabels[4]) stepLabels[4].textContent = t('step_db');

    if (documentList.length) {
        renderDocumentList(documentList);
        if (selectedDocUuid) selectDocument(selectedDocUuid);
    }
}

function initApp() {
    loadUiPrefs();
    applyTheme();
    initStaticBindings();
    applyTranslations();
    initUpload();
    fetchDocuments();
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
        if (uiPrefs.theme === 'system') {
            applyTheme();
        }
    });
}

function initStaticBindings() {
    const searchInput = document.getElementById('library-search');
    searchInput.addEventListener('input', (e) => filterLibrary(e.target.value));

    document.getElementById('print-doc-btn').addEventListener('click', printActiveDocument);
    document.getElementById('download-doc-btn').addEventListener('click', downloadActiveDocument);
    document.getElementById('delete-doc-btn').addEventListener('click', deleteActiveDocument);

    document.getElementById('clear-db-btn').addEventListener('click', clearLibrary);

    const miniPreview = document.getElementById('document-mini-preview');
    if (miniPreview) miniPreview.addEventListener('click', openDocPreviewModal);

    const docPreviewModal = document.getElementById('doc-preview-modal');
    docPreviewModal.addEventListener('click', (e) => {
        if (e.target === docPreviewModal) closeDocPreviewModal();
    });
    document.getElementById('doc-modal-close-btn').addEventListener('click', closeDocPreviewModal);
    document.getElementById('doc-modal-search').addEventListener('input', (e) => {
        rerenderPreviewModal(e.target.value);
    });

    const settingsModal = document.getElementById('settings-modal');
    document.getElementById('open-settings-btn').addEventListener('click', openSettingsModal);
    document.getElementById('settings-close-btn').addEventListener('click', closeSettingsModal);
    document.getElementById('settings-cancel-btn').addEventListener('click', closeSettingsModal);
    settingsModal.addEventListener('click', (e) => {
        if (e.target === settingsModal) closeSettingsModal();
    });
    document.getElementById('save-settings-btn').addEventListener('click', () => saveSettings(false));
    document.getElementById('save-restart-btn').addEventListener('click', () => saveSettings(true));
    document.getElementById('reindex-btn').addEventListener('click', reindexLibrary);
    document.getElementById('hard-reset-btn').addEventListener('click', hardReset);

    const dialogModal = document.getElementById('app-dialog-modal');
    dialogModal.addEventListener('click', (e) => {
        if (e.target === dialogModal) closeDialog(false);
    });
    document.getElementById('app-dialog-confirm-btn').addEventListener('click', () => closeDialog(true));
    document.getElementById('app-dialog-cancel-btn').addEventListener('click', () => closeDialog(false));
    document.addEventListener('click', handleGlobalButtonClicks);
}

function handleGlobalButtonClicks(event) {
    const toggleBtn = event.target.closest('[data-secret-toggle]');
    if (!toggleBtn) return;

    const inputId = toggleBtn.getAttribute('data-secret-toggle');
    const input = document.getElementById(inputId);
    if (!input) return;

    const makeVisible = input.type === 'password';
    input.type = makeVisible ? 'text' : 'password';

    const icon = toggleBtn.querySelector('i');
    if (icon) {
        icon.className = makeVisible ? 'fa-solid fa-eye-slash' : 'fa-solid fa-eye';
    }
    toggleBtn.setAttribute('aria-label', makeVisible ? 'Скрыть ключ' : 'Показать ключ');
}

let dialogResolver = null;
function closeDialog(result) {
    const dialog = document.getElementById('app-dialog-modal');
    dialog.style.display = 'none';
    if (dialogResolver) {
        const resolve = dialogResolver;
        dialogResolver = null;
        resolve(result);
    }
}

function showDialog({ title, message, confirmText = 'OK', cancelText = '', variant = 'info' }) {
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

    titleEl.innerText = title;
    messageEl.innerText = message;
    confirmBtn.innerText = confirmText;
    cancelBtn.innerText = cancelText || 'Cancel';
    cancelBtn.style.display = cancelText ? 'inline-flex' : 'none';
    dialog.style.display = 'flex';

    return new Promise((resolve) => {
        dialogResolver = resolve;
    });
}

function showNotice(title, message, variant = 'info') {
    return showDialog({ title, message, confirmText: 'OK', variant });
}

function askConfirmation(title, message, confirmText = 'Confirm', cancelText = 'Cancel', variant = 'warning') {
    return showDialog({ title, message, confirmText, cancelText, variant });
}

async function fetchJson(url, options = {}) {
    const response = await fetch(url, options);
    let payload = null;

    try {
        payload = await response.json();
    } catch (_err) {
        payload = null;
    }

    if (!response.ok) {
        const message = payload && payload.error
            ? payload.error
            : `HTTP error ${response.status}`;
        throw new Error(message);
    }

    return payload;
}

function getGroupedDocuments(chunks) {
    const grouped = {};
    chunks.forEach((chunk) => {
        const uuid = chunk.doc_uuid || `legacy-${chunk.id}`;
        if (!grouped[uuid]) {
            grouped[uuid] = {
                doc_uuid: uuid,
                filename: chunk.filename || 'Legacy Chunk',
                chunks: []
            };
        }
        grouped[uuid].chunks.push(chunk);
    });

    const docs = Object.values(grouped);
    docs.forEach((doc) => {
        doc.chunks.sort((a, b) => a.id - b.id);
        doc.text = doc.chunks.map((chunk) => chunk.text).join('\n\n');
        doc.id = doc.chunks[0].id;
    });

    return docs;
}

async function fetchDocuments() {
    try {
        const data = await fetchJson('/documents');
        rawChunks = data.documents || [];
        collectionHealth = data.collection_health || null;
        documentList = getGroupedDocuments(rawChunks);
        renderWarningBanner(buildLocalizedWarning(collectionHealth) || data.warning || null);
        renderDocumentList(documentList);

        if (selectedDocUuid) {
            const stillExists = documentList.some((doc) => doc.doc_uuid === selectedDocUuid);
            if (stillExists) {
                selectDocument(selectedDocUuid);
            } else {
                resetViewerState();
            }
        } else {
            resetViewerState();
        }
    } catch (err) {
        console.error('Failed to fetch documents from library:', err);
        renderWarningBanner(`Failed to load the library: ${err.message}`);
    }
}

function renderWarningBanner(message) {
    const banner = document.getElementById('app-warning-banner');
    if (!message) {
        banner.style.display = 'none';
        banner.textContent = '';
        return;
    }
    banner.textContent = message;
    banner.style.display = 'block';
}

function buildLocalizedWarning(health) {
    if (!health || !health.exists || health.dimension_matches) return null;
    return t('warning_dimension_mismatch', {
        name: health.collection_name,
        actual: String(health.actual_dim ?? t('unknown')),
        expected: String(health.configured_dim)
    });
}

function renderDocumentList(docs) {
    const listEl = document.getElementById('document-list');
    listEl.innerHTML = '';

    if (docs.length === 0) {
        listEl.innerHTML = `<li class="document-item" style="cursor: default; text-align: center; color: var(--text-secondary);">${escapeHtml(t('library_empty'))}</li>`;
        return;
    }

    docs.forEach((doc) => {
        const li = document.createElement('li');
        li.className = 'document-item';
        if (doc.doc_uuid === selectedDocUuid) li.className += ' active';

        li.innerHTML = `
            <div class="document-item-title" title="${escapeHtml(doc.filename)}">${escapeHtml(doc.filename)}</div>
            <div class="document-item-meta" style="font-size: 10px; margin-top: 4px; color: var(--text-secondary); display: flex; flex-direction: column; gap: 2px;">
                <span style="font-family: ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">UUID: ${escapeHtml(doc.doc_uuid)}</span>
                <span>${escapeHtml(t('chunks_label'))}: ${doc.chunks.length}</span>
            </div>
        `;

        li.addEventListener('click', () => selectDocument(doc.doc_uuid));
        listEl.appendChild(li);
    });
}

function filterLibrary(query) {
    if (!query) {
        renderDocumentList(documentList);
        return;
    }

    const q = query.toLowerCase();
    const filtered = documentList.filter((doc) =>
        doc.filename.toLowerCase().includes(q) ||
        doc.doc_uuid.toLowerCase().includes(q) ||
        doc.text.toLowerCase().includes(q)
    );
    renderDocumentList(filtered);
}

function resetViewerState() {
    selectedDocUuid = null;
    document.getElementById('viewer-section').style.display = 'none';
    document.getElementById('empty-state').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'flex';
}

function selectDocument(uuid) {
    selectedDocUuid = uuid;
    const doc = documentList.find((item) => item.doc_uuid === uuid);
    if (!doc) return;

    document.getElementById('empty-state').style.display = 'none';
    document.getElementById('viewer-section').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'none';

    document.getElementById('viewer-title').innerText = doc.filename;
    const charCount = doc.text.length;
    const tokenEst = Math.round(charCount / 4);
    document.getElementById('viewer-meta').innerText = `${t('chunks_label')}: ${doc.chunks.length} | ${t('length_label')}: ${charCount} ${t('chars_label')} | ${t('tokens_label')}: ${tokenEst} | UUID: ${doc.doc_uuid}`;

    const contentEl = document.getElementById('rendered-content');
    const renderer = new marked.Renderer();
    renderer.code = function(code, language) {
        if (language === 'mermaid') {
            return `<pre class="mermaid">${code}</pre>`;
        }
        return `<pre><code>${code}</code></pre>`;
    };
    marked.setOptions({ renderer });

    let parsedHTML = marked.parse(doc.text);
    parsedHTML = renderMath(parsedHTML);
    contentEl.innerHTML = parsedHTML;

    try {
        if (window.mermaid) {
            window.mermaid.init(undefined, contentEl.querySelectorAll('.mermaid'));
        }
    } catch (err) {
        console.error('Mermaid layout render failed:', err);
    }

    updateMiniPreview(doc);
    renderDocumentList(documentList);
}

function renderMath(text) {
    let html = text.replace(/\$\$([\s\S]+?)\$\$/g, (match, math) => {
        try {
            return `<div class="katex-block" style="margin: 16px 0; text-align: center;">${window.katex.renderToString(math, { displayMode: true, throwOnError: false })}</div>`;
        } catch (_err) {
            return match;
        }
    });

    html = html.replace(/\$([^\$\n]+?)\$/g, (match, math) => {
        try {
            return `<span class="katex-inline">${window.katex.renderToString(math, { displayMode: false, throwOnError: false })}</span>`;
        } catch (_err) {
            return match;
        }
    });

    return html;
}

function updateMiniPreview(doc) {
    const extEl = document.getElementById('mini-doc-ext');
    const bodyEl = document.getElementById('mini-doc-body');
    const nameEl = document.getElementById('mini-doc-name');
    if (!extEl || !bodyEl || !nameEl) return;

    extEl.innerText = doc.filename.split('.').pop().toUpperCase();
    nameEl.innerText = doc.filename;
    bodyEl.innerText = doc.text.replace(/[#*`_~\[\]]/g, '').substring(0, 800);
}

function openDocPreviewModal() {
    if (!selectedDocUuid) return;
    const doc = documentList.find((item) => item.doc_uuid === selectedDocUuid);
    if (!doc) return;

    currentPreviewDoc = doc;
    const modal = document.getElementById('doc-preview-modal');
    document.getElementById('doc-modal-search').value = '';
    document.getElementById('doc-modal-search-count').innerText = '';
    document.getElementById('doc-modal-title').innerText = doc.filename;
    document.getElementById('doc-modal-meta').innerText = `${t('chunks_label')}: ${doc.chunks.length} | ${t('length_label')}: ${doc.text.length} ${t('chars_label')} | UUID: ${doc.doc_uuid}`;

    rerenderPreviewModal('');

    const modalContentContainer = modal.querySelector('.doc-modal-content');
    modal.style.display = 'flex';
    modalContentContainer.offsetHeight;
    modalContentContainer.style.transform = 'scale(1)';
    modalContentContainer.style.opacity = '1';
}

function rerenderPreviewModal(query) {
    if (!currentPreviewDoc) return;
    const modalContent = document.getElementById('doc-modal-content');
    let parsedHTML = marked.parse(currentPreviewDoc.text);
    parsedHTML = renderMath(parsedHTML);
    modalContent.innerHTML = parsedHTML;

    try {
        if (window.mermaid) {
            window.mermaid.init(undefined, modalContent.querySelectorAll('.mermaid'));
        }
    } catch (_err) {}

    highlightSearchTerms(modalContent, query);
}

function closeDocPreviewModal() {
    const modal = document.getElementById('doc-preview-modal');
    const modalContentContainer = modal.querySelector('.doc-modal-content');
    modalContentContainer.style.transform = 'scale(0.9)';
    modalContentContainer.style.opacity = '0';

    setTimeout(() => {
        modal.style.display = 'none';
        currentPreviewDoc = null;
    }, 300);
}

function highlightSearchTerms(container, query) {
    removeHighlights(container);
    if (!query || query.trim().length === 0) {
        document.getElementById('doc-modal-search-count').innerText = '';
        return;
    }

    const terms = query.trim().replace(/[-\/\\^$*+?.()|[\]{}]/g, '\\$&');
    const regex = new RegExp(`(${terms})`, 'gi');
    let count = 0;

    function walk(node) {
        if (node.nodeType === 3) {
            const matches = node.nodeValue.match(regex);
            if (matches) {
                const span = document.createElement('span');
                span.className = 'highlight-wrapper';
                span.innerHTML = node.nodeValue.replace(regex, '<mark class="search-match" style="background-color: #fef08a; color: #854d0e; padding: 2px 2px; border-radius: 2px; font-weight: 600;">$1</mark>');
                node.parentNode.replaceChild(span, node);
                count += matches.length;
            }
        } else if (node.nodeType === 1 && node.childNodes && !['SCRIPT', 'STYLE', 'TEXTAREA', 'PRE', 'CODE'].includes(node.nodeName)) {
            for (let i = node.childNodes.length - 1; i >= 0; i--) {
                walk(node.childNodes[i]);
            }
        }
    }

    walk(container);
    document.getElementById('doc-modal-search-count').innerText = `${count} ${t('matches_label')}`;

    const firstMatch = container.querySelector('.search-match');
    if (firstMatch) firstMatch.scrollIntoView({ behavior: 'smooth', block: 'center' });
}

function removeHighlights(container) {
    container.querySelectorAll('.highlight-wrapper').forEach((wrapper) => {
        wrapper.parentNode.replaceChild(document.createTextNode(wrapper.textContent), wrapper);
    });
    container.normalize();
}

function initUpload() {
    const dropZone = document.getElementById('drop-zone');
    const fileInput = document.getElementById('file-input');

    ['dragenter', 'dragover', 'dragleave', 'drop'].forEach((eventName) => {
        dropZone.addEventListener(eventName, preventDefaults, false);
    });

    function preventDefaults(e) {
        e.preventDefault();
        e.stopPropagation();
    }

    ['dragenter', 'dragover'].forEach((eventName) => {
        dropZone.addEventListener(eventName, () => dropZone.classList.add('dragover'), false);
    });
    ['dragleave', 'drop'].forEach((eventName) => {
        dropZone.addEventListener(eventName, () => dropZone.classList.remove('dragover'), false);
    });

    dropZone.addEventListener('drop', (e) => {
        const files = e.dataTransfer.files;
        if (files.length > 0) handleFilesUpload(files[0]);
    });

    fileInput.addEventListener('change', (e) => {
        if (e.target.files.length > 0) handleFilesUpload(e.target.files[0]);
    });
}

async function handleFilesUpload(file) {
    const statusPanel = document.getElementById('status-panel');
    const statusFilename = document.getElementById('status-filename');
    const statusError = document.getElementById('status-error');
    statusFilename.innerText = file.name;
    statusPanel.style.display = 'block';
    statusError.style.display = 'none';
    statusError.innerText = '';
    resetSteps();

    let uploadFile = file;
    let isOcrNeeded = true;
    const fileExt = file.name.split('.').pop().toLowerCase();

    if (fileExt === 'docx') {
        updateStep('step-upload', 'active');
        try {
            if (!window.mammoth) throw new Error('mammoth.js not loaded');
            const arrayBuffer = await readFileAsArrayBuffer(file);
            const result = await window.mammoth.extractRawText({ arrayBuffer });
            if (!result.value || result.value.trim().length === 0) {
                throw new Error('No text extracted from DOCX');
            }
            const blob = new Blob([result.value], { type: 'text/plain;charset=utf-8' });
            uploadFile = new File([blob], file.name + '.txt', { type: 'text/plain' });
            isOcrNeeded = false;
            updateStep('step-upload', 'completed');
        } catch (err) {
            statusError.innerText = `Local DOCX parsing error: ${err.message}`;
            statusError.style.display = 'block';
            updateStep('step-upload', 'failed');
            ['step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach((id) => updateStep(id, 'failed'));
            return;
        }
    } else if (fileExt === 'txt' || fileExt === 'md') {
        isOcrNeeded = false;
    }

    updateStep('step-upload', 'active');

    const formData = new FormData();
    formData.append('file', uploadFile);

    let ocrTimer;
    let chunkTimer;
    let embedTimer;
    let dbTimer;

    if (isOcrNeeded) {
        ocrTimer = setTimeout(() => {
            updateStep('step-upload', 'completed');
            updateStep('step-ocr', 'active');
        }, 1500);
        chunkTimer = setTimeout(() => {
            updateStep('step-ocr', 'completed');
            updateStep('step-chunk', 'active');
        }, 4000);
        embedTimer = setTimeout(() => {
            updateStep('step-chunk', 'completed');
            updateStep('step-embedding', 'active');
        }, 6000);
        dbTimer = setTimeout(() => {
            updateStep('step-embedding', 'completed');
            updateStep('step-db', 'active');
        }, 8000);
    } else {
        ocrTimer = setTimeout(() => {
            updateStep('step-upload', 'completed');
            updateStep('step-ocr', 'completed');
            updateStep('step-chunk', 'active');
        }, 500);
        chunkTimer = setTimeout(() => {
            updateStep('step-chunk', 'completed');
            updateStep('step-embedding', 'active');
        }, 1200);
        embedTimer = setTimeout(() => {
            updateStep('step-embedding', 'completed');
            updateStep('step-db', 'active');
        }, 2000);
    }

    try {
        const response = await fetch('/upload', {
            method: 'POST',
            body: formData
        });

        clearTimeout(ocrTimer);
        clearTimeout(chunkTimer);
        clearTimeout(embedTimer);
        clearTimeout(dbTimer);

        let result = null;
        try {
            result = await response.json();
        } catch (_err) {
            result = null;
        }

        if (!response.ok) {
            throw new Error(result && result.error ? result.error : `Error code: ${response.status}`);
        }

        updateStep('step-upload', 'completed');
        updateStep('step-ocr', 'completed');
        updateStep('step-chunk', 'completed');
        updateStep('step-embedding', 'completed');
        updateStep('step-db', 'completed');

        await fetchDocuments();
        if (result && result.ids && result.ids.length > 0) {
            const newDoc = documentList.find((doc) => doc.chunks.some((chunk) => result.ids.includes(chunk.id)));
            if (newDoc) selectDocument(newDoc.doc_uuid);
        }
    } catch (err) {
        clearTimeout(ocrTimer);
        clearTimeout(chunkTimer);
        clearTimeout(embedTimer);
        clearTimeout(dbTimer);
        statusError.innerText = `Document processing error: ${err.message}`;
        statusError.style.display = 'block';
        ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach((stepId) => {
            if (stepId === 'step-ocr' && !isOcrNeeded) {
                updateStep(stepId, 'completed');
                return;
            }
            const stepEl = document.getElementById(stepId);
            if (stepEl.classList.contains('active') || !stepEl.classList.contains('completed')) {
                updateStep(stepId, 'failed');
            }
        });
    }
}

function readFileAsArrayBuffer(file) {
    return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = (e) => resolve(e.target.result);
        reader.onerror = () => reject(new Error('Failed to read file'));
        reader.readAsArrayBuffer(file);
    });
}

function updateStep(stepId, status) {
    const stepEl = document.getElementById(stepId);
    const iconEl = stepEl.querySelector('.step-icon');
    stepEl.classList.remove('active', 'completed', 'failed');
    stepEl.classList.add(status);

    if (status === 'active') {
        iconEl.innerHTML = '<i class="fa-solid fa-spinner fa-spin"></i>';
    } else if (status === 'completed') {
        iconEl.innerHTML = '<i class="fa-solid fa-check"></i>';
    } else if (status === 'failed') {
        iconEl.innerHTML = '<i class="fa-solid fa-xmark"></i>';
    } else {
        iconEl.innerHTML = '<i class="fa-solid fa-hourglass-start"></i>';
    }
}

function resetSteps() {
    ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach((stepId) => {
        updateStep(stepId, 'idle');
    });
}

function downloadActiveDocument() {
    if (!selectedDocUuid) return;
    const doc = documentList.find((item) => item.doc_uuid === selectedDocUuid);
    if (!doc) return;

    const blob = new Blob([doc.text], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${doc.filename}.md`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
}

function printActiveDocument() {
    if (!selectedDocUuid) return;
    const doc = documentList.find((item) => item.doc_uuid === selectedDocUuid);
    if (!doc) return;

    const printWindow = window.open('', '_blank', 'width=960,height=720');
    if (!printWindow) {
        showNotice(t('print_blocked_title'), t('print_blocked_message'), 'warning');
        return;
    }

    const renderedHtml = document.getElementById('rendered-content').innerHTML;
    const safeTitle = escapeHtml(doc.filename);
    const safeMeta = escapeHtml(`UUID: ${doc.doc_uuid} | ${t('chunks_label')}: ${doc.chunks.length}`);

    printWindow.document.open();
    printWindow.document.write(`
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>${safeTitle}</title>
    <link rel="stylesheet" href="${window.location.origin}/katex.min.css">
    <style>
        body { font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif; color: #0f172a; margin: 40px; line-height: 1.6; }
        h1 { margin: 0 0 8px 0; font-size: 28px; }
        .meta { color: #475569; font-size: 13px; margin-bottom: 24px; }
        .content { font-size: 15px; }
        pre { white-space: pre-wrap; word-break: break-word; background: #f8fafc; border: 1px solid #e2e8f0; border-radius: 12px; padding: 16px; }
        code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
        img, svg { max-width: 100%; }
        @page { margin: 16mm; }
    </style>
</head>
<body>
    <h1>${safeTitle}</h1>
    <div class="meta">${safeMeta}</div>
    <div class="content">${renderedHtml}</div>
</body>
</html>`);
    printWindow.document.close();
    printWindow.focus();
    setTimeout(() => {
        printWindow.print();
    }, 250);
}

async function deleteActiveDocument() {
    if (!selectedDocUuid) return;
    const doc = documentList.find((item) => item.doc_uuid === selectedDocUuid);
    if (!doc) return;

    const confirmed = await askConfirmation(
        t('delete_document_title'),
        t('delete_document_message', { filename: doc.filename }),
        t('delete_confirm'),
        t('dialog_cancel'),
        'danger'
    );
    if (!confirmed) return;

    try {
        await fetchJson(`/documents/${doc.id}`, { method: 'DELETE' });
        resetViewerState();
        await fetchDocuments();
    } catch (err) {
        await showNotice(t('delete_failed_title'), t('delete_failed_message', { message: err.message }), 'danger');
    }
}

async function clearLibrary() {
    const confirmed = await askConfirmation(
        t('clear_confirm_title'),
        t('clear_confirm_message'),
        t('clear_confirm_button'),
        t('dialog_cancel'),
        'warning'
    );
    if (!confirmed) return;

    try {
        await fetchJson('/clear', { method: 'POST' });
        resetViewerState();
        await fetchDocuments();
        await showNotice(t('clear_success_title'), t('clear_success_message'), 'success');
    } catch (err) {
        await showNotice(t('clear_failed_title'), t('clear_failed_message', { message: err.message }), 'danger');
    }
}

async function openSettingsModal() {
    try {
        const data = await fetchJson('/settings');
        renderSettings(data.settings || [], data.collection_health || null);
        document.getElementById('settings-modal').style.display = 'flex';
    } catch (err) {
        await showNotice(t('settings_failed_title'), t('settings_failed_message', { message: err.message }), 'danger');
    }
}

function closeSettingsModal() {
    document.getElementById('settings-modal').style.display = 'none';
}

function renderSettings(settings, health) {
    const form = document.getElementById('settings-form');
    const healthEl = document.getElementById('settings-health');
    form.innerHTML = '';

    const localCard = document.createElement('div');
    localCard.className = 'settings-local-card';
    localCard.innerHTML = `
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
    form.appendChild(localCard);

    if (health) {
        healthEl.className = 'settings-health' + (health.exists && !health.dimension_matches ? ' is-warning' : '');
        healthEl.innerHTML = health.exists
            ? `<strong>${escapeHtml(t('qdrant_collection'))}</strong> ${escapeHtml(health.collection_name)}<br><strong>${escapeHtml(t('configured_dim'))}</strong> ${health.configured_dim}<br><strong>${escapeHtml(t('actual_dim'))}</strong> ${health.actual_dim ?? t('unknown')}`
            : `<strong>${escapeHtml(t('collection_not_found'))}</strong>`;
    }

    settings.forEach((setting) => {
        const wrapper = document.createElement('div');
        const inputId = `setting-${escapeHtml(setting.key)}`;
        const isSecret = setting.key === 'MISTRAL_API_KEY';
        const inputType = isSecret ? 'password' : 'text';
        wrapper.className = 'settings-field';
        wrapper.innerHTML = `
            <label for="${inputId}">${escapeHtml(setting.label)}</label>
            <div class="settings-input-row">
                <input type="${inputType}" id="${inputId}" data-key="${escapeHtml(setting.key)}" value="${escapeHtmlAttr(setting.value || '')}">
                ${isSecret ? `<button type="button" class="secret-toggle-btn" data-secret-toggle="${inputId}" aria-label="Показать ключ"><i class="fa-solid fa-eye"></i></button>` : ''}
            </div>
            <small>${escapeHtml(setting.description)}</small>
        `;
        form.appendChild(wrapper);
    });

    const languageSelect = document.getElementById('ui-language-select');
    const themeSelect = document.getElementById('ui-theme-select');
    if (languageSelect) {
        languageSelect.value = uiPrefs.language;
        languageSelect.addEventListener('change', () => {
            saveUiPref('language', languageSelect.value);
            applyTranslations();
            renderSettings(settings, health);
        });
    }
    if (themeSelect) {
        themeSelect.value = uiPrefs.theme;
        themeSelect.addEventListener('change', () => {
            saveUiPref('theme', themeSelect.value);
            applyTheme();
            renderSettings(settings, health);
        });
    }
}

async function saveSettings(andRestart) {
    const values = {};
    document.querySelectorAll('#settings-form input[data-key]').forEach((input) => {
        values[input.dataset.key] = input.value;
    });

    try {
        await fetchJson('/settings', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ values })
        });

        if (!andRestart) {
            await showNotice(t('settings_saved_title'), t('settings_saved_message'), 'success');
            return;
        }

        closeSettingsModal();
        showReconnectOverlay(
            t('restarting_title'),
            t('restarting_message')
        );
        await fetchJson('/settings/restart', { method: 'POST' });
        await waitForReconnect();
    } catch (err) {
        hideReconnectOverlay();
        await showNotice(t('save_failed_title'), t('save_failed_message', { message: err.message }), 'danger');
    }
}

async function reindexLibrary() {
    const confirmed = await askConfirmation(
        t('reindex_title'),
        t('reindex_desc'),
        t('reindex_btn_text'),
        t('dialog_cancel'),
        'warning'
    );
    if (!confirmed) {
        return;
    }

    try {
        showReconnectOverlay(
            t('reindex_progress_title'),
            t('reindex_progress_message')
        );
        await fetchJson('/reindex', { method: 'POST' });
        await fetchDocuments();
        hideReconnectOverlay();
        closeSettingsModal();
        await showNotice(t('reindex_success_title'), t('reindex_success_message'), 'success');
    } catch (err) {
        hideReconnectOverlay();
        await showNotice(t('reindex_failed_title'), t('reindex_failed_message', { message: err.message }), 'danger');
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
    if (!confirmed) {
        return;
    }

    try {
        showReconnectOverlay(
            t('hard_reset_progress_title'),
            t('hard_reset_progress_message')
        );
        await fetchJson('/hard-reset', { method: 'POST' });
        resetViewerState();
        await fetchDocuments();
        hideReconnectOverlay();
        closeSettingsModal();
        await showNotice(t('hard_reset_success_title'), t('hard_reset_success_message'), 'success');
    } catch (err) {
        hideReconnectOverlay();
        await showNotice(t('hard_reset_failed_title'), t('hard_reset_failed_message', { message: err.message }), 'danger');
    }
}

function showReconnectOverlay(title, message) {
    document.getElementById('reconnect-title').innerText = title;
    document.getElementById('reconnect-message').innerText = message;
    document.getElementById('reconnect-overlay').style.display = 'flex';
}

function hideReconnectOverlay() {
    document.getElementById('reconnect-overlay').style.display = 'none';
}

async function waitForReconnect() {
    const startedAt = Date.now();
    while (Date.now() - startedAt < 120000) {
        try {
            await fetchJson('/documents');
            hideReconnectOverlay();
            window.location.reload();
            return;
        } catch (_err) {
            await new Promise((resolve) => setTimeout(resolve, 1500));
        }
    }

    hideReconnectOverlay();
    await showNotice(t('reconnect_timeout_title'), t('reconnect_timeout_message'), 'warning');
}

function escapeHtml(value) {
    return String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

function escapeHtmlAttr(value) {
    return escapeHtml(value).replace(/\n/g, '&#10;');
}

