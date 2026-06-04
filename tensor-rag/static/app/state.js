export const state = {
    documentList: [],
    rawChunks: [],
    selectedDocUuid: null,
    selectedDocDetails: null,
    activeTab: 'content',
    currentPreviewDoc: null,
    collectionHealth: null,
    settings: [],
    chatMessages: [],
    uiPrefs: {
        language: 'ru',
        theme: 'system',
        leftSidebarCollapsed: false,
        rightSidebarCollapsed: false,
    },
};

export const storageKeys = {
    language: 'tensor-rag-ui-language',
    theme: 'tensor-rag-ui-theme',
    leftSidebarCollapsed: 'tensor-rag-left-sidebar-collapsed',
    rightSidebarCollapsed: 'tensor-rag-right-sidebar-collapsed',
    chatPanelHeight: 'tensor-rag-chat-panel-height',
};

export function setSelectedDocUuid(docUuid) {
    state.selectedDocUuid = docUuid;
    document.dispatchEvent(new CustomEvent('tensor-rag:selected-document', { detail: { docUuid } }));
}

export function setChatMessages(messages) {
    state.chatMessages = messages;
    document.dispatchEvent(new CustomEvent('tensor-rag:chat-updated'));
}
