const THREE_MODULE_URL = '/vendor/three/three.module.js';

const NODE_COLORS = {
    root: 0xffe6ff,
    doc: 0x8ea4b7,
    macro: 0xff9a9e,
    medium: 0x76f4ff,
    micro: 0xcfeaff,
    entity: 0xffc0c6,
};

const EDGE_COLORS = {
    default: 0xb9c7d8,
    hierarchy: 0xd8b4ff,
    active: 0xfff0ff,
    relation: 0x9be8ff,
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
    scene.fog = new THREE.FogExp2(0x07101d, 0.00052);
    const camera = new THREE.PerspectiveCamera(50, 1, 1, 5000);
    const graphGroup = new THREE.Group();
    scene.add(graphGroup);

    const ambientLight = new THREE.AmbientLight(0xe8f3ff, 0.74);
    const keyLight = new THREE.DirectionalLight(0xffffff, 0.7);
    const fillLight = new THREE.DirectionalLight(0x9be8ff, 0.2);
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
    relaxNodeCollisions(nodes);
    recenterNodes(nodes);
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
                opacity: isActiveEdge ? 0.6 : (isHierarchy ? 0.26 : 0.16),
                depthWrite: false,
            }));
        }
        return edgeMaterials.get(key);
    };

    edges.forEach((edge) => {
        const source = nodeById.get(edge.source);
        const target = nodeById.get(edge.target);
        if (!source || !target) return;
        if (!hasFinitePosition(source) || !hasFinitePosition(target)) return;

        const geometry = new THREE.BufferGeometry().setFromPoints([
            new THREE.Vector3(source.x, source.y, source.z),
            new THREE.Vector3(target.x, target.y, target.z),
        ]);
        const line = new THREE.Line(geometry, getEdgeMaterial(edge));
        graphGroup.add(line);
    });

    const geometries = new Map();
    nodes.forEach((node) => {
        if (!hasFinitePosition(node)) setPosition(node, 0, 0, 0);
        const radius = getVisualRadius(node);
        const geometryKey = Math.round(radius);
        let geometry = geometries.get(geometryKey);
        if (!geometry) {
            geometry = new THREE.SphereGeometry(radius, 14, 10);
            geometries.set(geometryKey, geometry);
        }

        const material = new THREE.MeshStandardMaterial({
            color: NODE_COLORS[node.type] || NODE_COLORS.doc,
            roughness: 0.78,
            metalness: 0.0,
            emissive: NODE_COLORS[node.type] || NODE_COLORS.doc,
            emissiveIntensity: node.isActive ? 0.34 : 0.08,
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
                opacity: 0.18,
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
        rotationX: -0.08,
        rotationY: 0.18,
        targetRotationX: -0.08,
        targetRotationY: 0.18,
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
            if (options.inspectOnSelect || event.shiftKey || event.altKey || !options.onFocusNode) {
                showChunkInspector(canvasWrapper, node, options.chunks || [], options.language, {
                    sidePanel: Boolean(options.inspectOnSelect),
                });
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
            state.targetRotationX = -0.08;
            state.targetRotationY = 0.18;
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
    placeRadialDocumentStructure(chunkNodes);
    placeRelatedDocuments(docs);

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
        const lift = 10 + (index % 4) * 8;
        setPosition(
            node,
            anchor.x + Math.cos(angle) * (42 + (index % 5) * 13),
            anchor.y + lift,
            anchor.z + Math.sin(angle) * (40 + (index % 4) * 12),
        );
    });
    placePrimaryClusters(looseEntities, 560, 42, 0, 0.18);
}

function relaxNodeCollisions(nodes) {
    const colliders = nodes.filter((node) => node.type !== 'doc' && hasFinitePosition(node));
    if (colliders.length < 2) return;

    const iterations = colliders.length > 420 ? 22 : (colliders.length > 260 ? 30 : 42);
    for (let iteration = 0; iteration < iterations; iteration++) {
        let moved = false;
        for (let i = 0; i < colliders.length; i++) {
            const a = colliders[i];
            for (let j = i + 1; j < colliders.length; j++) {
                const b = colliders[j];
                const rawDx = finiteNumber(b.x) - finiteNumber(a.x);
                const rawDz = finiteNumber(b.z) - finiteNumber(a.z);
                const samePoint = Math.abs(rawDx) < 0.001 && Math.abs(rawDz) < 0.001;
                const fallbackAngle = seededAngle(`${a.id}:${b.id}:collision`);
                const dx = samePoint ? Math.cos(fallbackAngle) * 0.001 : rawDx;
                const dz = samePoint ? Math.sin(fallbackAngle) * 0.001 : rawDz;
                const distance = Math.sqrt(dx * dx + dz * dz);
                const minDistance = collisionRadius(a) + collisionRadius(b);
                if (distance >= minDistance) continue;

                const overlap = (minDistance - distance) * 0.52;
                const nx = dx / distance;
                const nz = dz / distance;
                const aLocked = a.type === 'root';
                const bLocked = b.type === 'root';

                if (!aLocked) {
                    const share = bLocked ? 1 : 0.5;
                    a.x -= nx * overlap * share;
                    a.z -= nz * overlap * share;
                    a.y += ySeparation(a, b, iteration) * share;
                }
                if (!bLocked) {
                    const share = aLocked ? 1 : 0.5;
                    b.x += nx * overlap * share;
                    b.z += nz * overlap * share;
                    b.y -= ySeparation(b, a, iteration) * share;
                }
                moved = true;
            }
        }
        if (!moved) break;
    }
}

function collisionRadius(node) {
    const base = getVisualRadius(node);
    if (node.type === 'root') return base * 2.6 + 18;
    if (node.type === 'macro') return base * 2.45 + 15;
    if (node.type === 'medium') return base * 2.3 + 12;
    if (node.type === 'entity') return base * 2.15 + 13;
    return base * 2.0 + 9;
}

function ySeparation(node, other, iteration) {
    const direction = seededUnit(`${node.id}:${other.id}`) > 0.5 ? 1 : -1;
    const damping = iteration < 8 ? 1 : 0.35;
    return direction * damping * 0.8;
}

function recenterNodes(nodes) {
    if (nodes.length === 0) return;
    const movable = nodes.filter((node) => node.type !== 'root');
    if (movable.length === 0) return;
    const center = averagePosition(movable);
    movable.forEach((node) => {
        node.x -= center.x * 0.42;
        node.z -= center.z * 0.42;
    });
}

function placeRadialDocumentStructure(nodes) {
    if (nodes.length === 0) return;

    const groups = groupStructureBranches(nodes);
    const branchCount = groups.length;
    const baseRadius = branchCount > 24 ? 170 : 145;
    const radialStep = branchCount > 24 ? 34 : 42;
    const tangentSpread = branchCount > 24 ? 34 : 46;

    groups.forEach((group, branchIndex) => {
        const angle = (branchIndex / branchCount) * Math.PI * 2 - Math.PI / 2;
        const tangent = angle + Math.PI / 2;
        const ordered = group.nodes
            .slice()
            .sort((a, b) => graphNodeRank(a) - graphNodeRank(b) || ((a.chunk_index ?? 0) - (b.chunk_index ?? 0)));
        const branchDepth = Math.max(1, Math.ceil(Math.sqrt(ordered.length)));

        ordered.forEach((node, localIndex) => {
            const level = graphNodeLevel(node);
            const ring = Math.floor(localIndex / branchDepth);
            const lane = localIndex % branchDepth;
            const laneCenter = (branchDepth - 1) / 2;
            const semanticNudge = layoutHintValue(node.layout_hint, 0) * 18 + layoutHintValue(node.layout_hint, 1) * 12;
            const radius = baseRadius + level * 82 + ring * radialStep + Math.max(0, group.nodes.length - 12) * 1.3;
            const side = (lane - laneCenter) * tangentSpread + semanticNudge;
            const lift = level === 0 ? 18 : (level === 1 ? 0 : -18);

            setPosition(
                node,
                Math.cos(angle) * radius + Math.cos(tangent) * side,
                lift + ((localIndex % 3) - 1) * 4,
                Math.sin(angle) * radius * 0.76 + Math.sin(tangent) * side * 0.8,
            );
        });
    });
}

function groupStructureBranches(nodes) {
    const byCluster = new Map();
    const byMacro = new Map();

    nodes.forEach((node) => {
        const clusterId = Number(node.cluster_id ?? Number.NaN);
        if (Number.isFinite(clusterId) && clusterId >= 0) {
            const key = `cluster:${clusterId}`;
            const list = byCluster.get(key) || [];
            list.push(node);
            byCluster.set(key, list);
            return;
        }

        const macroKey = node.type === 'macro'
            ? node.chunk_index
            : (node.macro_index ?? node.parent_chunk_index ?? node.chunk_index);
        const key = `macro:${macroKey ?? node.id}`;
        const list = byMacro.get(key) || [];
        list.push(node);
        byMacro.set(key, list);
    });

    const source = byCluster.size >= 2 ? byCluster : byMacro;
    return Array.from(source.entries())
        .map(([key, branchNodes]) => ({ key, nodes: branchNodes }))
        .sort((a, b) => {
            const aIndex = Math.min(...a.nodes.map((node) => node.chunk_index ?? Number.MAX_SAFE_INTEGER));
            const bIndex = Math.min(...b.nodes.map((node) => node.chunk_index ?? Number.MAX_SAFE_INTEGER));
            return aIndex - bIndex || a.key.localeCompare(b.key);
        });
}

function graphNodeRank(node) {
    return graphNodeLevel(node) * 100000 + (node.chunk_index ?? 0);
}

function graphNodeLevel(node) {
    if (node.type === 'macro') return 0;
    if (node.type === 'medium') return 1;
    if (node.type === 'micro') return 2;
    return 1;
}

function setPosition(node, x, y, z) {
    node.x = finiteNumber(x);
    node.y = finiteNumber(y);
    node.z = finiteNumber(z);
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
        setPosition(node, Math.cos(angle) * 650, 44 + (index % 3) * 14, Math.sin(angle) * 460);
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
    const validNodes = nodes.filter(hasFinitePosition);
    const total = validNodes.reduce((acc, node) => ({
        x: acc.x + node.x,
        y: acc.y + node.y,
        z: acc.z + node.z,
    }), { x: 0, y: 0, z: 0 });
    const count = Math.max(1, validNodes.length);
    return { x: total.x / count, y: total.y / count, z: total.z / count };
}

function layoutHintValue(layoutHint, index) {
    if (!Array.isArray(layoutHint)) return 0;
    return finiteNumber(layoutHint[index]);
}

function finiteNumber(value, fallback = 0) {
    const number = Number(value);
    return Number.isFinite(number) ? number : fallback;
}

function hasFinitePosition(node) {
    return Number.isFinite(node?.x) && Number.isFinite(node?.y) && Number.isFinite(node?.z);
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
    if (node.isActive) return Math.max(10, Math.min(16, (node.radius || 10) * 0.62));
    if (node.type === 'root') return 12;
    if (node.type === 'macro') return 8.5;
    if (node.type === 'medium') return 6.2;
    if (node.type === 'micro') return 3.8;
    if (node.type === 'entity') return 5.0;
    return 7;
}

function shouldShowLabel(node) {
    return node.isActive || node.type === 'root';
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
        const distance = Math.sqrt(finiteNumber(node.x) ** 2 + finiteNumber(node.y) ** 2 + finiteNumber(node.z) ** 2);
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
        byType.macro ? `<span style="color:#ffb3b8;">• ${byType.macro} ${isRu ? 'разделов' : 'sections'}</span>` : '',
        byType.medium ? `<span style="color:#76f4ff;">• ${byType.medium} ${isRu ? 'подразделов' : 'subsections'}</span>` : '',
        byType.entity ? `<span style="color:#ffc0c6;">• ${byType.entity} ${isRu ? 'сущностей' : 'entities'}</span>` : '',
    ].filter(Boolean).join(' ');
    return;
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
        <span><span class="graph-legend-dot" style="background:#ffe6ff;color:#ffe6ff;"></span>${isRu ? 'Документ' : 'Document'}</span>
        <span><span class="graph-legend-dot" style="background:#ff9a9e;color:#ff9a9e;"></span>${isRu ? 'Раздел' : 'Section'}</span>
        <span><span class="graph-legend-dot" style="background:#76f4ff;color:#76f4ff;"></span>${isRu ? 'Подраздел' : 'Subsection'}</span>
        <span><span class="graph-legend-dot" style="background:#ffc0c6;color:#ffc0c6;"></span>${isRu ? 'Сущность' : 'Entity'}</span>
        <span><span class="graph-legend-line" style="border-color:rgba(216,180,255,0.58);"></span>${isRu ? 'Связь / иерархия' : 'Link / hierarchy'}</span>
    `;
    return;
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

function showChunkInspector(parent, node, chunks, language, options = {}) {
    const chunk = chunks.find((item) => item.chunk_index === node.chunk_index);
    if (!chunk) return;

    const existing = parent.querySelector('.graph-inspector');
    if (existing) existing.remove();

    const isRu = language === 'ru';
    const header = Array.isArray(chunk.header_chain) ? chunk.header_chain.join(' > ') : '';
    const entities = Array.isArray(chunk.entities) ? chunk.entities.join(', ') : '';
    const inspector = document.createElement('div');
    inspector.className = options.sidePanel ? 'graph-inspector graph-inspector-side' : 'graph-inspector';
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
