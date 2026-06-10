local protocol = require("client.protocol")

print("Testing CRC16...")
local data = string.char(0x01, 0x04, 0x00, 0x00, 0x00, 0x61, 0x62, 0x63, 0x64)
local crc = protocol.crc16(data)
assert(crc ~= 0, "CRC should not be 0")

print("Testing Little-Endian Pack/Unpack...")
local packed32 = protocol.pack_uint32_le(0x12345678)
assert(#packed32 == 4)
local unpacked32 = protocol.unpack_uint32_le(packed32)
assert(unpacked32 == 0x12345678, "UInt32 mismatch")

local packed16 = protocol.pack_uint16_le(0xABCD)
assert(#packed16 == 2)
-- test string manipulation locally
assert(string.byte(packed16, 1) == 0xCD)
assert(string.byte(packed16, 2) == 0xAB)

print("Testing Pack Message...")
local msg = protocol.pack_message(0x01, "test")
assert(#msg == 1 + 4 + 4 + 2, "Message size mismatch")

print("Testing CSR Payload Packing...")
local row_ptrs = {0, 2, 4}
local col_indices = {0, 1, 0, 1}
local data_vals = {1.0, 2.0, 3.0, 4.0}
local csr_payload = protocol.pack_csr_payload(2, 2, row_ptrs, col_indices, data_vals)
local expected_csr_len = 8 + (3 * 4) + (4 * 4) + (4 * 8)
assert(#csr_payload == expected_csr_len, "CSR Payload size mismatch")

print("Testing Complex128 Payload Packing...")
local r_parts = {1.0, 2.0}
local i_parts = {0.5, 1.5}
local cmp_payload = protocol.pack_complex_payload(r_parts, i_parts)
local expected_cmp_len = 4 + (2 * 16)
assert(#cmp_payload == expected_cmp_len, "Complex Payload size mismatch")

print("Lua Client Tests Passed.")
