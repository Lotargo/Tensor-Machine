local protocol = require("client.protocol")
local ffi = require("ffi")

ffi.cdef[[
    typedef int ssize_t;
    int socket(int domain, int type, int protocol);
    int connect(int sockfd, const struct sockaddr *addr, unsigned int addrlen);
    ssize_t send(int sockfd, const void *buf, size_t len, int flags);
    ssize_t recv(int sockfd, void *buf, size_t len, int flags);
    int close(int fd);

    uint16_t htons(uint16_t hostshort);
    uint32_t inet_addr(const char *cp);

    struct sockaddr_in {
        short            sin_family;
        unsigned short   sin_port;
        struct { uint32_t s_addr; } sin_addr;
        char             sin_zero[8];
    };
]]

local g_connection_fd = 0

local function read_exactly(fd, len)
    local C = ffi.C
    local buf = ffi.new("char[?]", len)
    local total_read = 0
    while total_read < len do
        local recvd = C.recv(fd, buf + total_read, len - total_read, 0)
        if recvd <= 0 then
            return nil
        end
        total_read = total_read + recvd
    end
    return ffi.string(buf, len)
end

local function ensure_connected()
    if g_connection_fd > 0 then
        return g_connection_fd
    end
    local C = ffi.C
    local fd = C.socket(2, 1, 0)
    if fd < 0 then
        error("Failed to create socket")
    end
    local addr = ffi.new("struct sockaddr_in")
    addr.sin_family = 2
    addr.sin_port = C.htons(8443)
    addr.sin_addr.s_addr = C.inet_addr("127.0.0.1")
    local ret = C.connect(fd, ffi.cast("const struct sockaddr *", addr), ffi.sizeof(addr))
    if ret < 0 then
        C.close(fd)
        error("Failed to connect to router on port 8443")
    end
    g_connection_fd = fd
    return fd
end

local function close_connection()
    if g_connection_fd > 0 then
        ffi.C.close(g_connection_fd)
        g_connection_fd = 0
    end
end

local function send_to_router(opcode, payload)
    local msg = protocol.pack_message(opcode, payload)

    local C = ffi.C
    local fd = ensure_connected()

    local total_sent = 0
    local msg_buf = ffi.cast("const char*", msg)
    while total_sent < #msg do
        local sent = C.send(fd, msg_buf + total_sent, #msg - total_sent, 0)
        if sent < 0 then
            close_connection()
            error("Failed to send data")
        end
        total_sent = total_sent + sent
    end

    local header = read_exactly(fd, 5)
    if not header then
        close_connection()
        return nil
    end

    local resp_payload_size = protocol.unpack_uint32_le(header, 2)
    if resp_payload_size > 1024 * 1024 * 100 then
        close_connection()
        error("Response payload too large")
    end
    local payload_data = read_exactly(fd, resp_payload_size)
    if not payload_data then
        close_connection()
        return nil
    end

    local crc_bytes = read_exactly(fd, 2)

    local full_msg = header .. payload_data
    local expected_crc = protocol.crc16(full_msg)
    local actual_crc = protocol.unpack_uint16_le(crc_bytes)
    if expected_crc ~= actual_crc then
        close_connection()
        error("CRC mismatch: expected " .. expected_crc .. ", got " .. actual_crc)
    end

    return full_msg .. crc_bytes
end

local function parse_matrix_string(str)
    -- Very basic parser for "[1 2; 3 4]" -> TCBP binary
    -- We assume 2D matrices here.
    str = string.gsub(str, "%[", "")
    str = string.gsub(str, "%]", "")

    local rows = {}
    local r = 1
    for row_str in string.gmatch(str, "([^;]+)") do
        local row = {}
        local c = 1
        for num_str in string.gmatch(row_str, "%S+") do
            local num = tonumber(num_str)
            if not num then error("Invalid number in matrix at row " .. r .. ", col " .. c .. ": " .. num_str) end
            table.insert(row, num)
            c = c + 1
        end
        table.insert(rows, row)
        r = r + 1
    end

    local dim1 = #rows
    local dim2 = dim1 > 0 and #rows[1] or 0

    for r=1,dim1 do
        if #rows[r] ~= dim2 then
            error("Matrix is not rectangular: row " .. r .. " has " .. #rows[r] .. " elements, expected " .. dim2)
        end
    end

    if dim1 == 0 or dim2 == 0 then
        error("Matrix must have non-zero dimensions")
    end

    -- We need to serialize this into TCBP Float64 bytes
    -- Since LuaJIT FFI supports double arrays, this is easy
    local num_elements = dim1 * dim2
    local arr = ffi.new("double[?]", num_elements)
    local idx = 0
    for r=1,dim1 do
        for c=1,dim2 do
            arr[idx] = rows[r][c]
            idx = idx + 1
        end
    end

    local header = string.char(2) .. protocol.pack_uint32_le(dim1) .. protocol.pack_uint32_le(dim2)
    local data = ffi.string(arr, num_elements * 8)
    return header .. data
end

local function print_help()
    print("turbo-calc CLI")
    print("Usage: luajit main.lua <command> [arguments]")
    print("Commands:")
    print("  eval \"<expression>\"    - Evaluate math expression")
    print("  matrix \"<expression>\"  - Evaluate matrix expression e.g. \"[1 2; 3 4]\"")
    print("  solve \"<expression>\"   - Solve logic expression")
    print("  sym \"<func>,<var>\"     - Evaluate symbolic differentiation")
    print("  csp \"<expression>\"     - Solve CSP logic expression")
    print("  graph \"<nodes>,<ptr1...>,<col1...>,<val1...>\" - Run Dijkstra on CSR graph")
    print("  dsp \"<r1,i1,r2,i2...>\" - Run DFT on complex vector")
    print("  qsim \"<r1,i1,r2,i2...>\" - Apply Hadamard on state vector")
end

local cmd = arg[1]

if not cmd or cmd == "--help" then
    print_help()
    os.exit(0)
end

local payload_str = arg[2] or ""

local opcode, payload
if cmd == "eval" then
    opcode = protocol.OP_EVAL
    payload = payload_str
elseif cmd == "matrix" then
    opcode = protocol.OP_MATRIX
    payload = parse_matrix_string(payload_str)
elseif cmd == "solve" then
    opcode = protocol.OP_SOLVE
    payload = payload_str
elseif cmd == "sym" then
    opcode = protocol.OP_SYM
    payload = payload_str
elseif cmd == "csp" then
    opcode = protocol.OP_CSP
    payload = payload_str
elseif cmd == "graph" then
    opcode = protocol.OP_GRAPH
    -- Extremely naive parsing for testing graph: nodes, ptrs..., cols..., vals...
    local parts = {}
    for w in string.gmatch(payload_str, "([^,]+)") do table.insert(parts, tonumber(w)) end
    local num_nodes = parts[1]
    local num_ptrs = num_nodes + 1
    local row_ptrs = {}
    for i=1,num_ptrs do table.insert(row_ptrs, parts[1+i]) end
    local nnz = row_ptrs[num_ptrs]
    local col_indices = {}
    for i=1,nnz do table.insert(col_indices, parts[1+num_ptrs+i]) end
    local data = {}
    for i=1,nnz do table.insert(data, parts[1+num_ptrs+nnz+i]) end
    payload = protocol.pack_csr_payload(num_nodes, num_nodes, row_ptrs, col_indices, data)
elseif cmd == "dsp" then
    opcode = protocol.OP_DSP
    local parts = {}
    for w in string.gmatch(payload_str, "([^,]+)") do table.insert(parts, tonumber(w)) end
    local r_parts = {}
    local i_parts = {}
    for i=1,#parts,2 do
        table.insert(r_parts, parts[i])
        table.insert(i_parts, parts[i+1])
    end
    payload = protocol.pack_complex_payload(r_parts, i_parts)
elseif cmd == "qsim" then
    opcode = protocol.OP_QSIM
    local parts = {}
    for w in string.gmatch(payload_str, "([^,]+)") do table.insert(parts, tonumber(w)) end
    local r_parts = {}
    local i_parts = {}
    for i=1,#parts,2 do
        table.insert(r_parts, parts[i])
        table.insert(i_parts, parts[i+1])
    end
    payload = protocol.pack_complex_payload(r_parts, i_parts)
else
    print("Unknown command: " .. cmd)
    os.exit(1)
end

local ok, response = pcall(send_to_router, opcode, payload)
if not ok then
    print("Error communicating with router: " .. tostring(response))
    os.exit(1)
end

if response then
    local resp_opcode = string.byte(response, 1)
    local resp_payload_size = protocol.unpack_uint32_le(response, 2)
    local resp_payload = string.sub(response, 6, 5 + resp_payload_size)
    if resp_opcode ~= opcode then
        print("Error: Server returned protocol error: " .. resp_payload)
        os.exit(1)
    end
    if opcode == protocol.OP_MATRIX then
        -- Deserialize matrix response
        local num_dims = string.byte(resp_payload, 1)
        if num_dims == 2 then
            local r_dim1 = protocol.unpack_uint32_le(resp_payload, 2)
            local r_dim2 = protocol.unpack_uint32_le(resp_payload, 6)
            local data_bytes = string.sub(resp_payload, 10)
            local num_elems = r_dim1 * r_dim2
            local arr = ffi.new("double[?]", num_elems)
            ffi.copy(arr, data_bytes, num_elems * 8)
            print("Matrix Result (" .. r_dim1 .. "x" .. r_dim2 .. "):")
            local idx = 0
            for r=1,r_dim1 do
                local row_parts = {}
                for c=1,r_dim2 do
                    table.insert(row_parts, arr[idx])
                    idx = idx + 1
                end
                print(table.concat(row_parts, " "))
            end
        else
            print("Received unsupported matrix response dimensions: " .. num_dims)
        end
    elseif opcode == protocol.OP_DSP or opcode == protocol.OP_QSIM or opcode == protocol.OP_GRAPH then
        -- Deserialize complex response
        local num_elems = protocol.unpack_uint32_le(resp_payload, 1)
        local data_bytes = string.sub(resp_payload, 5)
        local arr = ffi.new("double[?]", num_elems * 2)
        ffi.copy(arr, data_bytes, num_elems * 16)
        print("Complex/Array Result:")
        for i=1,num_elems do
            print(arr[(i-1)*2] .. " + " .. arr[(i-1)*2+1] .. "i")
        end
    else
        print("Result: " .. resp_payload)
    end
else
    print("No response from router")
end
