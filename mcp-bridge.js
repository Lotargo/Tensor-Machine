#!/usr/bin/env node

const http = require('http');
const readline = require('readline');

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false
});

let sequence_number = 1;
const session_id = "lmstudio_session_" + Math.random().toString(36).substring(2, 10);

rl.on('line', (line) => {
  if (!line.trim()) return;
  try {
    const msg = JSON.parse(line);
    handleMessage(msg);
  } catch (err) {
    sendError(null, -32700, "Parse error: " + err.message);
  }
});

function handleMessage(msg) {
  if (msg.jsonrpc !== "2.0") {
    return sendError(msg.id, -32600, "Invalid Request");
  }

  switch (msg.method) {
    case "initialize":
      sendResponse(msg.id, {
        protocolVersion: "2024-11-05",
        capabilities: {
          tools: {}
        },
        serverInfo: {
          name: "tensor-rag-bridge",
          version: "0.1.0"
        }
      });
      break;

    case "notifications/initialized":
      // No response required
      break;

    case "tools/list":
      sendResponse(msg.id, {
        tools: [
          {
            name: "search",
            description: "PRIMARY RETRIEVAL TOOL: Use this tool first to find specific information, answers, quotes, or passages in the document library. This is the most reliable tool because it retrieves focused passages and avoids token limits. Formulate the query in the SAME language as the user's message/question (e.g., Russian, English, Chinese). DO NOT use question words (such as 'what', 'how', 'why', 'что', 'как', 'почему'). Always query using only descriptive search keywords/phrases in the matching language. Examples: 'Arrow Flight connection speed' (English) or 'скорость подключения Arrow Flight' (Russian). You can optionally filter results to a specific document by providing its `doc_uuid`.",
            inputSchema: {
              type: "object",
              properties: {
                query: {
                  type: "string",
                  description: "Text search query in the same language as the user's input. Do NOT include question words (like 'how', 'what', 'why', 'что', 'как', 'почему'). Examples: 'алгоритм ColBERT MaxSim' for Russian queries, or 'ColBERT MaxSim algorithm' for English queries."
                },
                final_top_k: {
                  type: "integer",
                  description: "Number of top documents to retrieve. Values above the configured server-side limit are clamped automatically.",
                  default: 5
                },
                doc_uuid: {
                  type: "string",
                  description: "Optional UUID of a specific document to search within. If omitted, searches across all documents."
                },
                alpha: {
                  type: "number",
                  description: "Optional alpha value [0.0 - 1.0] for hybrid search weighting. 1.0 is pure semantic, 0.0 is pure BM25 lexical search. If omitted, uses standard mode.",
                  minimum: 0.0,
                  maximum: 1.0
                },
                dsl: {
                  type: "string",
                  description: "Optional declarative S-expression query DSL"
                }
              },
              required: ["query"]
            }
          },
          {
            name: "read_document",
            description: "CRITICAL WARNING: DO NOT USE THIS TOOL for large documents (e.g. books, logs, long reports) as it will exceed the token budget and FAIL with a limit exceeded warning. Use this tool ONLY for reading very small files in full (e.g., under 1-2 pages). For retrieving specific facts, answers, or quotes from ANY document (including large ones), ALWAYS use the 'search' tool instead.",
            inputSchema: {
              type: "object",
              properties: {
                doc_uuid: {
                  type: "string",
                  description: "UUID of the indexed document to read in full."
                }
              },
              required: ["doc_uuid"]
            }
          },
          {
            name: "list_documents",
            description: "Retrieve a list of all indexed documents in the database, showing their filenames and unique UUIDs. Use this tool first if you want to find a document's doc_uuid to run a filtered search.",
            inputSchema: {
              type: "object",
              properties: {}
            }
          },
          {
            name: "get_document_metadata",
            description: "Retrieve metadata, anchors (headings), and relationships (edges) for a specific document by its doc_uuid.",
            inputSchema: {
              type: "object",
              properties: {
                doc_uuid: {
                  type: "string",
                  description: "UUID of the document."
                }
              },
              required: ["doc_uuid"]
            }
          },
          {
            name: "exec",
            description: "Execute a declarative DSL S-expression command for side-effects, such as adding metadata, anchors, or edges.",
            inputSchema: {
              type: "object",
              properties: {
                dsl: {
                  type: "string",
                  description: "DSL S-expression command to execute, e.g. (add-metadata :doc-uuid \"...\" :key \"...\" :value \"...\")"
                }
              },
              required: ["dsl"]
            }
          },
          {
            name: "help",
            description: "Retrieve comprehensive English guides, documentation, and query DSL examples for the hybrid search RAG engine.",
            inputSchema: {
              type: "object",
              properties: {}
            }
          }
        ]
      });
      break;

    case "tools/call":
      if (msg.params.name === "search" || msg.params.name === "read_document" || msg.params.name === "list_documents" || msg.params.name === "get_document_metadata" || msg.params.name === "exec" || msg.params.name === "help") {
        callTool(msg.id, msg.params.name, msg.params.arguments);
      } else {
        sendError(msg.id, -32601, "Method not found: tool " + msg.params.name);
      }
      break;

    default:
      if (msg.id !== undefined) {
        sendError(msg.id, -32601, "Method not found: " + msg.method);
      }
  }
}

function callTool(id, toolName, args) {
  const backendTool = toolName === "search" ? "search_text" : toolName;
  const payload = JSON.stringify({
    tool: backendTool,
    args: args || {},
    session_id: session_id,
    sequence_number: sequence_number++
  });

  const req = http.request({
    hostname: 'localhost',
    port: 8080,
    path: '/mcp',
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Content-Length': Buffer.byteLength(payload)
    }
  }, (res) => {
    let data = '';
    res.on('data', (chunk) => { data += chunk; });
    res.on('end', () => {
      try {
        if (res.statusCode !== 200) {
          return sendError(id, -32603, `HTTP Error ${res.statusCode}: ${data}`);
        }
        const respObj = JSON.parse(data);
        if (respObj.status !== 200) {
          return sendError(id, -32603, `Server Error: Status ${respObj.status}`);
        }
        // Decode bytes array to string
        const textContent = Buffer.from(respObj.body).toString('utf8');
        sendResponse(id, {
          content: [
            {
              type: "text",
              text: textContent
            }
          ]
        });
      } catch (err) {
        sendError(id, -32603, "Internal error parsing bridge response: " + err.message);
      }
    });
  });

  req.on('error', (err) => {
    sendError(id, -32603, "Failed to connect to local RAG server: " + err.message);
  });

  req.write(payload);
  req.end();
}

function sendResponse(id, result) {
  console.log(JSON.stringify({
    jsonrpc: "2.0",
    result: result,
    id: id
  }));
}

function sendError(id, code, message) {
  console.log(JSON.stringify({
    jsonrpc: "2.0",
    error: { code, message },
    id: id
  }));
}
