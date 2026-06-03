document.addEventListener('DOMContentLoaded', () => {
    initApp();
});

let documentList = [];
let rawChunks = [];
let selectedDocUuid = null;
let currentPreviewDoc = null;

function initApp() {
    fetchDocuments();
    initUpload();
    
    // Search library input
    const searchInput = document.getElementById('library-search');
    searchInput.addEventListener('input', (e) => {
        filterLibrary(e.target.value);
    });

    // Bind action buttons
    document.getElementById('download-doc-btn').addEventListener('click', downloadActiveDocument);
    document.getElementById('delete-doc-btn').addEventListener('click', deleteActiveDocument);

    // Bind clear database modal buttons
    const clearModal = document.getElementById('clear-db-modal');
    document.getElementById('clear-db-btn').addEventListener('click', () => {
        clearModal.style.display = 'flex';
    });
    document.getElementById('modal-cancel-btn').addEventListener('click', () => {
        clearModal.style.display = 'none';
    });
    document.getElementById('modal-confirm-btn').addEventListener('click', async () => {
        clearModal.style.display = 'none';
        try {
            const response = await fetch('/clear', {
                method: 'POST'
            });
            if (!response.ok) throw new Error('Не удалось очистить базу данных');
            
            selectedDocUuid = null;
            document.getElementById('viewer-section').style.display = 'none';
            document.getElementById('empty-state').style.display = 'flex';
            document.getElementById('drop-zone').style.display = 'flex';
            
            await fetchDocuments();
            alert('База данных успешно очищена.');
        } catch (err) {
            alert('Ошибка при очистке: ' + err.message);
        }
    });

    // Bind document preview modal buttons & clicks
    const miniPreview = document.getElementById('document-mini-preview');
    if (miniPreview) {
        miniPreview.addEventListener('click', openDocPreviewModal);
    }
    
    const docModalCloseBtn = document.getElementById('doc-modal-close-btn');
    if (docModalCloseBtn) {
        docModalCloseBtn.addEventListener('click', closeDocPreviewModal);
    }
    
    // Close preview modal on clicking outside content
    const docPreviewModal = document.getElementById('doc-preview-modal');
    if (docPreviewModal) {
        docPreviewModal.addEventListener('click', (e) => {
            if (e.target === docPreviewModal) {
                closeDocPreviewModal();
            }
        });
    }
    
    // Bind modal search input
    const modalSearchInput = document.getElementById('doc-modal-search');
    if (modalSearchInput) {
        modalSearchInput.addEventListener('input', (e) => {
            const query = e.target.value;
            const modalContent = document.getElementById('doc-modal-content');
            if (currentPreviewDoc) {
                // Re-render original HTML first
                let parsedHTML = marked.parse(currentPreviewDoc.text);
                parsedHTML = renderMath(parsedHTML);
                modalContent.innerHTML = parsedHTML;
                
                // Re-init Mermaid
                try {
                    if (window.mermaid) {
                        window.mermaid.init(undefined, modalContent.querySelectorAll('.mermaid'));
                    }
                } catch (err) {}
                
                // Highlight terms
                highlightSearchTerms(modalContent, query);
            }
        });
    }
}

// Group the flat list of chunks by doc_uuid
function getGroupedDocuments(chunks) {
    const grouped = {};
    chunks.forEach(chunk => {
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
    
    // Sort chunks inside each document by id ascending to preserve document order
    const docs = Object.values(grouped);
    docs.forEach(doc => {
        doc.chunks.sort((a, b) => a.id - b.id);
        // Stitch the text
        doc.text = doc.chunks.map(c => c.text).join('\n\n');
        // Primary ID to use for API calls like deletion
        doc.id = doc.chunks[0].id;
    });
    
    return docs;
}

// Fetch documents from Axum endpoint
async function fetchDocuments() {
    try {
        const response = await fetch('/documents');
        if (!response.ok) throw new Error('HTTP error ' + response.status);
        const data = await response.json();
        rawChunks = data.documents || [];
        documentList = getGroupedDocuments(rawChunks);
        renderDocumentList(documentList);
        
        // Re-select document if it still exists
        if (selectedDocUuid) {
            const stillExists = documentList.some(d => d.doc_uuid === selectedDocUuid);
            if (stillExists) {
                selectDocument(selectedDocUuid);
            } else {
                selectedDocUuid = null;
                document.getElementById('viewer-section').style.display = 'none';
                document.getElementById('empty-state').style.display = 'flex';
                document.getElementById('drop-zone').style.display = 'flex';
            }
        } else {
            document.getElementById('viewer-section').style.display = 'none';
            document.getElementById('empty-state').style.display = 'flex';
            document.getElementById('drop-zone').style.display = 'flex';
        }
    } catch (err) {
        console.error('Failed to fetch documents from library:', err);
    }
}

// Render library items
function renderDocumentList(docs) {
    const listEl = document.getElementById('document-list');
    listEl.innerHTML = '';
    
    if (docs.length === 0) {
        listEl.innerHTML = '<li class="document-item" style="cursor: default; text-align: center; color: var(--text-secondary);">Библиотека пуста</li>';
        return;
    }
    
    docs.forEach(doc => {
        const li = document.createElement('li');
        li.className = 'document-item';
        if (doc.doc_uuid === selectedDocUuid) li.className += ' active';
        
        li.innerHTML = `
            <div class="document-item-title" title="${doc.filename}">${doc.filename}</div>
            <div class="document-item-meta" style="font-size: 10px; margin-top: 4px; color: var(--text-secondary); display: flex; flex-direction: column; gap: 2px;">
                <span style="font-family: monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">UUID: ${doc.doc_uuid}</span>
                <span>Сегментов: ${doc.chunks.length}</span>
            </div>
        `;
        
        li.addEventListener('click', () => selectDocument(doc.doc_uuid));
        listEl.appendChild(li);
    });
}

// Filter library based on search
function filterLibrary(query) {
    if (!query) {
        renderDocumentList(documentList);
        return;
    }
    const q = query.toLowerCase();
    const filtered = documentList.filter(doc => 
        doc.filename.toLowerCase().includes(q) || 
        doc.doc_uuid.toLowerCase().includes(q) ||
        doc.text.toLowerCase().includes(q)
    );
    renderDocumentList(filtered);
}

// Select and preview a document
function selectDocument(uuid) {
    selectedDocUuid = uuid;
    
    // Set active item class
    const items = document.querySelectorAll('.document-item');
    items.forEach(el => el.classList.remove('active'));
    
    // Find active element to highlight
    const doc = documentList.find(d => d.doc_uuid === uuid);
    if (!doc) return;
    
    // Show sections
    document.getElementById('empty-state').style.display = 'none';
    document.getElementById('viewer-section').style.display = 'flex';
    document.getElementById('drop-zone').style.display = 'none';
    
    // Update titles
    document.getElementById('viewer-title').innerText = doc.filename;
    
    // Calculate size stats
    const charCount = doc.text.length;
    const tokenEst = Math.round(charCount / 4);
    document.getElementById('viewer-meta').innerText = `Сегментов: ${doc.chunks.length} | Длина: ${charCount} симв. | Примерно токенов: ${tokenEst} | UUID: ${doc.doc_uuid}`;
    
    // Render rich content
    const contentEl = document.getElementById('rendered-content');
    
    // Configure marked to format mermaid codeblocks
    const renderer = new marked.Renderer();
    renderer.code = function(code, language) {
        if (language === 'mermaid') {
            return `<pre class="mermaid">${code}</pre>`;
        }
        return `<pre><code>${code}</code></pre>`;
    };
    marked.setOptions({ renderer });
    
    // Render Markdown and LaTeX
    let parsedHTML = marked.parse(doc.text);
    parsedHTML = renderMath(parsedHTML);
    contentEl.innerHTML = parsedHTML;
    
    // Initialize Mermaid diagrams
    try {
        if (window.mermaid) {
            window.mermaid.init(undefined, document.querySelectorAll('.mermaid'));
        }
    } catch (e) {
        console.error("Mermaid layout render failed: ", e);
    }
    
    // Update HTML layout preview card
    updateMiniPreview(doc);
    
    // Highlight list item
    renderDocumentList(documentList);
}

// Render LaTeX equations helper
function renderMath(text) {
    // 1. Block equations $$ ... $$
    let html = text.replace(/\$\$([\s\S]+?)\$\$/g, (match, math) => {
        try {
            return `<div class="katex-block" style="margin: 16px 0; text-align: center;">${window.katex.renderToString(math, { displayMode: true, throwOnError: false })}</div>`;
        } catch (e) {
            return match;
        }
    });
    
    // 2. Inline equations $ ... $
    html = html.replace(/\$([^\$\n]+?)\$/g, (match, math) => {
        try {
            return `<span class="katex-inline">${window.katex.renderToString(math, { displayMode: false, throwOnError: false })}</span>`;
        } catch (e) {
            return match;
        }
    });
    
    return html;
}

// CSS Visual Document Page Drawing
function updateMiniPreview(doc) {
    const extEl = document.getElementById('mini-doc-ext');
    const bodyEl = document.getElementById('mini-doc-body');
    const nameEl = document.getElementById('mini-doc-name');
    
    if (!extEl || !bodyEl || !nameEl) return;
    
    const ext = doc.filename.split('.').pop().toUpperCase();
    extEl.innerText = ext;
    nameEl.innerText = doc.filename;
    
    // Clean markdown characters and truncate for aesthetic mockup page preview
    let cleanText = doc.text
        .replace(/[#*`_~\[\]]/g, '')
        .substring(0, 800);
        
    bodyEl.innerText = cleanText;
}

function openDocPreviewModal() {
    if (!selectedDocUuid) return;
    const doc = documentList.find(d => d.doc_uuid === selectedDocUuid);
    if (!doc) return;
    
    currentPreviewDoc = doc;
    
    const modal = document.getElementById('doc-preview-modal');
    const modalTitle = document.getElementById('doc-modal-title');
    const modalMeta = document.getElementById('doc-modal-meta');
    const modalContent = document.getElementById('doc-modal-content');
    const searchInput = document.getElementById('doc-modal-search');
    const searchCount = document.getElementById('doc-modal-search-count');
    
    // Reset search
    searchInput.value = '';
    searchCount.innerText = '';
    
    modalTitle.innerText = doc.filename;
    modalMeta.innerText = `Сегментов: ${doc.chunks.length} | Длина: ${doc.text.length} симв. | UUID: ${doc.doc_uuid}`;
    
    // Render the content (standard Markdown + LaTeX + Mermaid)
    let parsedHTML = marked.parse(doc.text);
    parsedHTML = renderMath(parsedHTML);
    modalContent.innerHTML = parsedHTML;
    
    // Init Mermaid in modal
    try {
        if (window.mermaid) {
            window.mermaid.init(undefined, modalContent.querySelectorAll('.mermaid'));
        }
    } catch (e) {
        console.error("Mermaid modal layout render failed: ", e);
    }
    
    // Show modal with animation
    const modalContentContainer = modal.querySelector('.doc-modal-content');
    modal.style.display = 'flex';
    
    // Force reflow
    modalContentContainer.offsetHeight;
    
    modalContentContainer.style.transform = 'scale(1)';
    modalContentContainer.style.opacity = '1';
}

function closeDocPreviewModal() {
    const modal = document.getElementById('doc-preview-modal');
    if (!modal) return;
    const modalContentContainer = modal.querySelector('.doc-modal-content');
    if (!modalContentContainer) return;
    
    modalContentContainer.style.transform = 'scale(0.9)';
    modalContentContainer.style.opacity = '0';
    
    setTimeout(() => {
        modal.style.display = 'none';
        currentPreviewDoc = null;
    }, 300);
}

// Safely highlight search terms without breaking HTML markup tags
function highlightSearchTerms(container, query) {
    removeHighlights(container);
    
    if (!query || query.trim().length === 0) {
        document.getElementById('doc-modal-search-count').innerText = '';
        return;
    }
    
    const terms = query.trim().replace(/[-\/\\^$*+?.()|[\]{}]/g, '\\$&'); // escape regex special chars
    const regex = new RegExp(`(${terms})`, 'gi');
    
    let count = 0;
    
    function walk(node) {
        if (node.nodeType === 3) { // Text node
            const matches = node.nodeValue.match(regex);
            if (matches) {
                const span = document.createElement('span');
                span.className = 'highlight-wrapper';
                span.innerHTML = node.nodeValue.replace(regex, '<mark class="search-match" style="background-color: #fef08a; color: #854d0e; padding: 2px 2px; border-radius: 2px; font-weight: 600; box-shadow: 0 1px 2px rgba(0,0,0,0.05);">$1</mark>');
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
    
    document.getElementById('doc-modal-search-count').innerText = count > 0 ? `${count} совп.` : '0 совп.';
    
    // Scroll the first match into view
    const firstMatch = container.querySelector('.search-match');
    if (firstMatch) {
        firstMatch.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
}

function removeHighlights(container) {
    const wrappers = container.querySelectorAll('.highlight-wrapper');
    wrappers.forEach(wrapper => {
        const parent = wrapper.parentNode;
        parent.replaceChild(document.createTextNode(wrapper.textContent), wrapper);
    });
    container.normalize();
}

// Drag & Drop Upload Logic
function initUpload() {
    const dropZone = document.getElementById('drop-zone');
    const fileInput = document.getElementById('file-input');
    
    // Prevent defaults
    ['dragenter', 'dragover', 'dragleave', 'drop'].forEach(eventName => {
        dropZone.addEventListener(eventName, preventDefaults, false);
    });
    
    function preventDefaults(e) {
        e.preventDefault();
        e.stopPropagation();
    }
    
    // Highlight drop zone
    ['dragenter', 'dragover'].forEach(eventName => {
        dropZone.addEventListener(eventName, () => dropZone.classList.add('dragover'), false);
    });
    
    ['dragleave', 'drop'].forEach(eventName => {
        dropZone.addEventListener(eventName, () => dropZone.classList.remove('dragover'), false);
    });
    
    // Handle dropped files
    dropZone.addEventListener('drop', (e) => {
        const dt = e.dataTransfer;
        const files = dt.files;
        if (files.length > 0) {
            handleFilesUpload(files[0]);
        }
    });
    
    // Handle click upload
    fileInput.addEventListener('change', (e) => {
        if (e.target.files.length > 0) {
            handleFilesUpload(e.target.files[0]);
        }
    });
}

// Process and Upload file to Axum server
async function handleFilesUpload(file) {
    const statusPanel = document.getElementById('status-panel');
    const statusFilename = document.getElementById('status-filename');
    const statusError = document.getElementById('status-error');
    
    statusFilename.innerText = file.name;
    statusPanel.style.display = 'block';
    statusError.style.display = 'none';
    statusError.innerText = '';
    
    // Reset steps
    resetSteps();
    
    let uploadFile = file;
    let isOcrNeeded = true;
    
    const fileExt = file.name.split('.').pop().toLowerCase();
    
    // Check if DOCX
    if (fileExt === 'docx') {
        updateStep('step-upload', 'active');
        try {
            if (!window.mammoth) {
                throw new Error("Библиотека mammoth.js не загружена.");
            }
            const arrayBuffer = await readFileAsArrayBuffer(file);
            const result = await window.mammoth.extractRawText({ arrayBuffer: arrayBuffer });
            
            if (!result.value || result.value.trim().length === 0) {
                throw new Error("Из документа .docx не удалось извлечь текст.");
            }
            
            // Create a text file from extracted text
            const blob = new Blob([result.value], { type: 'text/plain;charset=utf-8' });
            uploadFile = new File([blob], file.name + ".txt", { type: 'text/plain' });
            isOcrNeeded = false; // We extracted text, so OCR is not needed
            
            updateStep('step-upload', 'completed');
        } catch (err) {
            console.error('Local DOCX extraction failed:', err);
            statusError.innerText = `Ошибка локального парсинга DOCX: ${err.message}`;
            statusError.style.display = 'block';
            updateStep('step-upload', 'failed');
            ['step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach(id => updateStep(id, 'failed'));
            return;
        }
    } else if (fileExt === 'txt' || fileExt === 'md') {
        isOcrNeeded = false;
    } else if (fileExt !== 'pdf' && !['png', 'jpg', 'jpeg', 'webp'].includes(fileExt)) {
        // Warn user about unsupported file types, but proceed
        statusError.innerText = `Предупреждение: Формат .${fileExt} не поддерживается Mistral OCR API напрямую. Загрузка может завершиться ошибкой.`;
        statusError.style.display = 'block';
    }
    
    // Start step-by-step UI progress
    updateStep('step-upload', 'active');
    
    const formData = new FormData();
    formData.append('file', uploadFile);
    
    // Run pipeline timer simulation for status updates
    let ocrTimer, chunkTimer, embedTimer, dbTimer;
    
    if (isOcrNeeded) {
        // Normal OCR flow timers
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
        // Local text/docx bypass flow timers (much faster)
        ocrTimer = setTimeout(() => {
            updateStep('step-upload', 'completed');
            // OCR is bypassed
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
        
        // Clear simulated timers
        clearTimeout(ocrTimer);
        clearTimeout(chunkTimer);
        clearTimeout(embedTimer);
        clearTimeout(dbTimer);
        
        if (!response.ok) {
            let errorText = `Код ошибки: ${response.status}`;
            try {
                const errJson = await response.json();
                if (errJson && errJson.error) {
                    errorText = errJson.error;
                }
            } catch (e) {
                // fallback to general status text
                const text = await response.text();
                if (text && text.length < 200) {
                    errorText = text;
                }
            }
            throw new Error(errorText);
        }
        
        const result = await response.json();
        
        // Mark all steps completed on success
        updateStep('step-upload', 'completed');
        updateStep('step-ocr', 'completed');
        updateStep('step-chunk', 'completed');
        updateStep('step-embedding', 'completed');
        updateStep('step-db', 'completed');
        
        // Refresh document list and select the first new item
        await fetchDocuments();
        if (result.ids && result.ids.length > 0) {
            const newDoc = documentList.find(d => d.chunks.some(c => result.ids.includes(c.id)));
            if (newDoc) {
                selectDocument(newDoc.doc_uuid);
            }
        }
        
    } catch (err) {
        console.error('File ingestion failed:', err);
        
        // Stop all timers
        clearTimeout(ocrTimer);
        clearTimeout(chunkTimer);
        clearTimeout(embedTimer);
        clearTimeout(dbTimer);
        
        // Show error message
        statusError.innerText = `Ошибка обработки документа: ${err.message}`;
        statusError.style.display = 'block';
        
        // Mark current active/pending steps as failed
        ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach(stepId => {
            // For text/docx bypass, step-ocr should remain completed (since it is bypassed)
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
        reader.onerror = (e) => reject(new Error("Не удалось прочитать файл"));
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
    ['step-upload', 'step-ocr', 'step-chunk', 'step-embedding', 'step-db'].forEach(stepId => {
        updateStep(stepId, 'idle');
    });
}

// Download currently selected document text client-side
function downloadActiveDocument() {
    if (!selectedDocUuid) return;
    const doc = documentList.find(d => d.doc_uuid === selectedDocUuid);
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

// Delete currently selected document from DB & vector index
async function deleteActiveDocument() {
    if (!selectedDocUuid) return;
    const doc = documentList.find(d => d.doc_uuid === selectedDocUuid);
    if (!doc) return;
    
    if (!confirm(`Вы уверены, что хотите удалить весь документ "${doc.filename}" и все его сегменты (${doc.chunks.length})?`)) return;
    
    try {
        // Send delete request for the primary chunk ID (the backend will handle deleting all chunks sharing that doc_uuid)
        const response = await fetch(`/documents/${doc.id}`, {
            method: 'DELETE'
        });
        
        if (!response.ok) throw new Error('Не удалось удалить документ');
        
        // Clear active selection and reset viewer
        selectedDocUuid = null;
        document.getElementById('viewer-section').style.display = 'none';
        document.getElementById('empty-state').style.display = 'flex';
        
        // Refresh document sidebar
        await fetchDocuments();
    } catch (err) {
        alert('Ошибка при удалении документа: ' + err.message);
    }
}
