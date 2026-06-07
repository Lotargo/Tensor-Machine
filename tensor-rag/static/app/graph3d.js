const THREE_MODULE_URL = '/vendor/three/three.module.js';

const NODE_COLORS = {
    root: 0x68b8b1,
    doc: 0x8b95a1,
    macro: 0x3b82f6,
    medium: 0x06b6d4,
    micro: 0x64748b,
    entity: 0xf59e0b,
};

export async function createGraph3dRenderer(canvas, graphData, activeDocUuid, options = {}) {
    const THREE = await import(THREE_MODULE_URL);
    const canvasWrapper = canvas.parentElement;
    const renderer = new THREE.WebGLRenderer({
        canvas,
        antialias: true,
        alpha: true,
        preserveDrawingBuffer: Boolean(window.__TENSOR_RAG_GRAPH_PRESERVE_BUFFER__),
    });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setClearColor(0x000000, 0);

    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 5000);
    const graphGroup = new THREE.Group();
    scene.add(graphGroup);

    const ambientLight = new THREE.AmbientLight(0xffffff, 0.72);
    const keyLight = new THREE.DirectionalLight(0xffffff, 0.88);
    keyLight.position.set(160, 240, 360);
    scene.add(ambientLight, keyLight);

    const tooltipEl = createTooltip(canvasWrapper);
    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    const nodeMeshes = [];
    const nodeById = new Map();
    const nodes = normalizeNodes(graphData.nodes || []);
    const edges = graphData.edges || [];

    nodes.forEach((node) => nodeById.set(node.id, node));
    layoutNodes(nodes);
    renderStats(nodes, edges, options.statsEl, options.language);
    renderLegend(options.legendEl, options.language);

    const edgeMaterial = new THREE.LineBasicMaterial({
        color: 0x9ca3af,
        transparent: true,
        opacity: 0.28,
    });
    const relationMaterial = new THREE.LineBasicMaterial({
        color: 0x68b8b1,
        transparent: true,
        opacity: 0.46,
    });

    edges.forEach((edge) => {
        const source = nodeById.get(edge.source);
        const target = nodeById.get(edge.target);
        if (!source || !target) return;

        const geometry = new THREE.BufferGeometry().setFromPoints([
            new THREE.Vector3(source.x, source.y, source.z),
            new THREE.Vector3(target.x, target.y, target.z),
        ]);
        const line = new THREE.Line(geometry, edge.type === 'shares_entities' ? relationMaterial : edgeMaterial);
        graphGroup.add(line);
    });

    const geometries = new Map();
    nodes.forEach((node) => {
        const radius = Math.max(5, node.radius || 9);
        const geometryKey = Math.round(radius);
        let geometry = geometries.get(geometryKey);
        if (!geometry) {
            geometry = new THREE.SphereGeometry(radius, 18, 14);
            geometries.set(geometryKey, geometry);
        }

        const material = new THREE.MeshStandardMaterial({
            color: NODE_COLORS[node.type] || NODE_COLORS.doc,
            roughness: 0.52,
            metalness: node.isActive ? 0.12 : 0.02,
            emissive: NODE_COLORS[node.type] || NODE_COLORS.doc,
            emissiveIntensity: node.isActive ? 0.28 : 0.06,
        });
        const mesh = new THREE.Mesh(geometry, material);
        mesh.position.set(node.x, node.y, node.z);
        mesh.userData.node = node;
        graphGroup.add(mesh);
        nodeMeshes.push(mesh);

        if (shouldShowLabel(node)) {
            const label = createLabelSprite(THREE, node.label, node.isActive);
            label.position.set(node.x, node.y + radius + 12, node.z);
            graphGroup.add(label);
        }
    });

    const state = {
        animationId: null,
        disposed: false,
        hovered: null,
        dragging: false,
        moved: false,
        lastX: 0,
        lastY: 0,
        rotationX: -0.28,
        rotationY: 0.36,
        targetRotationX: -0.28,
        targetRotationY: 0.36,
        zoom: 720,
        targetZoom: 720,
    };

    function resize() {
        const rect = canvasWrapper.getBoundingClientRect();
        const width = rect.width || 600;
        const height = rect.height || 450;
        renderer.setSize(width, height, false);
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
    }

    function animate() {
        if (state.disposed) return;
        state.rotationX += (state.targetRotationX - state.rotationX) * 0.14;
        state.rotationY += (state.targetRotationY - state.rotationY) * 0.14;
        state.zoom += (state.targetZoom - state.zoom) * 0.16;
        graphGroup.rotation.x = state.rotationX;
        graphGroup.rotation.y = state.rotationY;
        camera.position.set(0, 0, state.zoom);
        camera.lookAt(0, 0, 0);
        renderer.render(scene, camera);
        state.animationId = requestAnimationFrame(animate);
    }

    function setPointerFromEvent(event) {
        const rect = canvas.getBoundingClientRect();
        pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
        pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    }

    function pickNode(event) {
        setPointerFromEvent(event);
        raycaster.setFromCamera(pointer, camera);
        const hit = raycaster.intersectObjects(nodeMeshes, false)[0];
        return hit?.object?.userData?.node || null;
    }

    function onPointerDown(event) {
        state.dragging = true;
        state.moved = false;
        state.lastX = event.clientX;
        state.lastY = event.clientY;
        canvas.setPointerCapture?.(event.pointerId);
    }

    function onPointerMove(event) {
        if (state.dragging) {
            const dx = event.clientX - state.lastX;
            const dy = event.clientY - state.lastY;
            if (Math.abs(dx) > 2 || Math.abs(dy) > 2) state.moved = true;
            state.targetRotationY += dx * 0.006;
            state.targetRotationX += dy * 0.004;
            state.targetRotationX = Math.max(-1.2, Math.min(1.2, state.targetRotationX));
            state.lastX = event.clientX;
            state.lastY = event.clientY;
            tooltipEl.style.display = 'none';
            return;
        }

        const node = pickNode(event);
        if (node !== state.hovered) {
            state.hovered = node;
            updateTooltip(tooltipEl, node, event, options.language);
            canvas.style.cursor = node ? 'pointer' : 'grab';
        } else if (node) {
            updateTooltip(tooltipEl, node, event, options.language);
        }
    }

    function onPointerUp(event) {
        const node = state.moved ? null : pickNode(event);
        state.dragging = false;
        canvas.releasePointerCapture?.(event.pointerId);
        if (!node) return;

        if ((node.type === 'root' || node.type === 'doc') && node.id !== `doc:${activeDocUuid}`) {
            options.onNavigate?.(node.id.replace(/^doc:/, ''));
            return;
        }
        if (node.id === `doc:${activeDocUuid}`) {
            options.onFocusNode?.(node.id);
            return;
        }
        if (['macro', 'medium', 'micro'].includes(node.type)) {
            if (event.shiftKey || event.altKey || !options.onFocusNode) {
                showChunkInspector(canvasWrapper, node, options.chunks || [], options.language);
            } else {
                options.onFocusNode(node.id);
            }
            return;
        }
        if (node.type === 'entity') {
            if (event.shiftKey || event.altKey || !options.onFocusNode) {
                options.onEntitySearch?.(node.label);
            } else {
                options.onFocusNode(node.id);
            }
        }
    }

    function onWheel(event) {
        event.preventDefault();
        state.targetZoom = Math.max(220, Math.min(1800, state.targetZoom + event.deltaY * 0.9));
    }

    function onResize() {
        resize();
    }

    resize();
    animate();
    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerup', onPointerUp);
    canvas.addEventListener('pointerleave', onPointerUp);
    canvas.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('resize', onResize);

    return {
        stop() {
            state.disposed = true;
            if (state.animationId) cancelAnimationFrame(state.animationId);
            canvas.removeEventListener('pointerdown', onPointerDown);
            canvas.removeEventListener('pointermove', onPointerMove);
            canvas.removeEventListener('pointerup', onPointerUp);
            canvas.removeEventListener('pointerleave', onPointerUp);
            canvas.removeEventListener('wheel', onWheel);
            window.removeEventListener('resize', onResize);
            tooltipEl.remove();
            graphGroup.traverse((object) => {
                object.geometry?.dispose?.();
                object.material?.dispose?.();
                object.material?.map?.dispose?.();
            });
            renderer.dispose();
        },
        resetView() {
            state.targetRotationX = -0.28;
            state.targetRotationY = 0.36;
            state.targetZoom = 720;
        },
        zoomIn() {
            state.targetZoom = Math.max(220, state.targetZoom * 0.78);
        },
        zoomOut() {
            state.targetZoom = Math.min(1800, state.targetZoom * 1.22);
        },
    };
}

function normalizeNodes(nodes) {
    return nodes.map((node) => ({ ...node, type: node.type || 'doc' }));
}

function layoutNodes(nodes) {
    const groups = groupBy(nodes, (node) => node.type);
    placeGroup(groups.root || [], 0, 0, 0, 0);
    placeGroup(groups.doc || [], 150, 70, -120, 0.4);
    placeGroup(groups.macro || [], 180, 0, -40, 0);
    placeGroup(groups.medium || [], 260, -30, -95, 0.32);
    placeGroup(groups.micro || [], 340, -70, -150, 0.6);
    placeGroup(groups.entity || [], 260, 110, 130, 0.12);
}

function groupBy(items, getKey) {
    return items.reduce((acc, item) => {
        const key = getKey(item);
        acc[key] = acc[key] || [];
        acc[key].push(item);
        return acc;
    }, {});
}

function placeGroup(nodes, radius, yOffset, zOffset, phase) {
    if (nodes.length === 0) return;
    if (radius === 0) {
        nodes.forEach((node) => {
            node.x = 0;
            node.y = yOffset;
            node.z = zOffset;
        });
        return;
    }

    nodes.forEach((node, index) => {
        const angle = (index / nodes.length) * Math.PI * 2 + phase;
        const layer = index % 3;
        node.x = Math.cos(angle) * (radius + layer * 26);
        node.y = yOffset + (layer - 1) * 34;
        node.z = Math.sin(angle) * radius + zOffset;
    });
}

function shouldShowLabel(node) {
    return node.isActive || node.type === 'root' || node.type === 'macro';
}

function createLabelSprite(THREE, text, isActive) {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const label = truncate(String(text || ''), isActive ? 34 : 24);
    ctx.font = isActive ? '700 24px sans-serif' : '600 20px sans-serif';
    const width = Math.ceil(ctx.measureText(label).width + 28);
    canvas.width = Math.max(96, width);
    canvas.height = 40;
    ctx.font = isActive ? '700 24px sans-serif' : '600 20px sans-serif';
    ctx.fillStyle = 'rgba(10, 16, 18, 0.72)';
    roundRect(ctx, 0, 0, canvas.width, canvas.height, 8);
    ctx.fill();
    ctx.fillStyle = '#eef4f1';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(label, canvas.width / 2, canvas.height / 2);
    const texture = new THREE.CanvasTexture(canvas);
    const material = new THREE.SpriteMaterial({ map: texture, transparent: true });
    const sprite = new THREE.Sprite(material);
    sprite.scale.set(canvas.width * 0.45, canvas.height * 0.45, 1);
    return sprite;
}

function renderStats(nodes, edges, statsEl, language) {
    if (!statsEl) return;
    const isRu = language === 'ru';
    const byType = {};
    nodes.forEach((node) => {
        byType[node.type] = (byType[node.type] || 0) + 1;
    });
    statsEl.innerHTML = [
        `<span><i class="fa-solid fa-cube"></i> ${nodes.length}</span>`,
        `<span><i class="fa-solid fa-link"></i> ${edges.length}</span>`,
        byType.macro ? `<span style="color:#93c5fd;">● ${byType.macro} ${isRu ? 'разделов' : 'sections'}</span>` : '',
        byType.medium ? `<span style="color:#67e8f9;">● ${byType.medium} ${isRu ? 'подразделов' : 'subsections'}</span>` : '',
        byType.entity ? `<span style="color:#fbbf24;">● ${byType.entity} ${isRu ? 'сущностей' : 'entities'}</span>` : '',
    ].filter(Boolean).join(' ');
}

function renderLegend(legendEl, language) {
    if (!legendEl) return;
    const isRu = language === 'ru';
    legendEl.innerHTML = `
        <span><span class="graph-legend-dot" style="background:#68b8b1;box-shadow:0 0 6px #68b8b1;"></span>${isRu ? 'Документ' : 'Document'}</span>
        <span><span class="graph-legend-dot" style="background:#3b82f6;"></span>${isRu ? 'Раздел' : 'Section'}</span>
        <span><span class="graph-legend-dot" style="background:#06b6d4;"></span>${isRu ? 'Подраздел' : 'Subsection'}</span>
        <span><span class="graph-legend-dot" style="background:#f59e0b;"></span>${isRu ? 'Сущность' : 'Entity'}</span>
        <span><span class="graph-legend-line" style="border-color:rgba(104,184,177,0.55);"></span>${isRu ? 'Связь / иерархия' : 'Link / hierarchy'}</span>
    `;
}

function createTooltip(parent) {
    const tooltip = document.createElement('div');
    tooltip.className = 'graph-tooltip';
    tooltip.style.display = 'none';
    parent.appendChild(tooltip);
    return tooltip;
}

function updateTooltip(tooltip, node, event, language) {
    if (!node) {
        tooltip.style.display = 'none';
        return;
    }
    const isRu = language === 'ru';
    tooltip.style.display = 'block';
    tooltip.style.left = `${event.offsetX + 14}px`;
    tooltip.style.top = `${Math.max(0, event.offsetY - 10)}px`;
    tooltip.innerHTML = `<b>${escapeHtml(node.label)}</b><br>${isRu ? 'Тип' : 'Type'}: ${escapeHtml(node.type)}`;
}

function showChunkInspector(parent, node, chunks, language) {
    const chunk = chunks.find((item) => item.chunk_index === node.chunk_index);
    if (!chunk) return;

    const existing = parent.querySelector('.graph-inspector');
    if (existing) existing.remove();

    const isRu = language === 'ru';
    const header = Array.isArray(chunk.header_chain) ? chunk.header_chain.join(' > ') : '';
    const entities = Array.isArray(chunk.entities) ? chunk.entities.join(', ') : '';
    const inspector = document.createElement('div');
    inspector.className = 'graph-inspector';
    inspector.innerHTML = `
        <div class="graph-inspector-header">
            <span class="graph-inspector-title">${escapeHtml(header || node.label)}</span>
            <button class="graph-inspector-close">&times;</button>
        </div>
        <div class="graph-inspector-body">
            <div class="graph-inspector-meta">
                <span>${isRu ? 'Уровень' : 'Level'}: ${escapeHtml(node.type)}</span>
                <span>${isRu ? 'Индекс' : 'Index'}: ${escapeHtml(String(node.chunk_index))}</span>
                ${entities ? `<span>${isRu ? 'Сущности' : 'Entities'}: ${escapeHtml(entities)}</span>` : ''}
            </div>
            <pre class="graph-inspector-text">${escapeHtml(chunk.text || '')}</pre>
        </div>
    `;
    inspector.querySelector('.graph-inspector-close').onclick = () => inspector.remove();
    inspector.querySelector('.graph-inspector-header').onclick = () => inspector.remove();
    parent.appendChild(inspector);
}

function truncate(value, maxLength) {
    return value.length > maxLength ? `${value.slice(0, Math.max(0, maxLength - 3))}...` : value;
}

function roundRect(ctx, x, y, width, height, radius) {
    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.arcTo(x + width, y, x + width, y + height, radius);
    ctx.arcTo(x + width, y + height, x, y + height, radius);
    ctx.arcTo(x, y + height, x, y, radius);
    ctx.arcTo(x, y, x + width, y, radius);
    ctx.closePath();
}

function escapeHtml(value) {
    return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}
