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

    const charCount = doc.text.length;
    const tokenEstimate = Math.round(charCount / 4);
    document.getElementById('viewer-meta').textContent =
        `${t('chunks_label')}: ${doc.chunks.length} | ${t('length_label')}: ${charCount} ${t('chars_label')} | ${t('tokens_label')}: ${tokenEstimate} | UUID: ${doc.doc_uuid}`;

    updateMiniPreview(doc);
    renderDocumentList(state.documentList);
    renderActiveTab();

    // Fetch details in background/async
    try {
        const details = await fetchJson(`/documents/${docUuid}/details`);
        if (requestId !== activeDocumentRequestId || state.selectedDocUuid !== docUuid) {
            return;
        }
        state.selectedDocDetails = details;
    } catch (error) {
        console.error('Failed to fetch document details:', error);
        if (requestId !== activeDocumentRequestId || state.selectedDocUuid !== docUuid) {
            return;
        }
        state.selectedDocDetails = { chunks: doc.chunks, anchors: [], edges: [] };
    }

    renderActiveTab();
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

    // Show/Hide tab content elements
    const tabs = ['content', 'hierarchy', 'entities', 'graph'];
    tabs.forEach(t => {
        const contentEl = document.getElementById(`tab-content-${t}`);
        if (contentEl) {
            contentEl.style.display = t === tabName ? 'block' : 'none';
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
            renderMarkdownInto(document.getElementById('rendered-content'), doc.text);
        }
        return;
    }

    if (!details) {
        return;
    } else if (activeTab === 'hierarchy') {
        renderHierarchyTree(document.getElementById('hierarchy-tree-container'), details.chunks);
    } else if (activeTab === 'entities') {
        renderEntities(document.getElementById('entities-tags-container'), details.chunks);
    } else if (activeTab === 'graph') {
        renderDocumentGraph(null, state.selectedDocUuid);
    }
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

    const content = document.createElement('div');
    content.className = 'hierarchy-item-content';
    content.style.display = 'none';

    const textDiv = document.createElement('div');
    textDiv.className = 'chunk-full-text';
    textDiv.textContent = chunk.text;
    content.appendChild(textDiv);

    const metaFooter = document.createElement('div');
    metaFooter.className = 'chunk-meta-footer';
    let metaParts = [];
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

    header.addEventListener('click', (event) => {
        event.stopPropagation();
        const isCollapsed = content.style.display === 'none';
        content.style.display = isCollapsed ? 'block' : 'none';
        
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

    const sortedEntities = Object.entries(entityCounts).sort((a, b) => b[1] - a[1]);
    const sortedKeywords = Object.entries(keywordCounts).sort((a, b) => b[1] - a[1]);

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

function cleanupGraphRenderer() {
    if (graphAnimationId) {
        cancelAnimationFrame(graphAnimationId);
        graphAnimationId = null;
    }

    const canvasWrapper = document.getElementById('graph-canvas-wrapper');
    const oldCanvas = canvasWrapper?.querySelector('canvas');
    if (oldCanvas && oldCanvas.cleanupGraph) {
        oldCanvas.cleanupGraph();
    }
}

function renderDocumentGraph(container, docUuid) {
    cleanupGraphRenderer();

    const canvasWrapper = document.getElementById('graph-canvas-wrapper');
    if (!canvasWrapper) return;

    canvasWrapper.innerHTML = '';
    const canvas = document.createElement('canvas');
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    canvas.style.display = 'block';
    canvasWrapper.appendChild(canvas);
    
    const rect = canvasWrapper.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = (rect.width || 600) * dpr;
    canvas.height = (rect.height || 450) * dpr;
    const ctx = canvas.getContext('2d');
    ctx.scale(dpr, dpr);
    
    const width = rect.width || 600;
    const height = rect.height || 450;
    
    ctx.fillStyle = '#9ca3af';
    ctx.font = '14px sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText(state.uiPrefs.language === 'ru' ? 'Загрузка графа...' : 'Loading graph...', width / 2, height / 2);
    
    fetchJson(`/documents/${docUuid}/graph?depth=2`)
        .then(data => {
            initGraphSimulation(canvas, ctx, width, height, data, docUuid);
        })
        .catch(err => {
            console.error("Failed to load graph:", err);
            ctx.clearRect(0, 0, width, height);
            ctx.fillStyle = '#ef4444';
            ctx.fillText(state.uiPrefs.language === 'ru' ? `Ошибка загрузки графа: ${err.message}` : `Failed to load graph: ${err.message}`, width / 2, height / 2);
        });
}

function initGraphSimulation(canvas, ctx, width, height, data, activeDocUuid) {
    const nodesData = data.nodes || [];
    const edgesData = data.edges || [];
    
    const nodes = nodesData.map((n, i) => {
        const angle = (i / nodesData.length) * Math.PI * 2;
        const radius = 100 * Math.random();
        return {
            id: n.doc_uuid,
            label: n.filename || 'Unknown',
            x: width / 2 + Math.cos(angle) * radius,
            y: height / 2 + Math.sin(angle) * radius,
            vx: 0,
            vy: 0,
            radius: n.doc_uuid === activeDocUuid ? 22 : 16,
            isActive: n.doc_uuid === activeDocUuid
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
                metadata: e.metadata
            });
        }
    });
    
    let draggedNode = null;
    let hoveredNode = null;
    
    function getNodeAt(x, y) {
        return nodes.find(n => {
            const dx = n.x - x;
            const dy = n.y - y;
            return Math.sqrt(dx*dx + dy*dy) < n.radius + 5;
        });
    }
    
    function getMousePos(event) {
        const rect = canvas.getBoundingClientRect();
        return {
            x: event.clientX - rect.left,
            y: event.clientY - rect.top
        };
    }
    
    const onMouseDown = (event) => {
        const pos = getMousePos(event);
        draggedNode = getNodeAt(pos.x, pos.y);
    };
    
    const onMouseMove = (event) => {
        const pos = getMousePos(event);
        if (draggedNode) {
            draggedNode.x = pos.x;
            draggedNode.y = pos.y;
            draggedNode.vx = 0;
            draggedNode.vy = 0;
        } else {
            hoveredNode = getNodeAt(pos.x, pos.y);
            canvas.style.cursor = hoveredNode ? 'pointer' : 'default';
        }
    };
    
    const stopDrag = () => {
        draggedNode = null;
    };
    
    const onClick = (event) => {
        const pos = getMousePos(event);
        const clickedNode = getNodeAt(pos.x, pos.y);
        if (clickedNode && clickedNode.id !== activeDocUuid) {
            selectDocument(clickedNode.id);
        }
    };
    
    canvas.addEventListener('mousedown', onMouseDown);
    canvas.addEventListener('mousemove', onMouseMove);
    canvas.addEventListener('click', onClick);
    window.addEventListener('mouseup', stopDrag);
    
    const repulsion = 400;
    const attraction = 0.04;
    const gravity = 0.02;
    const damping = 0.85;
    
    function tick() {
        for (let i = 0; i < nodes.length; i++) {
            const n1 = nodes[i];
            for (let j = i + 1; j < nodes.length; j++) {
                const n2 = nodes[j];
                const dx = n2.x - n1.x;
                const dy = n2.y - n1.y;
                const distSq = dx*dx + dy*dy || 1;
                const dist = Math.sqrt(distSq);
                
                const force = repulsion / distSq;
                const fx = (dx / dist) * force;
                const fy = (dy / dist) * force;
                
                if (n1 !== draggedNode) {
                    n1.vx -= fx;
                    n1.vy -= fy;
                }
                if (n2 !== draggedNode) {
                    n2.vx += fx;
                    n2.vy += fy;
                }
            }
        }
        
        edges.forEach(edge => {
            const n1 = edge.source;
            const n2 = edge.target;
            const dx = n2.x - n1.x;
            const dy = n2.y - n1.y;
            const dist = Math.sqrt(dx*dx + dy*dy) || 1;
            
            const desiredDist = 120;
            const force = (dist - desiredDist) * attraction;
            const fx = (dx / dist) * force;
            const fy = (dy / dist) * force;
            
            if (n1 !== draggedNode) {
                n1.vx += fx;
                n1.vy += fy;
            }
            if (n2 !== draggedNode) {
                n2.vx -= fx;
                n2.vy -= fy;
            }
        });
        
        nodes.forEach(n => {
            if (n === draggedNode) return;
            const dx = width / 2 - n.x;
            const dy = height / 2 - n.y;
            n.vx += dx * gravity;
            n.vy += dy * gravity;
        });
        
        nodes.forEach(n => {
            if (n === draggedNode) return;
            n.vx *= damping;
            n.vy *= damping;
            n.x += n.vx;
            n.y += n.vy;
            
            n.x = Math.max(n.radius, Math.min(width - n.radius, n.x));
            n.y = Math.max(n.radius, Math.min(height - n.radius, n.y));
        });
        
        ctx.clearRect(0, 0, width, height);
        
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
            
            if (n.isActive) {
                ctx.fillStyle = '#8b5cf6';
                ctx.strokeStyle = '#c084fc';
                ctx.lineWidth = 3;
            } else if (n === hoveredNode) {
                ctx.fillStyle = '#10b981';
                ctx.strokeStyle = '#6ee7b7';
                ctx.lineWidth = 2;
            } else {
                ctx.fillStyle = '#374151';
                ctx.strokeStyle = '#6b7280';
                ctx.lineWidth = 1.5;
            }
            ctx.fill();
            ctx.stroke();
            
            ctx.fillStyle = '#f3f4f6';
            ctx.font = n.isActive ? 'bold 11px sans-serif' : '10px sans-serif';
            ctx.textAlign = 'center';
            ctx.textBaseline = 'top';
            
            let label = n.label;
            if (label.length > 15) {
                label = label.slice(0, 12) + '...';
            }
            
            ctx.fillText(label, n.x, n.y + n.radius + 4);
        });
        
        graphAnimationId = requestAnimationFrame(tick);
    }
    
    tick();
    
    canvas.cleanupGraph = () => {
        canvas.removeEventListener('mousedown', onMouseDown);
        canvas.removeEventListener('mousemove', onMouseMove);
        canvas.removeEventListener('click', onClick);
        window.removeEventListener('mouseup', stopDrag);
    };
}
