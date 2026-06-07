import { state, setSelectedDocUuid } from './state.js';
import {
    askConfirmation,
    buildLocalizedWarning,
    escapeHtml,
    fetchJson,
    renderWarningBanner,
    showNotice,
    t,
} from './core.js';

let graphAnimationId = null;
let activeDocumentRequestId = 0;
const ENTITY_RENDER_LIMIT = 200;
const DISPLAY_LEVEL_ORDER = ['macro', 'medium', 'micro'];
const UPLOAD_STEPS = ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'];
const STEP_ICONS = {
    pending: 'fa-circle',
    active: 'fa-spinner fa-spin',
    completed: 'fa-check',
    failed: 'fa-xmark',
};

export function initLibrary() {
    document.getElementById('library-search').addEventListener('input', (event) => filterLibrary(event.target.value));
    document.getElementById('print-doc-btn').addEventListener('click', printActiveDocument);
    document.getElementById('download-doc-btn').addEventListener('click', downloadActiveDocument);
    document.getElementById('delete-doc-btn').addEventListener('click', deleteActiveDocument);
    document.getElementById('clear-db-btn').addEventListener('click', clearLibrary);
    document.getElementById('upload-document-btn').addEventListener('click', () => document.getElementById('file-input').click());

    const miniPreview = document.getElementById('document-mini-preview');
    if (miniPreview) miniPreview.addEventListener('click', openDocPreviewModal);

    const previewModal = document.getElementById('doc-preview-modal');
    previewModal.addEventListener('click', (event) => {
        if (event.target === previewModal) closeDocPreviewModal();
    });
    document.getElementById('doc-modal-close-btn').addEventListener('click', closeDocPreviewModal);
    document.getElementById('doc-modal-search').addEventListener('input', (event) => rerenderPreviewModal(event.target.value));

    // Wire up tab buttons
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.addEventListener('click', (event) => {
            const tabName = event.target.getAttribute('data-tab');
            switchTab(tabName);
        });
    });

    // Wire up rebuild graph button
    const rebuildBtn = document.getElementById('btn-rebuild-graph');
    if (rebuildBtn) {
        rebuildBtn.addEventListener('click', () => {
            if (state.selectedDocUuid) {
                renderDocumentGraph(null, state.selectedDocUuid);
            }
        });
    }

    // Wire up fullscreen graph button
    const fullscreenBtn = document.getElementById('btn-fullscreen-graph');
    if (fullscreenBtn) {
        fullscreenBtn.addEventListener('click', () => {
            if (state.selectedDocUuid) {
                openGraphFullscreen();
            }
        });
    }

    // Wire up auto-link buttons
    const autoLinkBtn = document.getElementById('btn-auto-link-graph');
    if (autoLinkBtn) {
        autoLinkBtn.addEventListener('click', runAutoLink);
    }
    const autoLinkControlsBtn = document.getElementById('btn-auto-link-graph-controls');
    if (autoLinkControlsBtn) {
        autoLinkControlsBtn.addEventListener('click', runAutoLink);
    }
    document.querySelectorAll('.graph-modal-auto-link-btn').forEach(btn => {
        btn.addEventListener('click', runAutoLink);
    });

    // Wire up graph modal close
    const graphModalCloseBtn = document.getElementById('graph-modal-close-btn');
    if (graphModalCloseBtn) {
        graphModalCloseBtn.addEventListener('click', closeGraphFullscreen);
    }
    const graphModal = document.getElementById('graph-modal');
    if (graphModal) {
        const closeOnBg = (event) => {
            if (event.target === graphModal) closeGraphFullscreen();
        };
        graphModal.addEventListener('click', closeOnBg);
        // Store reference so applyTranslations doesn't reregister
        graphModal._closeOnBg = closeOnBg;
    }

    initUpload();
}

export function getGroupedDocuments(documents) {
    return (documents || []).map((doc) => ({
        id: doc.id,
        doc_uuid: doc.doc_uuid || `legacy-${doc.id}`,
        filename: doc.filename || 'Unknown document',
        chunk_count: Number(doc.chunk_count || 0),
        char_count: Number(doc.char_count || 0),
        approx_tokens: Number(doc.approx_tokens || Math.round(Number(doc.char_count || 0) / 4)),
        preview_text: doc.preview_text || '',
        text: doc.preview_text || '',
    }));
}

export async function fetchDocuments() {
    try {
        const data = await fetchJson('/documents');
        state.rawChunks = [];
        state.collectionHealth = data.collection_health || null;
        state.documentList = getGroupedDocuments(data.documents);
        renderWarningBanner(buildLocalizedWarning(state.collectionHealth) || data.warning || null);
        renderDocumentList(state.documentList);

        if (state.selectedDocUuid) {
            const exists = state.documentList.some((doc) => doc.doc_uuid === state.selectedDocUuid);
            if (exists) {
                selectDocument(state.selectedDocUuid);
            } else {
                resetViewerState();
            }
        } else {
            resetViewerState();
        }
    } catch (error) {
        console.error('Failed to fetch documents:', error);
        renderWarningBanner(`Failed to load the library: ${error.message}`);
    }
}

export function renderDocumentList(docs) {
    const list = document.getElementById('document-list');
    list.innerHTML = '';

    if (!docs.length) {
        list.innerHTML = `<li class="document-item" style="cursor: default; text-align: center; color: var(--text-secondary);">${escapeHtml(t('library_empty'))}</li>`;
        return;
    }

    docs.forEach((doc) => {
        const item = document.createElement('li');
        item.className = 'document-item';
        if (doc.doc_uuid === state.selectedDocUuid) item.classList.add('active');
        item.innerHTML = `
            <div class="document-item-title" title="${escapeHtml(doc.filename)}">${escapeHtml(doc.filename)}</div>
            <div class="document-item-meta">
                <span style="font-family: ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">UUID: ${escapeHtml(doc.doc_uuid)}</span>
                <span>${escapeHtml(t('chunks_label'))}: ${doc.chunk_count}</span>
            </div>
        `;
        item.addEventListener('click', () => selectDocument(doc.doc_uuid));
        list.appendChild(item);
    });
}

export function filterLibrary(query) {
    if (!query) {
        renderDocumentList(state.documentList);
        return;
    }

    const lower = query.toLowerCase();
    const filtered = state.documentList.filter((doc) =>
        doc.filename.toLowerCase().includes(lower) ||
        doc.doc_uuid.toLowerCase().includes(lower) ||
        (doc.preview_text || '').toLowerCase().includes(lower)
    );
    renderDocumentList(filtered);
}

export function resetViewerState() {
    cleanupGraphRenderer();
    setSelectedDocUuid(null);
    state.selectedDocDetails = null;
    document.getElementById('viewer-section').style.display = 'none';
    document.getElementById('empty-state').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'flex';
    switchTab('content');
    renderDocumentList(state.documentList);
}

export async function selectDocument(docUuid) {
    const doc = state.documentList.find((item) => item.doc_uuid === docUuid);
    if (!doc) return;
    const requestId = ++activeDocumentRequestId;

    setSelectedDocUuid(docUuid);
    state.selectedDocDetails = null;
    document.getElementById('empty-state').style.display = 'none';
    document.getElementById('viewer-section').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'none';
    document.getElementById('viewer-title').textContent = doc.filename;

    const charCount = doc.char_count || doc.text.length;
    const tokenEstimate = doc.approx_tokens || Math.round(charCount / 4);
    document.getElementById('viewer-meta').textContent =
        `${t('chunks_label')}: ${doc.chunk_count} | ${t('length_label')}: ${charCount} ${t('chars_label')} | ${t('tokens_label')}: ${tokenEstimate} | UUID: ${doc.doc_uuid}`;

    updateMiniPreview(doc);
    renderDocumentList(state.documentList);
    renderActiveTab();

    // Fetch details in background/async
    try {
        const details = await fetchJson(`/documents/${docUuid}/details`);
        if (requestId !== activeDocumentRequestId || state.selectedDocUuid !== docUuid) {
            return;
        }
        details.render_revision = requestId;
        state.selectedDocDetails = details;
        hydrateDocumentFromDetails(doc, details);
    } catch (error) {
        console.error('Failed to fetch document details:', error);
        if (requestId !== activeDocumentRequestId || state.selectedDocUuid !== docUuid) {
            return;
        }
        state.selectedDocDetails = { chunks: [], anchors: [], edges: [] };
    }

    updateViewerMeta(doc);
    updateMiniPreview(doc);
    renderActiveTab();
}

function hydrateDocumentFromDetails(doc, details) {
    const chunks = Array.isArray(details?.chunks) ? [...details.chunks] : [];
    chunks.sort((a, b) => (a.chunk_index ?? 0) - (b.chunk_index ?? 0));
    doc.text = buildDocumentDisplayText(chunks);
    doc.chunk_count = chunks.length;
    doc.char_count = doc.text.length;
    doc.approx_tokens = Math.round(doc.char_count / 4);
    doc.view_revision = (doc.view_revision || 0) + 1;
}

function buildDocumentDisplayText(chunks) {
    if (!Array.isArray(chunks) || chunks.length === 0) return '';

    const displayLevel = DISPLAY_LEVEL_ORDER.find((level) =>
        chunks.some((chunk) => (chunk.chunk_level || '').toLowerCase() === level)
    );
    const displayChunks = (displayLevel
        ? chunks.filter((chunk) => (chunk.chunk_level || '').toLowerCase() === displayLevel)
        : chunks
    ).sort((a, b) => (a.chunk_index ?? 0) - (b.chunk_index ?? 0));

    return joinDisplayChunks(displayChunks);
}

function joinDisplayChunks(chunks) {
    const parts = [];
    for (const chunk of chunks) {
        const text = (chunk.text || '').trim();
        if (!text) continue;

        if (parts.length === 0) {
            parts.push(text);
            continue;
        }

        const previous = parts[parts.length - 1];
        parts.push(trimDisplayOverlap(previous, text));
    }

    return parts.filter(Boolean).join('\n\n');
}

function trimDisplayOverlap(previous, current) {
    const maxOverlap = Math.min(previous.length, current.length, 6000);
    for (let length = maxOverlap; length >= 80; length -= 1) {
        if (previous.endsWith(current.slice(0, length))) {
            return current.slice(length).trimStart();
        }
    }
    return current;
}

function updateViewerMeta(doc) {
    const charCount = doc.char_count || doc.text.length;
    const tokenEstimate = doc.approx_tokens || Math.round(charCount / 4);
    document.getElementById('viewer-meta').textContent =
        `${t('chunks_label')}: ${doc.chunk_count} | ${t('length_label')}: ${charCount} ${t('chars_label')} | ${t('tokens_label')}: ${tokenEstimate} | UUID: ${doc.doc_uuid}`;
}

function renderMarkdownInto(container, text) {
    const renderer = new marked.Renderer();
    renderer.code = function(code, language) {
        if (language === 'mermaid') return `<pre class="mermaid">${code}</pre>`;
        return `<pre><code>${code}</code></pre>`;
    };
    marked.setOptions({ renderer });

    let parsed = marked.parse(text);
    parsed = renderMath(parsed);
    container.innerHTML = parsed;
    try {
        if (window.mermaid) {
            window.mermaid.init(undefined, container.querySelectorAll('.mermaid'));
        }
    } catch (error) {
        console.error('Mermaid render failed:', error);
    }
}

function renderMath(text) {
    let html = text.replace(/\$\$([\s\S]+?)\$\$/g, (match, math) => {
        try {
            return `<div class="katex-block" style="margin: 16px 0; text-align: center;">${window.katex.renderToString(math, { displayMode: true, throwOnError: false })}</div>`;
        } catch (_error) {
            return match;
        }
    });

    html = html.replace(/\$([^\$\n]+?)\$/g, (match, math) => {
        try {
            return `<span class="katex-inline">${window.katex.renderToString(math, { displayMode: false, throwOnError: false })}</span>`;
        } catch (_error) {
            return match;
        }
    });

    return html;
}

function updateMiniPreview(doc) {
    document.getElementById('mini-doc-ext').textContent = (doc.filename.split('.').pop() || 'txt').toUpperCase();
    document.getElementById('mini-doc-name').textContent = doc.filename;
    document.getElementById('mini-doc-body').textContent = doc.text.replace(/[#*`_~\[\]]/g, '').slice(0, 800);
}

export function openDocPreviewModal() {
    if (!state.selectedDocUuid) return;
    const doc = state.documentList.find((item) => item.doc_uuid === state.selectedDocUuid);
    if (!doc) return;

    state.currentPreviewDoc = doc;
    document.getElementById('doc-modal-search').value = '';
    document.getElementById('doc-modal-search-count').textContent = '';
    document.getElementById('doc-modal-title').textContent = doc.filename;
    document.getElementById('doc-modal-meta').textContent =
        `${t('chunks_label')}: ${doc.chunk_count} | ${t('length_label')}: ${doc.char_count || doc.text.length} ${t('chars_label')} | UUID: ${doc.doc_uuid}`;

    rerenderPreviewModal('');
    const modal = document.getElementById('doc-preview-modal');
    const content = modal.querySelector('.doc-modal-content');
    modal.style.display = 'flex';
    content.offsetHeight;
    content.style.transform = 'scale(1)';
    content.style.opacity = '1';
}

function rerenderPreviewModal(query) {
    if (!state.currentPreviewDoc) return;
    const container = document.getElementById('doc-modal-content');
    renderMarkdownInto(container, state.currentPreviewDoc.text);
    highlightSearchTerms(container, query);
}

function closeDocPreviewModal() {
    const modal = document.getElementById('doc-preview-modal');
    const content = modal.querySelector('.doc-modal-content');
    content.style.transform = 'scale(0.9)';
    content.style.opacity = '0';
    setTimeout(() => {
        modal.style.display = 'none';
        state.currentPreviewDoc = null;
    }, 300);
}

function highlightSearchTerms(container, query) {
    removeHighlights(container);
    if (!query || !query.trim()) {
        document.getElementById('doc-modal-search-count').textContent = '';
        return;
    }

    const pattern = query.trim().replace(/[-\/\\^$*+?.()|[\]{}]/g, '\\$&');
    const regex = new RegExp(`(${pattern})`, 'gi');
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
            for (let index = node.childNodes.length - 1; index >= 0; index -= 1) {
                walk(node.childNodes[index]);
            }
        }
    }

    walk(container);
    document.getElementById('doc-modal-search-count').textContent = `${count} ${t('matches_label')}`;
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
    ['dragenter', 'dragover'].forEach((eventName) => {
        dropZone.addEventListener(eventName, () => dropZone.classList.add('dragover'), false);
    });
    ['dragleave', 'drop'].forEach((eventName) => {
        dropZone.addEventListener(eventName, () => dropZone.classList.remove('dragover'), false);
    });

    dropZone.addEventListener('drop', (event) => {
        const files = event.dataTransfer.files;
        if (files.length > 0) handleFilesUpload(files[0]);
    });
    fileInput.addEventListener('change', (event) => {
        if (event.target.files.length > 0) handleFilesUpload(event.target.files[0]);
    });
}

function preventDefaults(event) {
    event.preventDefault();
    event.stopPropagation();
}

async function handleFilesUpload(file) {
    const statusPanel = document.getElementById('status-panel');
    const statusFilename = document.getElementById('status-filename');
    const statusError = document.getElementById('status-error');
    const uploadId = createUploadId();
    let progressSource = null;
    statusFilename.textContent = file.name;
    statusError.textContent = '';
    statusError.style.display = 'none';
    statusPanel.style.display = 'flex';
    resetSteps();
    updateStep('step-upload', 'active');
    progressSource = subscribeUploadProgress(uploadId);

    const formData = new FormData();
    formData.append('file', file);

    try {
        await (progressSource?.ready || Promise.resolve());
        const result = await fetchJson(`/upload?upload_id=${encodeURIComponent(uploadId)}`, {
            method: 'POST',
            body: formData,
        });

        progressSource?.close();
        completePendingUploadSteps();
        await fetchDocuments();

        const newDoc = state.documentList.find((doc) => doc.doc_uuid === result.doc_uuid);
        if (newDoc) selectDocument(newDoc.doc_uuid);
    } catch (error) {
        console.error('Upload failed:', error);
        progressSource?.close();
        statusError.textContent = error.message;
        statusError.style.display = 'block';
        failActiveUploadStep();
    }
}

function createUploadId() {
    if (window.crypto?.randomUUID) return window.crypto.randomUUID();
    return `upload-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function subscribeUploadProgress(uploadId) {
    if (!window.EventSource) return null;

    const source = new EventSource(`/upload/progress/${encodeURIComponent(uploadId)}`);
    let resolved = false;
    const ready = new Promise((resolve) => {
        const finish = () => {
            if (resolved) return;
            resolved = true;
            resolve();
        };
        source.onopen = finish;
        setTimeout(finish, 500);
    });
    source.addEventListener('upload-progress', (event) => {
        try {
            const progress = JSON.parse(event.data);
            if (!progress.step || !progress.status) return;
            updateStep(progress.step, progress.status, progress.message, progress.current, progress.total);
        } catch (error) {
            console.error('Failed to parse upload progress event:', error);
        }
    });
    source.onerror = () => {
        console.warn('Upload progress stream disconnected; upload request will continue.');
    };
    return {
        ready,
        close: () => source.close(),
    };
}

function updateStep(stepId, status, message = '', current = null, total = null) {
    const element = document.getElementById(stepId);
    if (!element) return;
    const normalized = status || 'pending';
    element.classList.remove('pending', 'active', 'completed', 'failed');
    element.classList.add(normalized);
    if (message) {
        const progressSuffix = Number.isFinite(current) && Number.isFinite(total) && total > 0
            ? ` (${current}/${total})`
            : '';
        element.title = `${message}${progressSuffix}`;
    } else {
        element.removeAttribute('title');
    }
    const iconEl = element.querySelector('.step-icon');
    if (iconEl) {
        iconEl.innerHTML = `<i class="fa-solid ${STEP_ICONS[normalized] || STEP_ICONS.pending}"></i>`;
    }
}

function resetSteps() {
    UPLOAD_STEPS.forEach((stepId) => {
        updateStep(stepId, 'pending');
    });
}

function failActiveUploadStep() {
    const activeStep = UPLOAD_STEPS.find((stepId) =>
        document.getElementById(stepId)?.classList.contains('active')
    );
    updateStep(activeStep || 'step-upload', 'failed');
}

function completePendingUploadSteps() {
    UPLOAD_STEPS.forEach((stepId) => {
        const element = document.getElementById(stepId);
        if (!element || element.classList.contains('failed')) return;
        updateStep(stepId, 'completed');
    });
}

function getSelectedDocument() {
    return state.documentList.find((item) => item.doc_uuid === state.selectedDocUuid);
}

async function deleteActiveDocument() {
    const doc = getSelectedDocument();
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
        await fetchDocuments();
    } catch (error) {
        await showNotice(t('delete_failed_title'), t('delete_failed_message', { message: error.message }), 'danger');
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
        await fetchDocuments();
        await showNotice(t('clear_success_title'), t('clear_success_message'), 'success');
    } catch (error) {
        await showNotice(t('clear_failed_title'), t('clear_failed_message', { message: error.message }), 'danger');
    }
}

function downloadActiveDocument() {
    const doc = getSelectedDocument();
    if (!doc) return;

    const blob = new Blob([doc.text], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = doc.filename.endsWith('.md') ? doc.filename : `${doc.filename}.md`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
}

async function printActiveDocument() {
    const doc = getSelectedDocument();
    if (!doc) return;
    const printWindow = window.open('', '_blank');
    if (!printWindow) {
        await showNotice(t('print_blocked_title'), t('print_blocked_message'), 'warning');
        return;
    }

    const renderedHtml = document.getElementById('rendered-content').innerHTML;
    printWindow.document.open();
    printWindow.document.write(`
        <html>
            <head>
                <title>${doc.filename}</title>
                <style>
                    body { font-family: Georgia, serif; padding: 32px; line-height: 1.6; color: #111827; }
                    pre { white-space: pre-wrap; }
                    img { max-width: 100%; }
                </style>
            </head>
            <body>
                <h1>${doc.filename}</h1>
                ${renderedHtml}
            </body>
        </html>
    `);
    printWindow.document.close();
    printWindow.focus();
    printWindow.print();
}

// Tab Switching & Caching
export function switchTab(tabName) {
    state.activeTab = tabName;

    if (tabName !== 'graph') {
        cleanupGraphRenderer();
    }

    // Toggle active classes on tab buttons
    document.querySelectorAll('.tab-btn').forEach(btn => {
        if (btn.getAttribute('data-tab') === tabName) {
            btn.classList.add('active');
        } else {
            btn.classList.remove('active');
        }
    });

    const tabs = ['content', 'hierarchy', 'entities', 'graph'];
    tabs.forEach(t => {
        const contentEl = document.getElementById(`tab-content-${t}`);
        if (contentEl) {
            contentEl.style.display = t === tabName ? 'flex' : 'none';
        }
    });

    renderActiveTab();
}

export function renderActiveTab() {
    const activeTab = state.activeTab || 'content';
    const details = state.selectedDocDetails;

    if (activeTab === 'content') {
        const doc = state.documentList.find((item) => item.doc_uuid === state.selectedDocUuid);
        if (doc) {
            const container = document.getElementById('rendered-content');
            const renderKey = `${doc.doc_uuid}:${doc.view_revision || 0}:${doc.text.length}`;
            if (container.dataset.renderKey !== renderKey) {
                renderMarkdownInto(container, doc.text);
                container.dataset.renderKey = renderKey;
            }
        }
        return;
    }

    if (!details) {
        return;
    } else if (activeTab === 'hierarchy') {
        const container = document.getElementById('hierarchy-tree-container');
        const renderKey = getDetailsRenderKey(details);
        if (container.dataset.renderKey !== renderKey) {
            renderHierarchyTree(container, details.chunks);
            container.dataset.renderKey = renderKey;
        }
    } else if (activeTab === 'entities') {
        const container = document.getElementById('entities-tags-container');
        const renderKey = getDetailsRenderKey(details);
        if (container.dataset.renderKey !== renderKey) {
            renderEntities(container, details.chunks);
            container.dataset.renderKey = renderKey;
        }
    } else if (activeTab === 'graph') {
        renderDocumentGraph(null, state.selectedDocUuid);
    }
}

function getDetailsRenderKey(details) {
    const chunks = Array.isArray(details?.chunks) ? details.chunks : [];
    const first = chunks[0]?.id ?? chunks[0]?.chunk_index ?? '';
    const last = chunks[chunks.length - 1]?.id ?? chunks[chunks.length - 1]?.chunk_index ?? '';
    return `${details?.doc_uuid || state.selectedDocUuid || ''}:${details?.render_revision || 0}:${chunks.length}:${first}:${last}`;
}

// Tree builder
function buildHierarchyTree(chunks) {
    const macros = [];
    const mediums = [];
    const micros = [];
    const flat = [];
    
    chunks.forEach(c => {
        const lvl = (c.chunk_level || '').toLowerCase();
        if (lvl === 'macro') {
            macros.push({ ...c, children: [] });
        } else if (lvl === 'medium') {
            mediums.push({ ...c, children: [] });
        } else if (lvl === 'micro') {
            micros.push({ ...c });
        } else {
            flat.push(c);
        }
    });

    // Link mediums to macros
    mediums.forEach(med => {
        const parentIndex = med.parent_chunk_index;
        const parentMacro = macros.find(m => m.chunk_index === parentIndex);
        if (parentMacro) {
            parentMacro.children.push(med);
        } else {
            flat.push(med);
        }
    });

    // Link micros to mediums/macros
    micros.forEach(mic => {
        const parentIndex = mic.parent_chunk_index;
        let parentMedium = mediums.find(m => m.chunk_index === parentIndex);
        if (parentMedium) {
            parentMedium.children.push(mic);
        } else {
            const parentMacro = macros.find(m => m.chunk_index === parentIndex);
            if (parentMacro) {
                parentMacro.children.push(mic);
            } else {
                flat.push(mic);
            }
        }
    });

    return { macros, flat };
}

// Tree element creator
function createHierarchyItem(chunk, type) {
    const li = document.createElement('li');
    li.className = `hierarchy-item ${type}-item`;

    const header = document.createElement('div');
    header.className = 'hierarchy-item-header';

    const hasChildren = chunk.children && chunk.children.length > 0;
    if (hasChildren) {
        const chevron = document.createElement('i');
        chevron.className = 'fa-solid fa-chevron-right toggle-icon';
        header.appendChild(chevron);
    } else {
        const bullet = document.createElement('span');
        bullet.className = 'hierarchy-bullet';
        bullet.innerHTML = '&bull;';
        header.appendChild(bullet);
    }

    const badge = document.createElement('span');
    badge.className = `node-badge ${type}-badge`;
    badge.textContent = type.toUpperCase();
    header.appendChild(badge);

    const title = document.createElement('span');
    title.className = 'node-title';
    
    let titleText = '';
    if (Array.isArray(chunk.header_chain) && chunk.header_chain.length > 0) {
        titleText = chunk.header_chain.join(' ➔ ');
    } else {
        titleText = chunk.text.slice(0, 60) + (chunk.text.length > 60 ? '...' : '');
    }
    title.textContent = titleText;
    header.appendChild(title);

    li.appendChild(header);
    let content = null;
    let didBuildContent = false;

    function ensureContent() {
        if (didBuildContent) return content;
        didBuildContent = true;

        content = document.createElement('div');
        content.className = 'hierarchy-item-content';
        content.style.display = 'none';

        const textDiv = document.createElement('div');
        textDiv.className = 'chunk-full-text';
        textDiv.textContent = chunk.text;
        content.appendChild(textDiv);

        const metaFooter = document.createElement('div');
        metaFooter.className = 'chunk-meta-footer';
        const metaParts = [];
        if (chunk.chunk_index !== undefined) metaParts.push(`Index: ${chunk.chunk_index}`);
        if (chunk.zone_keywords && chunk.zone_keywords.length > 0) {
            metaParts.push(`Keywords: ${chunk.zone_keywords.slice(0, 5).join(', ')}`);
        }
        metaFooter.textContent = metaParts.join(' | ');
        content.appendChild(metaFooter);

        if (hasChildren) {
            const subList = document.createElement('ul');
            subList.className = 'hierarchy-sublist';
            chunk.children.forEach(child => {
                const childType = (child.chunk_level || 'medium').toLowerCase();
                const childItem = createHierarchyItem(child, childType);
                subList.appendChild(childItem);
            });
            content.appendChild(subList);
        }

        li.appendChild(content);
        return content;
    }

    header.addEventListener('click', (event) => {
        event.stopPropagation();
        const contentNode = ensureContent();
        const isCollapsed = contentNode.style.display === 'none';
        contentNode.style.display = isCollapsed ? 'block' : 'none';
        
        if (hasChildren) {
            const icon = header.querySelector('.toggle-icon');
            if (icon) {
                icon.className = isCollapsed 
                    ? 'fa-solid fa-chevron-down toggle-icon' 
                    : 'fa-solid fa-chevron-right toggle-icon';
            }
        }
    });

    return li;
}

// Tree renderer
function renderHierarchyTree(container, chunks) {
    container.innerHTML = '';
    if (!chunks || !chunks.length) {
        container.innerHTML = '<div style="padding: 12px; color: var(--text-secondary);">No hierarchy data available.</div>';
        return;
    }

    const { macros, flat } = buildHierarchyTree(chunks);

    if (macros.length === 0 && flat.length === 0) {
        container.innerHTML = '<div style="padding: 12px; color: var(--text-secondary);">No hierarchy data available.</div>';
        return;
    }

    const list = document.createElement('ul');
    list.className = 'hierarchy-list';

    macros.forEach(macro => {
        const item = createHierarchyItem(macro, 'macro');
        list.appendChild(item);
    });

    flat.forEach(item => {
        const li = createHierarchyItem(item, (item.chunk_level || 'flat').toLowerCase());
        list.appendChild(li);
    });

    container.appendChild(list);
}

// Tag Element Builder
function createTagElement(name, count, type) {
    const badge = document.createElement('div');
    badge.className = `tag-badge ${type}-tag-badge`;
    badge.style.display = 'inline-flex';
    badge.style.alignItems = 'center';
    badge.style.gap = '6px';
    badge.style.padding = '4px 10px';
    badge.style.borderRadius = '20px';
    badge.style.fontSize = '12px';
    badge.style.background = type === 'entity' ? 'rgba(139, 92, 246, 0.15)' : 'rgba(16, 185, 129, 0.15)';
    badge.style.border = type === 'entity' ? '1px solid rgba(139, 92, 246, 0.3)' : '1px solid rgba(16, 185, 129, 0.3)';
    badge.style.color = 'var(--text-primary)';
    badge.style.cursor = 'pointer';
    badge.style.transition = 'all 0.2s';
    
    const textSpan = document.createElement('span');
    textSpan.className = 'tag-text';
    textSpan.textContent = name;
    textSpan.title = `Click to filter library search for "${name}"`;
    textSpan.addEventListener('click', (event) => {
        event.stopPropagation();
        const searchInput = document.getElementById('library-search');
        if (searchInput) {
            searchInput.value = name;
            searchInput.dispatchEvent(new Event('input'));
            if (state.uiPrefs.leftSidebarCollapsed) {
                const toggleBtn = document.getElementById('toggle-library-btn');
                if (toggleBtn) toggleBtn.click();
            }
        }
    });
    badge.appendChild(textSpan);

    const countSpan = document.createElement('span');
    countSpan.className = 'tag-count';
    countSpan.style.background = 'rgba(255, 255, 255, 0.1)';
    countSpan.style.padding = '1px 5px';
    countSpan.style.borderRadius = '10px';
    countSpan.style.fontSize = '10px';
    countSpan.style.color = 'var(--text-secondary)';
    countSpan.textContent = count;
    badge.appendChild(countSpan);

    const aiBtn = document.createElement('span');
    aiBtn.className = 'tag-ai-btn';
    aiBtn.style.color = 'rgba(255, 255, 255, 0.4)';
    aiBtn.style.marginLeft = '2px';
    aiBtn.style.cursor = 'pointer';
    aiBtn.style.display = 'inline-flex';
    aiBtn.style.alignItems = 'center';
    aiBtn.innerHTML = '<i class="fa-solid fa-comment-dots"></i>';
    aiBtn.title = `Ask AI Companion about "${name}" in this document`;
    aiBtn.addEventListener('click', (event) => {
        event.stopPropagation();
        if (state.uiPrefs.rightSidebarCollapsed) {
            const toggleBtn = document.getElementById('toggle-ai-btn');
            if (toggleBtn) toggleBtn.click();
        }

        const chatInput = document.getElementById('chat-input');
        if (chatInput) {
            const doc = state.documentList.find((item) => item.doc_uuid === state.selectedDocUuid);
            const docName = doc ? doc.filename : 'this document';
            chatInput.value = `Tell me more about "${name}" in the context of ${docName}.`;
            chatInput.focus();
            const sendBtn = document.getElementById('send-chat-btn');
            if (sendBtn) sendBtn.click();
        }
    });
    
    badge.addEventListener('mouseenter', () => {
        badge.style.transform = 'translateY(-1px)';
        badge.style.boxShadow = '0 2px 6px rgba(0,0,0,0.15)';
        aiBtn.style.color = 'var(--color-primary)';
    });
    badge.addEventListener('mouseleave', () => {
        badge.style.transform = 'translateY(0)';
        badge.style.boxShadow = 'none';
        aiBtn.style.color = 'rgba(255, 255, 255, 0.4)';
    });

    badge.appendChild(aiBtn);
    return badge;
}

// Entities cloud renderer
function renderEntities(container, chunks) {
    const entitiesList = document.getElementById('entities-list');
    const keywordsList = document.getElementById('keywords-list');
    if (!entitiesList || !keywordsList) return;

    entitiesList.innerHTML = '';
    keywordsList.innerHTML = '';

    if (!chunks || !chunks.length) {
        entitiesList.innerHTML = '<div style="color: var(--text-secondary);">No entities extracted.</div>';
        keywordsList.innerHTML = '<div style="color: var(--text-secondary);">No keywords extracted.</div>';
        return;
    }

    const entityCounts = {};
    const keywordCounts = {};

    chunks.forEach(chunk => {
        const ents = Array.isArray(chunk.entities) ? chunk.entities : [];
        ents.forEach(e => {
            const name = (e || '').trim();
            if (name) entityCounts[name] = (entityCounts[name] || 0) + 1;
        });

        const kws = Array.isArray(chunk.zone_keywords) ? chunk.zone_keywords : [];
        kws.forEach(k => {
            const name = (k || '').trim();
            if (name) keywordCounts[name] = (keywordCounts[name] || 0) + 1;
        });
    });

    const sortedEntities = Object.entries(entityCounts)
        .sort((a, b) => b[1] - a[1])
        .slice(0, ENTITY_RENDER_LIMIT);
    const sortedKeywords = Object.entries(keywordCounts)
        .sort((a, b) => b[1] - a[1])
        .slice(0, ENTITY_RENDER_LIMIT);

    if (sortedEntities.length === 0) {
        entitiesList.innerHTML = '<div style="color: var(--text-secondary);">No entities.</div>';
    } else {
        sortedEntities.forEach(([name, count]) => {
            entitiesList.appendChild(createTagElement(name, count, 'entity'));
        });
    }

    if (sortedKeywords.length === 0) {
        keywordsList.innerHTML = '<div style="color: var(--text-secondary);">No keywords.</div>';
    } else {
        sortedKeywords.forEach(([name, count]) => {
            keywordsList.appendChild(createTagElement(name, count, 'keyword'));
        });
    }
}

let currentGraphSim = null;

function cleanupGraphRenderer() {
    if (currentGraphSim) {
        currentGraphSim.stop();
        currentGraphSim = null;
    }
    if (graphAnimationId) {
        cancelAnimationFrame(graphAnimationId);
        graphAnimationId = null;
    }
}

function renderDocumentGraph(container, docUuid) {
    cleanupGraphRenderer();

    const canvas = document.getElementById('graph-canvas');
    const canvasWrapper = document.getElementById('graph-canvas-wrapper');
    const statsEl = document.getElementById('graph-stats');
    const legendEl = document.getElementById('graph-legend');
    const emptyOverlay = document.getElementById('graph-empty-overlay');
    if (!canvas || !canvasWrapper) return;

    const rect = canvasWrapper.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = (rect.width || 600) * dpr;
    canvas.height = (rect.height || 450) * dpr;
    canvas.style.width = (rect.width || 600) + 'px';
    canvas.style.height = (rect.height || 450) + 'px';
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    
    const width = rect.width || 600;
    const height = rect.height || 450;
    
    if (emptyOverlay) emptyOverlay.style.display = 'none';

    const details = state.selectedDocDetails;
    if (!details || !Array.isArray(details.chunks) || details.chunks.length === 0) {
        ctx.fillStyle = '#9ca3af';
        ctx.font = '14px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(state.uiPrefs.language === 'ru' ? 'Загрузка данных документа...' : 'Loading document data...', width / 2, height / 2);
        // Retry when details arrive
        const check = () => {
            if (state.selectedDocDetails && Array.isArray(state.selectedDocDetails.chunks) && state.selectedDocDetails.chunks.length > 0) {
                renderDocumentGraph(container, docUuid);
            } else {
                setTimeout(check, 300);
            }
        };
        setTimeout(check, 300);
        return;
    }

    const graphData = buildDocumentGraphData(details, docUuid);
    const hasEdges = (graphData.edges || []).length > 0;
    if (!hasEdges && emptyOverlay) {
        emptyOverlay.style.display = 'flex';
        ctx.clearRect(0, 0, width, height);
        return;
    }

    currentGraphSim = createGraphSimulation(canvas, graphData, docUuid, {
        statsEl,
        legendEl,
        onNavigate: (nodeId) => {
            selectDocument(nodeId);
        }
    });
    wireGraphZoomButtons(currentGraphSim);
}

function buildDocumentGraphData(details, activeDocUuid) {
    const chunks = details.chunks || [];
    const edges = details.edges || [];
    const doc = state.documentList.find(d => d.doc_uuid === activeDocUuid);
    const docLabel = doc?.filename || 'Document';
    const MAX_ENTITIES = 80;
    
    const nodes = [];
    const edgeSet = [];
    const entityCount = {};
    const chunkHierarchy = {};
    
    // Root node
    nodes.push({
        id: `doc:${activeDocUuid}`,
        label: docLabel,
        type: 'root',
        isActive: true,
        radius: 26,
    });
    
    // Collect entities and count them
    chunks.forEach(ch => {
        const ents = Array.isArray(ch.entities) ? ch.entities : [];
        ents.forEach(e => {
            const name = (e || '').trim();
            if (name) entityCount[name] = (entityCount[name] || 0) + 1;
        });
    });
    
    // Sort entities by frequency and cap
    const topEntities = Object.entries(entityCount)
        .sort((a, b) => b[1] - a[1])
        .slice(0, MAX_ENTITIES)
        .map(([name, count]) => name);
    const entitySet = new Set(topEntities);
    
    // Build nodes for chunks and entities
    chunks.forEach(ch => {
        const idx = ch.chunk_index;
        const level = (ch.chunk_level || 'micro').toLowerCase();
        const chunkId = `chunk:${idx}`;
        
        let radius = 10;
        if (level === 'macro') radius = 16;
        else if (level === 'medium') radius = 13;
        
        const header = Array.isArray(ch.header_chain) ? ch.header_chain : [];
        const label = header.length > 0 ? header[header.length - 1] : `${level} ${idx}`;
        
        nodes.push({
            id: chunkId,
            label: String(label),
            type: level,
            chunk_index: idx,
            parent_chunk_index: ch.parent_chunk_index,
            radius,
            isActive: false,
        });
        
        chunkHierarchy[idx] = { id: chunkId, level, parent: ch.parent_chunk_index };
        
        // Edge: root → macro, parent → child
        if (level === 'macro') {
            edgeSet.push({ source: `doc:${activeDocUuid}`, target: chunkId, type: 'contains' });
        } else if (ch.parent_chunk_index != null) {
            edgeSet.push({ source: `chunk:${ch.parent_chunk_index}`, target: chunkId, type: 'contains' });
        }
        
        // Entity edges
        const ents = Array.isArray(ch.entities) ? ch.entities : [];
        ents.forEach(e => {
            const name = (e || '').trim();
            if (name && entitySet.has(name)) {
                edgeSet.push({ source: chunkId, target: `entity:${name}`, type: 'has_entity' });
            }
        });
    });
    
    // Entity nodes
    topEntities.forEach(name => {
        nodes.push({
            id: `entity:${name}`,
            label: name,
            type: 'entity',
            radius: 7,
            isActive: false,
        });
    });
    
    // Also add inter-document edges from details if they exist
    if (Array.isArray(edges) && edges.length > 0) {
        edges.forEach(e => {
            const otherUuid = e.source === activeDocUuid ? e.target : e.source;
            // Add the other document as a node if not already present
            if (!nodes.find(n => n.id === `doc:${otherUuid}`)) {
                const otherDoc = state.documentList.find(d => d.doc_uuid === otherUuid);
                nodes.push({
                    id: `doc:${otherUuid}`,
                    label: otherDoc?.filename || 'Related Document',
                    type: 'doc',
                    radius: 16,
                    isActive: false,
                });
            }
            edgeSet.push({
                source: `doc:${e.source}`,
                target: `doc:${e.target}`,
                type: e.type || 'shares_entities',
            });
        });
    }
    
    return { nodes, edges: edgeSet };
}

function createGraphSimulation(canvas, data, activeDocUuid, { statsEl, legendEl, onNavigate } = {}) {
    const nodesData = data.nodes || [];
    const edgesData = data.edges || [];
    const canvasWrapper = canvas.parentElement;
    
    function resizeCanvas() {
        const rect = canvasWrapper.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        canvas.width = (rect.width || 600) * dpr;
        canvas.height = (rect.height || 450) * dpr;
        canvas.style.width = (rect.width || 600) + 'px';
        canvas.style.height = (rect.height || 450) + 'px';
        return { width: rect.width || 600, height: rect.height || 450, dpr };
    }
    
    let dims = resizeCanvas();
    const ctx = canvas.getContext('2d');
    
    const tooltipEl = document.createElement('div');
    tooltipEl.className = 'graph-tooltip';
    tooltipEl.style.display = 'none';
    canvasWrapper.appendChild(tooltipEl);
    
    const view = { x: 0, y: 0, scale: 1 };
    
    const nodes = nodesData.map((n, i) => {
        const angle = (i / nodesData.length) * Math.PI * 2;
        const spread = 100 * Math.random();
        return {
            id: n.id,
            label: n.label || 'Unknown',
            type: n.type || 'doc',
            x: dims.width / 2 + Math.cos(angle) * spread,
            y: dims.height / 2 + Math.sin(angle) * spread,
            vx: 0, vy: 0,
            radius: n.radius || 10,
            isActive: n.isActive || false,
            chunk_index: n.chunk_index,
        };
    });
    
    const edges = [];
    edgesData.forEach(e => {
        const sourceNode = nodes.find(n => n.id === e.source);
        const targetNode = nodes.find(n => n.id === e.target);
        if (sourceNode && targetNode) {
            edges.push({
                id: e.id,
                source: sourceNode,
                target: targetNode,
                type: e.type,
            });
        }
    });
    
    function getNodeColor(n) {
        if (n.isActive) return '#8b5cf6';
        if (n.type === 'macro') return '#3b82f6';
        if (n.type === 'medium') return '#06b6d4';
        if (n.type === 'micro') return '#64748b';
        if (n.type === 'entity') return '#f59e0b';
        if (n.type === 'doc') return '#374151';
        return '#374151';
    }
    
    function getNodeStrokeColor(n) {
        if (n.isActive) return '#c084fc';
        if (n.type === 'macro') return '#93c5fd';
        if (n.type === 'medium') return '#67e8f9';
        if (n.type === 'micro') return '#94a3b8';
        if (n.type === 'entity') return '#fbbf24';
        if (n.type === 'doc') return '#6b7280';
        return '#6b7280';
    }
    
    if (statsEl) {
        const byType = {};
        nodes.forEach(n => { byType[n.type] = (byType[n.type] || 0) + 1; });
        const parts = [
            `<span><i class="fa-solid fa-circle-nodes"></i> ${nodes.length}</span>`,
            `<span><i class="fa-solid fa-link"></i> ${edges.length}</span>`,
        ];
        if (byType.macro) parts.push(`<span style="color:#93c5fd;">● ${byType.macro} ${state.uiPrefs.language === 'ru' ? 'разделов' : 'sections'}</span>`);
        if (byType.medium) parts.push(`<span style="color:#67e8f9;">● ${byType.medium} ${state.uiPrefs.language === 'ru' ? 'подразделов' : 'subsections'}</span>`);
        if (byType.entity) parts.push(`<span style="color:#fbbf24;">● ${byType.entity} ${state.uiPrefs.language === 'ru' ? 'сущностей' : 'entities'}</span>`);
        statsEl.innerHTML = parts.join(' ');
    }
    
    if (legendEl) {
        const isRu = state.uiPrefs.language === 'ru';
        legendEl.innerHTML = `
            <span><span class="graph-legend-dot" style="background:#8b5cf6;box-shadow:0 0 6px #8b5cf6;"></span>${isRu ? 'Документ' : 'Document'}</span>
            <span><span class="graph-legend-dot" style="background:#3b82f6;"></span>${isRu ? 'Раздел' : 'Section'}</span>
            <span><span class="graph-legend-dot" style="background:#06b6d4;"></span>${isRu ? 'Подраздел' : 'Subsection'}</span>
            <span><span class="graph-legend-dot" style="background:#f59e0b;"></span>${isRu ? 'Сущность' : 'Entity'}</span>
            <span><span class="graph-legend-line" style="border-color:rgba(59,130,246,0.4);"></span>${isRu ? 'Иерархия' : 'Hierarchy'}</span>
            <span><span style="width:12px;height:0;border-top:2px dashed rgba(245,158,11,0.5);display:inline-block;flex:0 0 auto;"></span>${isRu ? 'Содержит сущность' : 'Has entity'}</span>
        `;
    }
    
    const repulsion = 400;
    const attraction = 0.04;
    const gravity = 0.02;
    const damping = 0.85;
    let alpha = 1.0;
    const alphaDecay = 0.02;
    const alphaMin = 0.001;
    const energyThreshold = 0.5;
    let tickCount = 0;
    let frozen = false;
    
    function warmUp() {
        if (frozen) {
            frozen = false;
            alpha = 0.3;
            tick();
        }
    }
    
    function showChunkContent(node) {
        const chunks = (state.selectedDocDetails && state.selectedDocDetails.chunks) || [];
        const chunk = chunks.find(c => c.chunk_index === node.chunk_index);
        if (!chunk) return;
        
        const text = chunk.text || '';
        const header = Array.isArray(chunk.header_chain) ? chunk.header_chain.join(' > ') : '';
        const entities = Array.isArray(chunk.entities) ? chunk.entities.join(', ') : '';
        
        const existing = canvasWrapper.querySelector('.graph-inspector');
        if (existing) existing.remove();
        
        const inspector = document.createElement('div');
        inspector.className = 'graph-inspector';
        inspector.innerHTML = `
            <div class="graph-inspector-header">
                <span class="graph-inspector-title">${escapeHtml(header || node.label)}</span>
                <button class="graph-inspector-close">&times;</button>
            </div>
            <div class="graph-inspector-body">
                <div class="graph-inspector-meta">
                    <span>${state.uiPrefs.language === 'ru' ? 'Уровень' : 'Level'}: ${node.type}</span>
                    <span>${state.uiPrefs.language === 'ru' ? 'Индекс' : 'Index'}: ${node.chunk_index}</span>
                    ${entities ? `<span>${state.uiPrefs.language === 'ru' ? 'Сущности' : 'Entities'}: ${escapeHtml(entities)}</span>` : ''}
                </div>
                <pre class="graph-inspector-text">${escapeHtml(text)}</pre>
            </div>
        `;
        inspector.querySelector('.graph-inspector-close').onclick = () => inspector.remove();
        inspector.querySelector('.graph-inspector-header').onclick = () => inspector.remove();
        canvasWrapper.appendChild(inspector);
    }
    
    let draggedNode = null;
    let hoveredNode = null;
    let isPanning = false;
    let panStartX = 0, panStartY = 0;
    let mouseDownX = 0, mouseDownY = 0;
    let mouseDownNode = null;
    let mouseMovedSinceDown = false;
    
    function getNodeAt(screenX, screenY) {
        const worldX = (screenX - view.x) / view.scale;
        const worldY = (screenY - view.y) / view.scale;
        return nodes.find(n => {
            const dx = n.x - worldX;
            const dy = n.y - worldY;
            return Math.sqrt(dx*dx + dy*dy) < n.radius + 5;
        });
    }
    
    function getMousePos(event) {
        const rect = canvas.getBoundingClientRect();
        return { x: event.clientX - rect.left, y: event.clientY - rect.top };
    }
    
    function onMouseDown(event) {
        const pos = getMousePos(event);
        mouseDownX = pos.x;
        mouseDownY = pos.y;
        mouseMovedSinceDown = false;
        const node = getNodeAt(pos.x, pos.y);
        mouseDownNode = node;
        
        if (hoveredNode) {
            hoveredNode = null;
            updateTooltip(null);
        }
        
        if (node) {
            draggedNode = node;
            warmUp();
        } else {
            isPanning = true;
            panStartX = pos.x - view.x;
            panStartY = pos.y - view.y;
        }
    }
    
    function updateTooltip(node) {
        if (node) {
            const cssX = node.x * view.scale + view.x;
            const cssY = node.y * view.scale + view.y;
            tooltipEl.style.display = 'block';
            tooltipEl.style.left = Math.min(cssX + 14, dims.width - 220) + 'px';
            tooltipEl.style.top = Math.max(0, cssY - 10) + 'px';
            let info = `<b>${escapeHtml(node.label)}</b>`;
            info += `<br>${state.uiPrefs.language === 'ru' ? 'Тип' : 'Type'}: ${node.type}`;
            if (node.chunk_index !== undefined) {
                info += `, ${state.uiPrefs.language === 'ru' ? 'Индекс' : 'Index'}: ${node.chunk_index}`;
            }
            tooltipEl.innerHTML = info;
        } else {
            tooltipEl.style.display = 'none';
        }
    }
    
    function onMouseMove(event) {
        const pos = getMousePos(event);
        
        if (Math.abs(pos.x - mouseDownX) > 3 || Math.abs(pos.y - mouseDownY) > 3) {
            mouseMovedSinceDown = true;
        }
        
        if (draggedNode) {
            const worldX = (pos.x - view.x) / view.scale;
            const worldY = (pos.y - view.y) / view.scale;
            draggedNode.x = worldX;
            draggedNode.y = worldY;
            draggedNode.vx = 0;
            draggedNode.vy = 0;
        } else if (isPanning) {
            view.x = pos.x - panStartX;
            view.y = pos.y - panStartY;
        } else {
            const prev = hoveredNode;
            hoveredNode = getNodeAt(pos.x, pos.y);
            canvas.style.cursor = hoveredNode ? 'pointer' : 'grab';
            if (hoveredNode !== prev) {
                updateTooltip(hoveredNode);
            }
        }
    }
    
    function onMouseUp() {
        const wasDragged = draggedNode !== null;
        if (draggedNode) draggedNode = null;
        if (isPanning) isPanning = false;
        if (hoveredNode) {
            hoveredNode = null;
            updateTooltip(null);
        }
        
        if (wasDragged) {
            warmUp();
            return;
        }
        
        if (!mouseMovedSinceDown && mouseDownNode) {
            // Navigate to related document
            if ((mouseDownNode.type === 'root' || mouseDownNode.type === 'doc') && mouseDownNode.id !== `doc:${activeDocUuid}`) {
                const docUuid = mouseDownNode.id.replace(/^doc:/, '');
                if (onNavigate) onNavigate(docUuid);
                return;
            }
            // Show chunk content
            if (['macro', 'medium', 'micro'].includes(mouseDownNode.type)) {
                showChunkContent(mouseDownNode);
                return;
            }
            // Search entity
            if (mouseDownNode.type === 'entity') {
                const searchInput = document.getElementById('library-search');
                if (searchInput) {
                    searchInput.value = mouseDownNode.label;
                    searchInput.dispatchEvent(new Event('input'));
                }
                showNotice(
                    state.uiPrefs.language === 'ru' ? 'Поиск сущности' : 'Search entity',
                    state.uiPrefs.language === 'ru' ? `Поиск: ${mouseDownNode.label}` : `Searching: ${mouseDownNode.label}`,
                    'info'
                );
                return;
            }
        }
    }
    
    function onWheel(event) {
        event.preventDefault();
        const pos = getMousePos(event);
        const worldX = (pos.x - view.x) / view.scale;
        const worldY = (pos.y - view.y) / view.scale;
        
        const delta = event.deltaY > 0 ? 1 / 1.15 : 1.15;
        const newScale = Math.max(0.1, Math.min(10, view.scale * delta));
        view.x = pos.x - worldX * newScale;
        view.y = pos.y - worldY * newScale;
        view.scale = newScale;
    }
    
    canvas.addEventListener('mousedown', onMouseDown);
    canvas.addEventListener('mousemove', onMouseMove);
    canvas.addEventListener('mouseup', onMouseUp);
    canvas.addEventListener('mouseleave', onMouseUp);
    canvas.addEventListener('wheel', onWheel, { passive: false });
    
    function onResize() {
        dims = resizeCanvas();
    }
    window.addEventListener('resize', onResize);
    
    function tick() {
        if (frozen) return;
        
        const width = dims.width;
        const height = dims.height;
        const dpr = dims.dpr;
        
        alpha -= alphaDecay;
        if (alpha < 0) alpha = 0;
        
        tickCount++;
        
        for (let i = 0; i < nodes.length; i++) {
            const n1 = nodes[i];
            for (let j = i + 1; j < nodes.length; j++) {
                const n2 = nodes[j];
                const dx = n2.x - n1.x;
                const dy = n2.y - n1.y;
                const distSq = dx*dx + dy*dy || 1;
                const dist = Math.sqrt(distSq);
                
                const force = repulsion / distSq * alpha;
                const fx = (dx / dist) * force;
                const fy = (dy / dist) * force;
                
                if (n1 !== draggedNode) { n1.vx -= fx; n1.vy -= fy; }
                if (n2 !== draggedNode) { n2.vx += fx; n2.vy += fy; }
            }
        }
        
        edges.forEach(edge => {
            const n1 = edge.source;
            const n2 = edge.target;
            const dx = n2.x - n1.x;
            const dy = n2.y - n1.y;
            const dist = Math.sqrt(dx*dx + dy*dy) || 1;
            
            const desiredDist = 120;
            const force = (dist - desiredDist) * attraction * alpha;
            const fx = (dx / dist) * force;
            const fy = (dy / dist) * force;
            
            if (n1 !== draggedNode) { n1.vx += fx; n1.vy += fy; }
            if (n2 !== draggedNode) { n2.vx -= fx; n2.vy -= fy; }
        });
        
        nodes.forEach(n => {
            if (n === draggedNode) return;
            n.vx += (width / 2 - n.x) * gravity * alpha;
            n.vy += (height / 2 - n.y) * gravity * alpha;
        });
        
        let totalEnergy = 0;
        nodes.forEach(n => {
            if (n === draggedNode) return;
            n.vx *= damping;
            n.vy *= damping;
            n.x += n.vx;
            n.y += n.vy;
            totalEnergy += n.vx * n.vx + n.vy * n.vy;
            
            const margin = 200;
            n.x = Math.max(-margin, Math.min(width + margin, n.x));
            n.y = Math.max(-margin, Math.min(height + margin, n.y));
        });
        
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, width, height);
        
        ctx.save();
        ctx.translate(view.x, view.y);
        ctx.scale(view.scale, view.scale);
        
        edges.forEach(edge => {
            ctx.beginPath();
            ctx.moveTo(edge.source.x, edge.source.y);
            ctx.lineTo(edge.target.x, edge.target.y);
            
            if (edge.type === 'shares_entities') {
                ctx.strokeStyle = 'rgba(139, 92, 246, 0.4)';
                ctx.lineWidth = 2;
                ctx.setLineDash([4, 4]);
            } else {
                ctx.strokeStyle = 'rgba(156, 163, 175, 0.4)';
                ctx.lineWidth = 1.5;
                ctx.setLineDash([]);
            }
            ctx.stroke();
            ctx.setLineDash([]);
        });
        
        nodes.forEach(n => {
            ctx.beginPath();
            ctx.arc(n.x, n.y, n.radius, 0, Math.PI * 2);
            
            if (n === hoveredNode) {
                ctx.shadowColor = '#f3f4f6';
                ctx.shadowBlur = 12;
            } else if (n.isActive) {
                ctx.shadowColor = '#8b5cf6';
                ctx.shadowBlur = 15;
            }
            
            ctx.fillStyle = getNodeColor(n);
            ctx.fill();
            ctx.strokeStyle = getNodeStrokeColor(n);
            ctx.lineWidth = n.isActive ? 3 : (n.type === 'entity' ? 1 : 1.5);
            ctx.stroke();
            ctx.shadowBlur = 0;
            
            const fontSize = Math.max(9, Math.min(14, 12 / view.scale));
            ctx.fillStyle = '#f3f4f6';
            ctx.font = (n.isActive ? 'bold ' : '') + fontSize + 'px sans-serif';
            ctx.textAlign = 'center';
            ctx.textBaseline = 'top';
            
            let label = n.label;
            const maxChars = Math.max(8, Math.round(20 / view.scale));
            if (label.length > maxChars) {
                label = label.slice(0, maxChars - 3) + '...';
            }
            
            // Label background for readability
            const metrics = ctx.measureText(label);
            const pad = 3;
            ctx.fillStyle = 'rgba(0,0,0,0.65)';
            const bgX = n.x - metrics.width / 2 - pad;
            const bgY = n.y + n.radius + 4 - pad;
            const bgW = metrics.width + pad * 2;
            const bgH = fontSize + pad * 2;
            ctx.beginPath();
            ctx.roundRect(bgX, bgY, bgW, bgH, 3);
            ctx.fill();
            
            ctx.fillStyle = '#f3f4f6';
            ctx.fillText(label, n.x, n.y + n.radius + 4);
        });
        
        ctx.restore();
        
        if (alpha <= alphaMin || totalEnergy < energyThreshold) {
            frozen = true;
            return;
        }
        
        graphAnimationId = requestAnimationFrame(tick);
    }
    
    tick();
    
    function resetView() {
        if (nodes.length === 0) return;
        let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
        nodes.forEach(n => {
            if (n.x < minX) minX = n.x;
            if (n.x > maxX) maxX = n.x;
            if (n.y < minY) minY = n.y;
            if (n.y > maxY) maxY = n.y;
        });
        
        if (maxX - minX < 10 || maxY - minY < 10) {
            view.x = 0; view.y = 0; view.scale = 1;
            return;
        }
        
        const pad = 80;
        const worldW = (maxX - minX) + pad * 2;
        const worldH = (maxY - minY) + pad * 2;
        const centerX = (minX + maxX) / 2;
        const centerY = (minY + maxY) / 2;
        
        const scaleX = dims.width / worldW;
        const scaleY = dims.height / worldH;
        view.scale = Math.min(1, Math.min(scaleX, scaleY));
        view.x = dims.width / 2 - centerX * view.scale;
        view.y = dims.height / 2 - centerY * view.scale;
    }
    
    function zoomIn() {
        const cx = dims.width / 2;
        const cy = dims.height / 2;
        const worldX = (cx - view.x) / view.scale;
        const worldY = (cy - view.y) / view.scale;
        view.scale = Math.min(10, view.scale * 1.3);
        view.x = cx - worldX * view.scale;
        view.y = cy - worldY * view.scale;
    }
    
    function zoomOut() {
        const cx = dims.width / 2;
        const cy = dims.height / 2;
        const worldX = (cx - view.x) / view.scale;
        const worldY = (cy - view.y) / view.scale;
        view.scale = Math.max(0.1, view.scale * 0.7);
        view.x = cx - worldX * view.scale;
        view.y = cy - worldY * view.scale;
    }
    
    setTimeout(resetView, 600);
    
    return {
        stop() {
            if (graphAnimationId) {
                cancelAnimationFrame(graphAnimationId);
                graphAnimationId = null;
            }
            canvas.removeEventListener('mousedown', onMouseDown);
            canvas.removeEventListener('mousemove', onMouseMove);
            canvas.removeEventListener('mouseup', onMouseUp);
            canvas.removeEventListener('mouseleave', onMouseUp);
            canvas.removeEventListener('wheel', onWheel);
            window.removeEventListener('resize', onResize);
        },
        resetView,
        zoomIn,
        zoomOut,
    };
}

function wireGraphZoomButtons(sim) {
    document.querySelectorAll('.graph-zoom-btn').forEach(btn => {
        const zoomAction = btn.getAttribute('data-zoom');
        btn.onclick = () => {
            if (zoomAction === 'in') sim.zoomIn();
            else if (zoomAction === 'out') sim.zoomOut();
            else if (zoomAction === 'fit') sim.resetView();
        };
    });
}

function openGraphFullscreen() {
    const modal = document.getElementById('graph-modal');
    if (!modal || !state.selectedDocUuid) return;
    
    const existingSim = modal._graphSim;
    if (existingSim) {
        existingSim.stop();
        modal._graphSim = null;
    }
    
    modal.style.display = 'flex';
    
    const canvas = document.getElementById('graph-modal-canvas');
    const container = document.getElementById('graph-modal-container');
    const statsEl = document.getElementById('graph-modal-stats');
    const legendEl = document.getElementById('graph-modal-legend');
    const emptyOverlay = document.getElementById('graph-modal-empty-overlay');
    if (!canvas || !container) return;
    
    if (emptyOverlay) emptyOverlay.style.display = 'none';
    
    const rect = container.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = (rect.width || 800) * dpr;
    canvas.height = (rect.height || 600) * dpr;
    canvas.style.width = (rect.width || 800) + 'px';
    canvas.style.height = (rect.height || 600) + 'px';
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    
    const width = rect.width || 800;
    const height = rect.height || 600;
    
    const details = state.selectedDocDetails;
    if (!details || !Array.isArray(details.chunks) || details.chunks.length === 0) {
        ctx.fillStyle = '#9ca3af';
        ctx.font = '14px sans-serif';
        ctx.textAlign = 'center';
        ctx.fillText(state.uiPrefs.language === 'ru' ? 'Загрузка данных документа...' : 'Loading document data...', width / 2, height / 2);
        const check = () => {
            if (state.selectedDocDetails && Array.isArray(state.selectedDocDetails.chunks) && state.selectedDocDetails.chunks.length > 0) {
                openGraphFullscreen();
            } else {
                setTimeout(check, 300);
            }
        };
        setTimeout(check, 300);
        return;
    }
    
    const graphData = buildDocumentGraphData(details, state.selectedDocUuid);
    const hasEdges = (graphData.edges || []).length > 0;
    if (!hasEdges && emptyOverlay) {
        emptyOverlay.style.display = 'flex';
        ctx.clearRect(0, 0, width, height);
        return;
    }
    
    const sim = createGraphSimulation(canvas, graphData, state.selectedDocUuid, {
        statsEl,
        legendEl,
        onNavigate: (nodeId) => {
            closeGraphFullscreen();
            selectDocument(nodeId);
        }
    });
    modal._graphSim = sim;
    wireGraphZoomButtons(sim);
}

function closeGraphFullscreen() {
    const modal = document.getElementById('graph-modal');
    if (!modal) return;
    const sim = modal._graphSim;
    if (sim) {
        sim.stop();
        modal._graphSim = null;
    }
    modal.style.display = 'none';
}

async function runAutoLink() {
    const btn = document.querySelector('#btn-auto-link-graph, #btn-auto-link-graph-controls, .graph-modal-auto-link-btn');
    if (btn) btn.disabled = true;
    
    try {
        const result = await fetchJson('/documents/auto-link?threshold=3', { method: 'POST' });
        const count = result.edges_created || 0;
        if (count > 0) {
            showNotice(
                t('graph_auto_link_complete_title'),
                t('graph_auto_link_complete_msg', { count: String(count) }),
                'success'
            );
            if (state.selectedDocUuid) {
                setTimeout(() => renderDocumentGraph(null, state.selectedDocUuid), 500);
            }
        } else {
            showNotice(
                t('graph_auto_link_none_title'),
                t('graph_auto_link_none_msg'),
                'info'
            );
        }
    } catch (err) {
        console.error("Auto-link failed:", err);
        showNotice(
            t('graph_auto_link_failed_title'),
            err.message || String(err),
            'danger'
        );
    } finally {
        if (btn) btn.disabled = false;
    }
}
