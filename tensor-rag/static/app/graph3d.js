const THREE_MODULE_URL = '/vendor/three/three.module.js';

const NODE_COLORS = {
    root: 0x68b8b1,
    doc: 0x8b95a1,
    macro: 0x5b8def,
    medium: 0x27b7c6,
    micro: 0x7c8a93,
    entity: 0xd99a2b,
};

const EDGE_COLORS = {
    default: 0xbed2cd,
    hierarchy: 0x68b8b1,
    active: 0xd9fff7,
    relation: 0x8aa2b3,
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
    scene.fog = new THREE.FogExp2(0x111a17, 0.00072);
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 5000);
    const graphGroup = new THREE.Group();
    scene.add(graphGroup);

    const ambientLight = new THREE.AmbientLight(0xdcefe9, 0.68);
    const keyLight = new THREE.DirectionalLight(0xffffff, 0.78);
    const fillLight = new THREE.DirectionalLight(0x68b8b1, 0.24);
    keyLight.position.set(180, 260, 360);
    fillLight.position.set(-280, -120, 180);
    scene.add(ambientLight, keyLight, fillLight);

    const tooltipEl = createTooltip(canvasWrapper);
    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    const nodeMeshes = [];
    const nodeById = new Map();
    const nodes = normalizeNodes(graphData.nodes || []);
    const edges = graphData.edges || [];

    nodes.forEach((node) => nodeById.set(node.id, node));
    layoutNodes(nodes, edges);
    renderStats(nodes, edges, options.statsEl, options.language);
    renderLegend(options.legendEl, options.language);

    const activeNodeIds = new Set(nodes.filter((node) => node.isActive).map((node) => node.id));
    const edgeMaterials = new Map();
    const getEdgeMaterial = (edge) => {
        const isActiveEdge = activeNodeIds.has(edge.source) || activeNodeIds.has(edge.target);
        const isHierarchy = edge.type === 'contains';
        const key = isActiveEdge ? 'active' : edge.type || 'default';
        if (!edgeMaterials.has(key)) {
            edgeMaterials.set(key, new THREE.LineBasicMaterial({
                color: isActiveEdge ? EDGE_COLORS.active : (isHierarchy ? EDGE_COLORS.hierarchy : EDGE_COLORS.relation),
                transparent: true,
                opacity: isActiveEdge ? 0.52 : (isHierarchy ? 0.2 : 0.14),
                depthWrite: false,
            }));
        }
        return edgeMaterials.get(key);
    };

    edges.forEach((edge) => {
        const source = nodeById.get(edge.source);
        const target = nodeById.get(edge.target);
        if (!source || !target) return;

        const geometry = new THREE.BufferGeometry().setFromPoints([
            new THREE.Vector3(source.x, source.y, source.z),
            new THREE.Vector3(target.x, target.y, target.z),
        ]);
        const line = new THREE.Line(geometry, getEdgeMaterial(edge));
        graphGroup.add(line);
    });

    const geometries = new Map();
    nodes.forEach((node) => {
        const radius = getVisualRadius(node);
        const geometryKey = Math.round(radius);
        let geometry = geometries.get(geometryKey);
        if (!geometry) {
            geometry = new THREE.SphereGeometry(radius, 16, 12);
            geometries.set(geometryKey, geometry);
        }

        const material = new THREE.MeshStandardMaterial({
            color: NODE_COLORS[node.type] || NODE_COLORS.doc,
            roughness: 0.86,
            metalness: 0.0,
            emissive: NODE_COLORS[node.type] || NODE_COLORS.doc,
            emissiveIntensity: node.isActive ? 0.22 : 0.035,
        });
        const mesh = new THREE.Mesh(geometry, material);
        mesh.position.set(node.x, node.y, node.z);
        mesh.userData.node = node;
        graphGroup.add(mesh);
        nodeMeshes.push(mesh);

        if (node.isActive) {
            const haloGeometry = new THREE.SphereGeometry(radius * 1.85, 24, 16);
            const haloMaterial = new THREE.MeshBasicMaterial({
                color: 0xd9fff7,
                transparent: true,
                opacity: 0.12,
                depthWrite: false,
            });
            const halo = new THREE.Mesh(haloGeometry, haloMaterial);
            halo.position.copy(mesh.position);
            graphGroup.add(halo);
        }

        if (shouldShowLabel(node)) {
            const label = createLabelSprite(THREE, node.label, node.isActive);
            label.position.set(node.x, node.y + radius + 14, node.z);
            graphGroup.add(label);
        }
    });

    const defaultZoom = computeCameraDistance(nodes);

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
        zoom: defaultZoom,
        targetZoom: defaultZoom,
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
            state.targetZoom = defaultZoom;
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

function layoutNodes(nodes, edges) {
    const byId = new Map(nodes.map((node) => [node.id, node]));
    const connectedChunksByEntity = new Map();
    const roots = nodes.filter((node) => node.type === 'root');
    const docs = nodes.filter((node) => node.type === 'doc');
    const macros = nodes.filter((node) => node.type === 'macro');
    const mediums = nodes.filter((node) => node.type === 'medium');
    const micros = nodes.filter((node) => node.type === 'micro');
    const entities = nodes.filter((node) => node.type === 'entity');
    const chunkNodes = [...macros, ...mediums, ...micros];

    roots.forEach((node) => setPosition(node, 0, 0, 0));
    placeRelatedDocuments(docs);

    const macroByChunkIndex = new Map(macros.map((node) => [node.chunk_index, node]));
    const mediumByChunkIndex = new Map(mediums.map((node) => [node.chunk_index, node]));
    const usedSemanticLayout = placeSemanticHints(chunkNodes);

    if (!usedSemanticLayout) {
        placePrimaryClusters(macros, 210, -10, -20);

        const orphanMediums = [];
        mediums.forEach((node) => {
            const parent = macroByChunkIndex.get(node.parent_chunk_index);
            if (!parent) {
                orphanMediums.push(node);
                return;
            }
            placeChildNearParent(node, parent, node.chunk_index, 58, 26, -18);
        });
        placePrimaryClusters(orphanMediums, 260, -34, -60, 0.42);

        const orphanMicros = [];
        micros.forEach((node) => {
            const parent = mediumByChunkIndex.get(node.parent_chunk_index) || macroByChunkIndex.get(node.parent_chunk_index);
            if (!parent) {
                orphanMicros.push(node);
                return;
            }
            placeChildNearParent(node, parent, node.chunk_index, 44, 20, -30);
        });
        placePrimaryClusters(orphanMicros, 300, -70, -100, 0.68);
    }

    edges.forEach((edge) => {
        const source = byId.get(edge.source);
        const target = byId.get(edge.target);
        if (!source || !target) return;
        const entity = source.type === 'entity' ? source : (target.type === 'entity' ? target : null);
        const chunk = source.type !== 'entity' && source.type !== 'doc' && source.type !== 'root'
            ? source
            : (target.type !== 'entity' && target.type !== 'doc' && target.type !== 'root' ? target : null);
        if (!entity || !chunk) return;
        const list = connectedChunksByEntity.get(entity.id) || [];
        list.push(chunk);
        connectedChunksByEntity.set(entity.id, list);
    });

    const looseEntities = [];
    entities.forEach((node, index) => {
        const chunks = connectedChunksByEntity.get(node.id) || [];
        if (chunks.length === 0) {
            looseEntities.push(node);
            return;
        }
        const anchor = averagePosition(chunks);
        const angle = seededAngle(node.id);
        const lift = 42 + (index % 5) * 10;
        setPosition(
            node,
            anchor.x + Math.cos(angle) * (54 + (index % 4) * 12),
            anchor.y + lift,
            anchor.z + Math.sin(angle) * (50 + (index % 3) * 14),
        );
    });
    placePrimaryClusters(looseEntities, 310, 96, 70, 0.18);
}

function placeSemanticHints(nodes) {
    const hinted = nodes.filter((node) => Array.isArray(node.layout_hint) && node.layout_hint.length >= 2);
    if (hinted.length < Math.max(4, Math.floor(nodes.length * 0.45))) return false;

    const clusterIds = Array.from(new Set(hinted.map((node) => Number(node.cluster_id ?? -1)))).sort((a, b) => a - b);
    const clusterOffset = new Map();
    clusterIds.forEach((clusterId, index) => {
        if (clusterId < 0) {
            clusterOffset.set(clusterId, { x: 0, z: 150 });
            return;
        }
        const angle = goldenAngle(index);
        const radius = 130 + Math.sqrt(index + 1) * 48;
        clusterOffset.set(clusterId, {
            x: Math.cos(angle) * radius,
            z: Math.sin(angle) * radius * 0.78,
        });
    });

    hinted.forEach((node) => {
        const clusterId = Number(node.cluster_id ?? -1);
        const offset = clusterOffset.get(clusterId) || { x: 0, z: 0 };
        const hint = node.layout_hint;
        const distance = Number(node.cluster_distance || 0);
        const levelLift = node.type === 'macro' ? 34 : (node.type === 'medium' ? 0 : -30);
        setPosition(
            node,
            offset.x + Number(hint[0] || 0) * 380 + seededUnit(node.id) * 36,
            levelLift - Math.min(42, distance * 120),
            offset.z + Number(hint[1] || 0) * 280 + Number(hint[2] || 0) * 120,
        );
    });

    nodes
        .filter((node) => !hinted.includes(node))
        .forEach((node, index) => {
            const angle = goldenAngle(index);
            setPosition(node, Math.cos(angle) * 260, -50, Math.sin(angle) * 180 - 90);
        });

    return true;
}

function setPosition(node, x, y, z) {
    node.x = x;
    node.y = y;
    node.z = z;
}

function placePrimaryClusters(nodes, radius, yOffset, zOffset, phase = 0) {
    if (nodes.length === 0) return;

    nodes.forEach((node, index) => {
        const angle = goldenAngle(index) + phase;
        const band = Math.sqrt(index + 1) / Math.sqrt(nodes.length + 1);
        const jitter = seededUnit(node.id) * 28;
        setPosition(
            node,
            Math.cos(angle) * (radius * band + jitter),
            yOffset + ((index % 5) - 2) * 16,
            Math.sin(angle) * (radius * 0.72 * band + jitter) + zOffset,
        );
    });
}

function placeRelatedDocuments(nodes) {
    nodes.forEach((node, index) => {
        const angle = goldenAngle(index) + 0.9;
        setPosition(node, Math.cos(angle) * 360, 78 + (index % 3) * 18, Math.sin(angle) * 210 - 150);
    });
}

function placeChildNearParent(node, parent, seed, radius, yStep, zOffset) {
    const angle = seededAngle(`${node.id}:${seed}`);
    const distance = radius + (seed % 7) * 7;
    setPosition(
        node,
        parent.x + Math.cos(angle) * distance,
        parent.y - yStep + ((seed % 5) - 2) * 7,
        parent.z + Math.sin(angle) * distance * 0.72 + zOffset,
    );
}

function averagePosition(nodes) {
    const total = nodes.reduce((acc, node) => ({
        x: acc.x + node.x,
        y: acc.y + node.y,
        z: acc.z + node.z,
    }), { x: 0, y: 0, z: 0 });
    const count = Math.max(1, nodes.length);
    return { x: total.x / count, y: total.y / count, z: total.z / count };
}

function goldenAngle(index) {
    return index * 2.399963229728653;
}

function seededAngle(value) {
    return seededUnit(value) * Math.PI * 2;
}

function seededUnit(value) {
    let hash = 2166136261;
    const text = String(value);
    for (let i = 0; i < text.length; i++) {
        hash ^= text.charCodeAt(i);
        hash = Math.imul(hash, 16777619);
    }
    return ((hash >>> 0) % 10000) / 10000;
}

function getVisualRadius(node) {
    if (node.isActive) return Math.max(11, Math.min(18, (node.radius || 10) * 0.72));
    if (node.type === 'root') return 14;
    if (node.type === 'macro') return 10;
    if (node.type === 'medium') return 7.5;
    if (node.type === 'micro') return 5.2;
    if (node.type === 'entity') return 5.8;
    return 8;
}

function shouldShowLabel(node) {
    return node.isActive || node.type === 'root' || (node.type === 'macro' && node.radius >= 16);
}

function createLabelSprite(THREE, text, isActive) {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    const label = truncate(String(text || ''), isActive ? 34 : 24);
    ctx.font = isActive ? '700 22px sans-serif' : '600 18px sans-serif';
    const width = Math.ceil(ctx.measureText(label).width + 28);
    canvas.width = Math.max(96, width);
    canvas.height = 38;
    ctx.font = isActive ? '700 22px sans-serif' : '600 18px sans-serif';
    ctx.fillStyle = isActive ? 'rgba(10, 22, 20, 0.82)' : 'rgba(10, 16, 18, 0.56)';
    roundRect(ctx, 0, 0, canvas.width, canvas.height, 8);
    ctx.fill();
    ctx.fillStyle = isActive ? '#f3fffb' : '#dce9e5';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(label, canvas.width / 2, canvas.height / 2);
    const texture = new THREE.CanvasTexture(canvas);
    const material = new THREE.SpriteMaterial({ map: texture, transparent: true });
    const sprite = new THREE.Sprite(material);
    sprite.scale.set(canvas.width * 0.38, canvas.height * 0.38, 1);
    return sprite;
}

function computeCameraDistance(nodes) {
    const maxRadius = nodes.reduce((max, node) => {
        const distance = Math.sqrt((node.x || 0) ** 2 + (node.y || 0) ** 2 + (node.z || 0) ** 2);
        return Math.max(max, distance);
    }, 280);
    return Math.max(420, Math.min(1250, maxRadius * 2.35));
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
