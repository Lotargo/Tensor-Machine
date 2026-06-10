-- TCBP Protocol for Lua
local ffi = require("ffi")
local bit = require("bit")

local protocol = {}

-- Opcode constants
protocol.OP_EVAL = 0x01
protocol.OP_MATRIX = 0x02
protocol.OP_SOLVE = 0x03
protocol.OP_SYM = 0x04
protocol.OP_GRAPH = 0x05
protocol.OP_DSP = 0x06
protocol.OP_QSIM = 0x07
protocol.OP_CSP = 0x08

-- CRC16 (CCITT-FALSE) implementation
function protocol.crc16(data)
    local crc = 0xFFFF
    for i = 1, #data do
        local byte = string.byte(data, i)
        crc = bit.bxor(crc, bit.lshift(byte, 8))
        for j = 1, 8 do
            if bit.band(crc, 0x8000) ~= 0 then
                crc = bit.bxor(bit.lshift(crc, 1), 0x1021)
            else
                crc = bit.lshift(crc, 1)
            end
            crc = bit.band(crc, 0xFFFF)
        end
    end
    return crc
end

-- Function to pack an integer to Little-Endian bytes
function protocol.pack_uint32_le(val)
    return string.char(
        bit.band(val, 0xFF),
        bit.band(bit.rshift(val, 8), 0xFF),
        bit.band(bit.rshift(val, 16), 0xFF),
        bit.band(bit.rshift(val, 24), 0xFF)
    )
end

function protocol.pack_uint16_le(val)
    return string.char(
        bit.band(val, 0xFF),
        bit.band(bit.rshift(val, 8), 0xFF)
    )
end

function protocol.unpack_uint32_le(str, offset)
    offset = offset or 1
    return bit.bor(
        string.byte(str, offset),
        bit.lshift(string.byte(str, offset + 1), 8),
        bit.lshift(string.byte(str, offset + 2), 16),
        bit.lshift(string.byte(str, offset + 3), 24)
    )
end

function protocol.unpack_uint16_le(str, offset)
    offset = offset or 1
    return bit.bor(
        string.byte(str, offset),
        bit.lshift(string.byte(str, offset + 1), 8)
    )
end

-- CSR Graph Payload Parser and Packer
-- [Num Rows (4 bytes)] + [Num Cols (4 bytes)] + [Row Pointers (N+1 * 4 bytes)] + [Col Indices (M * 4 bytes)] + [Data (M * 8 bytes)]
function protocol.pack_csr_payload(num_rows, num_cols, row_ptrs, col_indices, data)
    local header = protocol.pack_uint32_le(num_rows) .. protocol.pack_uint32_le(num_cols)
    local ptrs = {}
    for i=1,num_rows+1 do table.insert(ptrs, protocol.pack_uint32_le(row_ptrs[i])) end
    local ptrs_str = table.concat(ptrs)
    local num_nnz = #data
    local cols = {}
    for i=1,num_nnz do table.insert(cols, protocol.pack_uint32_le(col_indices[i])) end
    local cols_str = table.concat(cols)
    local arr = ffi.new("double[?]", num_nnz)
    for i=1,num_nnz do arr[i-1] = data[i] end
    local data_str = ffi.string(arr, num_nnz * 8)
    return header .. ptrs_str .. cols_str .. data_str
end

-- Complex128 Vector Payload Packer (DSP)
-- [Num Elements (4 bytes)] + [Real/Imag Pairs (N * 16 bytes)]
function protocol.pack_complex_payload(real_parts, imag_parts)
    local num_elems = #real_parts
    local header = protocol.pack_uint32_le(num_elems)
    local arr = ffi.new("double[?]", num_elems * 2)
    for i=1,num_elems do
        arr[(i-1)*2] = real_parts[i]
        arr[(i-1)*2+1] = imag_parts[i]
    end
    return header .. ffi.string(arr, num_elems * 16)
end

-- Pack message
-- [Opcode (1 byte)] + [Payload Size N (4 bytes LE)] + [Payload (N bytes)] + [CRC16 (2 bytes LE)]
function protocol.pack_message(opcode, payload)
    local payload_size = #payload
    local header = string.char(opcode) .. protocol.pack_uint32_le(payload_size)
    local data = header .. payload
    local crc = protocol.crc16(data)
    return data .. protocol.pack_uint16_le(crc)
end

return protocol
