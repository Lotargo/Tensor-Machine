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

    initUpload();
}

export function getGroupedDocuments(chunks) {
    const grouped = {};
    chunks.forEach((chunk) => {
        const uuid = chunk.doc_uuid || `legacy-${chunk.id}`;
        if (!grouped[uuid]) {
            grouped[uuid] = {
                doc_uuid: uuid,
                filename: chunk.filename || 'Legacy Chunk',
                chunks: [],
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

export async function fetchDocuments() {
    try {
        const data = await fetchJson('/documents');
        state.rawChunks = data.documents || [];
        state.collectionHealth = data.collection_health || null;
        state.documentList = getGroupedDocuments(state.rawChunks);
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
                <span>${escapeHtml(t('chunks_label'))}: ${doc.chunks.length}</span>
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
        doc.text.toLowerCase().includes(lower)
    );
    renderDocumentList(filtered);
}

export function resetViewerState() {
    setSelectedDocUuid(null);
    document.getElementById('viewer-section').style.display = 'none';
    document.getElementById('empty-state').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'flex';
    renderDocumentList(state.documentList);
}

export function selectDocument(docUuid) {
    const doc = state.documentList.find((item) => item.doc_uuid === docUuid);
    if (!doc) return;

    setSelectedDocUuid(docUuid);
    document.getElementById('empty-state').style.display = 'none';
    document.getElementById('viewer-section').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'none';
    document.getElementById('viewer-title').textContent = doc.filename;

    const charCount = doc.text.length;
    const tokenEstimate = Math.round(charCount / 4);
    document.getElementById('viewer-meta').textContent =
        `${t('chunks_label')}: ${doc.chunks.length} | ${t('length_label')}: ${charCount} ${t('chars_label')} | ${t('tokens_label')}: ${tokenEstimate} | UUID: ${doc.doc_uuid}`;

    renderMarkdownInto(document.getElementById('rendered-content'), doc.text);
    updateMiniPreview(doc);
    renderDocumentList(state.documentList);
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
        `${t('chunks_label')}: ${doc.chunks.length} | ${t('length_label')}: ${doc.text.length} ${t('chars_label')} | UUID: ${doc.doc_uuid}`;

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
    document.getElementById('status-filename').textContent = file.name;
    document.getElementById('status-error').style.display = 'none';
    document.getElementById('status-title').textContent = `${t('processing_file')} ${file.name}`;
    statusPanel.style.display = 'flex';
    resetSteps();
    updateStep('step-upload', 'active');

    const formData = new FormData();
    formData.append('file', file);

    try {
        updateStep('step-upload', 'completed');
        updateStep('step-ocr', 'active');
        updateStep('step-ocr', file.name.match(/\.(txt|md)$/i) ? 'completed' : 'completed');
        updateStep('step-chunk', 'active');

        const result = await fetchJson('/upload', {
            method: 'POST',
            body: formData,
        });

        updateStep('step-chunk', 'completed');
        updateStep('step-embedding', 'completed');
        updateStep('step-db', 'completed');
        await fetchDocuments();

        const newDoc = state.documentList.find((doc) => doc.doc_uuid === result.doc_uuid);
        if (newDoc) selectDocument(newDoc.doc_uuid);
    } catch (error) {
        console.error('Upload failed:', error);
        const errorBox = document.getElementById('status-error');
        errorBox.textContent = error.message;
        errorBox.style.display = 'block';
    }
}

function updateStep(stepId, status) {
    const element = document.getElementById(stepId);
    if (!element) return;
    element.classList.remove('active', 'completed', 'failed');
    if (status) element.classList.add(status);
}

function resetSteps() {
    ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach((stepId) => {
        updateStep(stepId, '');
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
