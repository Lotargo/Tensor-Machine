import { state, setChatMessages } from './state.js';
import { escapeHtml, showNotice, t, saveUiPref } from './core.js';

let activeStreamController = null;

export function initChat() {
    document.getElementById('send-chat-btn').addEventListener('click', () => sendChatMessage('sidebar'));
    document.getElementById('send-chat-modal-btn').addEventListener('click', () => sendChatMessage('modal'));
    document.getElementById('clear-chat-btn').addEventListener('click', clearChat);
    document.getElementById('open-chat-modal-btn').addEventListener('click', openChatModal);
    document.getElementById('chat-modal-close-btn').addEventListener('click', closeChatModal);
    document.getElementById('chat-modal').addEventListener('click', (event) => {
        if (event.target.id === 'chat-modal') closeChatModal();
    });

    bindComposerShortcuts(document.getElementById('chat-input'), 'sidebar');
    bindComposerShortcuts(document.getElementById('chat-modal-input'), 'modal');
    initChatResizer();
    document.getElementById('chat-rag-toggle').addEventListener('change', renderChatScope);
    document.getElementById('chat-scope-doc-toggle').addEventListener('change', renderChatScope);

    document.addEventListener('tensor-rag:chat-updated', renderChat);
    document.addEventListener('tensor-rag:selected-document', renderChatScope);
    document.addEventListener('tensor-rag:settings-loaded', renderChatScope);
    document.addEventListener('tensor-rag:translations-applied', () => {
        renderChat();
        renderChatScope();
    });

    if (!state.chatMessages.length) {
        setChatMessages([{
            role: 'system',
            content: t('chat_empty'),
        }]);
    }
    renderChat();
    renderChatScope();
}

function bindComposerShortcuts(textarea, source) {
    textarea.addEventListener('keydown', (event) => {
        if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            sendChatMessage(source);
        }
    });
}

function getScopeValue() {
    return document.getElementById('chat-scope-doc-toggle').checked;
}

function getRagValue() {
    return document.getElementById('chat-rag-toggle').checked;
}

function getChatMode() {
    return state.settings.find((setting) => setting.key === 'AI_CHAT_MODE')?.value || 'rag';
}

function getSelectedDocLabel() {
    if (!state.selectedDocUuid) return t('chat_scope_none');
    const doc = state.documentList.find((item) => item.doc_uuid === state.selectedDocUuid);
    if (!doc) return t('chat_scope_none');
    return `${doc.filename} (${doc.doc_uuid})`;
}

export function renderChatScope() {
    const caption = document.getElementById('chat-scope-caption');
    if (!getRagValue()) {
        caption.textContent = t('chat_scope_general');
        return;
    }
    if (getChatMode() === 'agent') {
        caption.textContent = getScopeValue() && state.selectedDocUuid
            ? `${t('chat_scope_active')}: ${getSelectedDocLabel()}`
            : 'Agent mode: the model will decide on its own whether to use MCP tools.';
        return;
    }
    caption.textContent = getScopeValue() && state.selectedDocUuid ? `${t('chat_scope_active')}: ${getSelectedDocLabel()}` : getSelectedDocLabel();
}

function getUserFacingMessages() {
    return state.chatMessages;
}

export function renderChat() {
    renderMessagesInto(document.getElementById('chat-messages'));
    renderMessagesInto(document.getElementById('chat-modal-messages'));
}

function renderMessagesInto(container) {
    container.innerHTML = '';
    getUserFacingMessages().forEach((message) => {
        const wrapper = document.createElement('div');
        wrapper.className = 'chat-message';
        wrapper.dataset.role = message.role;
        if (message.loading) wrapper.classList.add('chat-loading');

        const bubble = document.createElement('div');
        bubble.className = 'chat-bubble';
        if (message.loading || message.thinking) {
            const thinking = document.createElement('div');
            thinking.className = 'chat-thinking';
            thinking.innerHTML = `
                <span>${escapeHtml(message.thinkingLabel || 'Thinking')}</span>
                <span class="chat-thinking-dots"><span></span><span></span><span></span></span>
            `;
            wrapper.appendChild(thinking);
        }
        bubble.innerHTML = message.role === 'assistant' ? renderAssistantContent(message.content) : escapeHtml(message.content).replaceAll('\n', '<br>');
        wrapper.appendChild(bubble);

        if (message.sources?.length) {
            const sources = document.createElement('div');
            sources.className = 'chat-sources';
            message.sources.forEach((source) => {
                const card = document.createElement('div');
                card.className = 'chat-source-card';
                card.innerHTML = `
                    <strong>[${source.index}] ${escapeHtml(source.title)}</strong>
                    <p>${escapeHtml(source.excerpt)}</p>
                `;
                sources.appendChild(card);
            });
            wrapper.appendChild(sources);
        }

        container.appendChild(wrapper);
    });
    container.scrollTop = container.scrollHeight;
}

function renderAssistantContent(content) {
    if (!content) return '';
    try {
        return marked.parse(content);
    } catch (_error) {
        return escapeHtml(content).replaceAll('\n', '<br>');
    }
}

function readComposerValue(source) {
    return source === 'modal'
        ? document.getElementById('chat-modal-input').value
        : document.getElementById('chat-input').value;
}

function clearComposerValue() {
    document.getElementById('chat-input').value = '';
    document.getElementById('chat-modal-input').value = '';
}

async function sendChatMessage(source) {
    const text = readComposerValue(source).trim();
    if (!text) return;
    if (activeStreamController) {
        activeStreamController.abort();
        activeStreamController = null;
    }

    const outbound = state.chatMessages.filter((message) => !message.loading);
    const nextMessages = [
        ...outbound,
        { role: 'user', content: text },
        { role: 'assistant', content: '', loading: true, thinking: true, thinkingLabel: t('chat_thinking_search') },
    ];
    setChatMessages(nextMessages);
    clearComposerValue();

    try {
        const historyForApi = nextMessages
            .filter((message) => !message.loading && ['user', 'assistant'].includes(message.role))
            .map(({ role, content }) => ({ role, content }));

        activeStreamController = new AbortController();
        const response = await fetch('/chat/stream', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                messages: historyForApi,
                doc_uuid: state.selectedDocUuid,
                scope_to_selected_document: getScopeValue(),
                use_retrieval: getRagValue(),
            }),
            signal: activeStreamController.signal,
        });

        if (!response.ok || !response.body) {
            let message = `HTTP error ${response.status}`;
            try {
                const payload = await response.json();
                message = payload.error || message;
            } catch (_error) {}
            throw new Error(message);
        }

        await consumeSseResponse(response, outbound, text);
    } catch (error) {
        setChatMessages([...outbound, { role: 'user', content: text }]);
        if (error.name !== 'AbortError') {
            await showNotice(t('ai_chat_failed_title'), t('ai_chat_failed_message', { message: error.message }), 'danger');
        }
    } finally {
        activeStreamController = null;
    }
}

function clearChat() {
    setChatMessages([{ role: 'system', content: t('chat_empty') }]);
}

function openChatModal() {
    const modal = document.getElementById('chat-modal');
    const content = modal.querySelector('.chat-modal-content');
    modal.style.display = 'flex';
    content.offsetHeight;
    content.style.transform = 'scale(1)';
    content.style.opacity = '1';
    document.getElementById('chat-modal-input').focus();
}

function closeChatModal() {
    const modal = document.getElementById('chat-modal');
    const content = modal.querySelector('.chat-modal-content');
    content.style.transform = 'scale(0.9)';
    content.style.opacity = '0';
    setTimeout(() => {
        modal.style.display = 'none';
    }, 300);
}

async function consumeSseResponse(response, outbound, userText) {
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let accumulated = '';
    let collectedSources = [];

    while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        let splitIndex;
        while ((splitIndex = buffer.indexOf('\n\n')) !== -1) {
            const frame = buffer.slice(0, splitIndex);
            buffer = buffer.slice(splitIndex + 2);
            const parsed = parseSseFrame(frame);
            if (!parsed) continue;

            if (parsed.event === 'thinking') {
                const phaseLabel = parsed.data.label || (parsed.data.phase === 'reasoning'
                    ? t('chat_thinking_reason')
                    : t('chat_thinking_search'));
                updateStreamingAssistant(outbound, userText, {
                    content: accumulated,
                    loading: true,
                    thinking: true,
                    thinkingLabel: phaseLabel,
                    sources: collectedSources,
                });
            } else if (parsed.event === 'sources') {
                collectedSources = Array.isArray(parsed.data) ? parsed.data : [];
                updateStreamingAssistant(outbound, userText, {
                    content: accumulated,
                    loading: true,
                    thinking: true,
                    thinkingLabel: t('chat_thinking_reason'),
                    sources: collectedSources,
                });
            } else if (parsed.event === 'tool') {
                updateStreamingAssistant(outbound, userText, {
                    content: accumulated,
                    loading: true,
                    thinking: true,
                    thinkingLabel: parsed.data.label || t('chat_thinking_reason'),
                    sources: collectedSources,
                });
            } else if (parsed.event === 'token') {
                accumulated += parsed.data.delta || '';
                updateStreamingAssistant(outbound, userText, {
                    content: accumulated,
                    loading: true,
                    thinking: false,
                    thinkingLabel: '',
                    sources: collectedSources,
                });
            } else if (parsed.event === 'done') {
                collectedSources = parsed.data.sources || collectedSources;
                updateStreamingAssistant(outbound, userText, {
                    content: accumulated,
                    loading: false,
                    thinking: false,
                    thinkingLabel: '',
                    sources: collectedSources,
                });
            } else if (parsed.event === 'error') {
                throw new Error(parsed.data.message || 'Streaming failed');
            }
        }
    }

    updateStreamingAssistant(outbound, userText, {
        content: accumulated,
        loading: false,
        thinking: false,
        thinkingLabel: '',
        sources: collectedSources,
    });
}

function parseSseFrame(frame) {
    let event = 'message';
    const dataLines = [];
    frame.split('\n').forEach((line) => {
        if (line.startsWith('event: ')) {
            event = line.slice(7).trim();
        } else if (line.startsWith('data: ')) {
            dataLines.push(line.slice(6));
        }
    });
    if (!dataLines.length) return null;
    try {
        return {
            event,
            data: JSON.parse(dataLines.join('\n')),
        };
    } catch (_error) {
        return null;
    }
}

function updateStreamingAssistant(outbound, userText, assistant) {
    setChatMessages([
        ...outbound,
        { role: 'user', content: userText },
        { role: 'assistant', ...assistant },
    ]);
}

function initChatResizer() {
    const panel = document.querySelector('.chat-panel');
    const handle = document.getElementById('chat-resize-handle');
    if (!panel || !handle) return;

    applyChatHeight(panel, state.uiPrefs.chatPanelHeight || 620);

    const startResize = (clientY) => {
        const startY = clientY;
        const startHeight = panel.getBoundingClientRect().height;
        document.body.style.userSelect = 'none';
        document.body.style.cursor = 'ns-resize';

        const onMove = (moveEvent) => {
            const currentY = moveEvent.touches ? moveEvent.touches[0].clientY : moveEvent.clientY;
            const delta = currentY - startY;
            const nextHeight = Math.max(460, Math.min(1400, startHeight + delta));
            applyChatHeight(panel, nextHeight);
        };

        const onUp = () => {
            window.removeEventListener('mousemove', onMove);
            window.removeEventListener('mouseup', onUp);
            window.removeEventListener('touchmove', onMove);
            window.removeEventListener('touchend', onUp);
            document.body.style.userSelect = '';
            document.body.style.cursor = '';
            saveUiPref('chatPanelHeight', Math.round(panel.getBoundingClientRect().height));
        };

        window.addEventListener('mousemove', onMove);
        window.addEventListener('mouseup', onUp);
        window.addEventListener('touchmove', onMove, { passive: false });
        window.addEventListener('touchend', onUp);
    };

    handle.addEventListener('mousedown', (event) => {
        event.preventDefault();
        startResize(event.clientY);
    });

    handle.addEventListener('touchstart', (event) => {
        event.preventDefault();
        const touch = event.touches[0];
        if (!touch) return;
        startResize(touch.clientY);
    });
}

function applyChatHeight(panel, height) {
    panel.style.height = `${Math.round(height)}px`;
    panel.style.minHeight = `${Math.round(height)}px`;
}
