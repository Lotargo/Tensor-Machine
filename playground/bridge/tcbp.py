"""
tcbp.py — TCBP (Turbo-Calc Binary Protocol) client for Python.

Wire format (all values Little-Endian):

  ┌─────────┬──────────────┬──────────┬──────────┐
  │ Opcode  │ Payload Len  │ Payload  │ CRC16    │
  │  1 B    │   4 B LE     │  N B     │  2 B LE  │
  └─────────┴──────────────┴──────────┴──────────┘

CRC16-CCITT (poly 0x1021, init 0xFFFF, CCITT-FALSE variant).
No reflection, no final XOR.

Opcodes (from v10 router/protocol.lisp):
  0x01  Eval      — safe arithmetic eval
  0x02  Matrix    — tensor/matrix ops (delegated to APL)
  0x03  Solve     — Prolog CLP(FD) equation solver
  0x04  Sym       — symbolic differentiation
  0x05  Graph     — Dijkstra shortest path (CSR input)
  0x06  DSP       — DFT/FFT (complex vector)
  0x07  QSim      — quantum state simulation
  0x08  CSP       — Prolog constraint satisfaction
  0x09  Evolve    — v7 AST evolution            (v7 extension)
  0x0A  Censor    — v7 Prolog censor check      (v7 extension)
  0x0B  Formula   — formulas.py eval on server  (v7 extension)
"""

from __future__ import annotations

import socket
import struct
from dataclasses import dataclass
from typing import Optional


class Opcode:
    EVAL = 0x01
    MATRIX = 0x02
    SOLVE = 0x03
    SYM = 0x04
    GRAPH = 0x05
    DSP = 0x06
    QSIM = 0x07
    CSP = 0x08
    EVOLVE = 0x09
    CENSOR = 0x0A
    FORMULA = 0x0B


# ---------------------------------------------------------------------------
# CRC16-CCITT (same algorithm as v10 protocol.lisp)
# ---------------------------------------------------------------------------

_CRC_TABLE: list[int] = []


def _build_table() -> list[int]:
    table = []
    for byte in range(256):
        crc = byte << 8
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
        table.append(crc)
    return table


def crc16_ccitt_false(data: bytes) -> int:
    if not _CRC_TABLE:
        _CRC_TABLE.extend(_build_table())
    crc = 0xFFFF
    for byte in data:
        crc = ((crc << 8) ^ _CRC_TABLE[((crc >> 8) ^ byte) & 0xFF]) & 0xFFFF
    return crc


# ---------------------------------------------------------------------------
# Payload builders / parsers
# ---------------------------------------------------------------------------

def pack_matrix_payload(shape: list[int], data: list[float]) -> bytes:
    """Pack a tensor payload (opcode 0x02).

    [NumDims 1B][DimLengths Nd×4B LE][Data prod(dims)×8B LE Float64]
    """
    fmt = f'<B{len(shape)}I{len(data)}d'
    return struct.pack(fmt, len(shape), *shape, *data)


def parse_matrix_payload(payload: bytes) -> tuple[list[int], list[float]]:
    nd = payload[0]
    dims = list(struct.unpack_from(f'<{nd}I', payload, 1))
    n = 1
    for d in dims:
        n *= d
    data = list(struct.unpack_from(f'<{n}d', payload, 1 + nd * 4))
    return dims, data


def pack_complex_payload(real: list[float], imag: list[float]) -> bytes:
    n = len(real)
    fmt = f'<I{n}d{n}d'
    return struct.pack(fmt, n, *real, *imag)


def parse_complex_payload(payload: bytes) -> tuple[list[float], list[float]]:
    n = struct.unpack_from('<I', payload, 0)[0]
    real = list(struct.unpack_from(f'<{n}d', payload, 4))
    imag = list(struct.unpack_from(f'<{n}d', payload, 4 + n * 8))
    return real, imag


def pack_csr_payload(num_rows: int, num_cols: int,
                     row_ptrs: list[int],
                     col_indices: list[int],
                     data: list[float]) -> bytes:
    nnz = len(data)
    fmt = f'<II{len(row_ptrs)}I{nnz}I{nnz}d'
    return struct.pack(fmt, num_rows, num_cols, *row_ptrs, *col_indices, *data)


def parse_csr_payload(payload: bytes) -> tuple:
    num_rows, num_cols = struct.unpack_from('<II', payload, 0)
    n_rp = num_rows + 1
    off = 8
    row_ptrs = list(struct.unpack_from(f'<{n_rp}I', payload, off))
    off += n_rp * 4
    nnz = row_ptrs[-1]
    col_indices = list(struct.unpack_from(f'<{nnz}I', payload, off))
    off += nnz * 4
    data = list(struct.unpack_from(f'<{nnz}d', payload, off))
    return num_rows, num_cols, row_ptrs, col_indices, data


def pack_evolve_payload(domain: str, ast_str: str, iterations: int = 500,
                        flags: int = 0) -> bytes:
    dom_enc = domain.encode('utf-8')
    ast_enc = ast_str.encode('utf-8')
    return struct.pack(f'<IB{len(dom_enc)}sH{len(ast_enc)}sIH',
                       1, len(dom_enc), dom_enc,
                       len(ast_enc), ast_enc, iterations, flags)


def parse_evolve_response(payload: bytes) -> dict:
    version = struct.unpack_from('<I', payload, 0)[0]
    off = 4
    best_ast_len = struct.unpack_from('<H', payload, off)[0]
    off += 2
    best_ast = payload[off:off + best_ast_len].decode('utf-8')
    off += best_ast_len
    best_penalty = struct.unpack_from('<d', payload, off)[0]
    off += 8
    total_eval = struct.unpack_from('<I', payload, off)[0]
    off += 4
    elapsed = struct.unpack_from('<d', payload, off)[0]
    return dict(best_ast=best_ast, best_penalty=best_penalty,
                total_evaluations=total_eval, elapsed_seconds=elapsed)


def pack_censor_payload(domain: str, ast_str: str) -> bytes:
    dom_enc = domain.encode('utf-8')
    ast_enc = ast_str.encode('utf-8')
    return struct.pack(f'<IB{len(dom_enc)}sH{len(ast_enc)}s',
                       1, len(dom_enc), dom_enc, len(ast_enc), ast_enc)


# ---------------------------------------------------------------------------
# TCBP Client
# ---------------------------------------------------------------------------

@dataclass
class TcbpResponse:
    opcode: int
    payload: bytes

    def as_text(self) -> str:
        return self.payload.decode('utf-8', errors='replace')

    def as_float64(self) -> float:
        return struct.unpack('<d', self.payload)[0]

    def as_matrix(self) -> tuple[list[int], list[float]]:
        return parse_matrix_payload(self.payload)

    def as_complex(self) -> tuple[list[float], list[float]]:
        return parse_complex_payload(self.payload)

    def as_evolve(self) -> dict:
        return parse_evolve_response(self.payload)


class TcbpClient:
    """Low-level TCBP client.

    Connects to the Lisp router (port 8443 by default).
    Handles frame framing, CRC verification, and reconnection.
    """

    def __init__(self, host: str = '127.0.0.1', port: int = 8443,
                 connect_timeout: float = 5.0):
        self.host = host
        self.port = port
        self.connect_timeout = connect_timeout
        self._sock: Optional[socket.socket] = None

    # ------------------------------------------------------------------
    # Connection management
    # ------------------------------------------------------------------

    def connect(self):
        if self._sock is not None:
            return
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.settimeout(self.connect_timeout)
        self._sock.connect((self.host, self.port))

    def close(self):
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
            self._sock = None

    def __enter__(self) -> 'TcbpClient':
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    # ------------------------------------------------------------------
    # Core send / receive
    # ------------------------------------------------------------------

    def send_frame(self, opcode: int, payload: bytes) -> TcbpResponse:
        """Send one TCBP frame and return the response."""
        self.connect()
        header = struct.pack('<BI', opcode, len(payload))
        frame = header + payload + struct.pack('<H', crc16_ccitt_false(header + payload))
        self._sock.sendall(frame)
        return self._recv_frame()

    def _recv_frame(self) -> TcbpResponse:
        header = self._recv_exact(5)
        opcode = header[0]
        payload_len = struct.unpack('<I', header[1:5])[0]
        payload_and_crc = self._recv_exact(payload_len + 2)
        received_crc = struct.unpack('<H', payload_and_crc[-2:])[0]
        payload_data = payload_and_crc[:-2]
        computed = crc16_ccitt_false(header + payload_data)
        if computed != received_crc:
            raise ValueError(f'CRC mismatch: computed={computed:#06x} '
                             f'received={received_crc:#06x}')
        return TcbpResponse(opcode, payload_data)

    def _recv_exact(self, n: int) -> bytes:
        buf = bytearray()
        while len(buf) < n:
            chunk = self._sock.recv(n - len(buf))
            if not chunk:
                raise ConnectionError('Connection closed by peer')
            buf.extend(chunk)
        return bytes(buf)

    # ------------------------------------------------------------------
    # Typed TCBP calls matching each opcode
    # ------------------------------------------------------------------

    def call_eval(self, expression: str) -> str:
        return self.send_frame(Opcode.EVAL, expression.encode('utf-8')).as_text()

    def call_matrix(self, shape: list[int], data: list[float]) -> tuple[list[int], list[float]]:
        return self.send_frame(Opcode.MATRIX,
                               pack_matrix_payload(shape, data)).as_matrix()

    def call_solve(self, equations: str) -> str:
        return self.send_frame(Opcode.SOLVE, equations.encode('utf-8')).as_text()

    def call_sym(self, function: str, variable: str) -> str:
        payload = f'{function},{variable}'.encode('utf-8')
        return self.send_frame(Opcode.SYM, payload).as_text()

    def call_graph(self, num_rows: int, num_cols: int,
                   row_ptrs: list[int], col_indices: list[int],
                   data: list[float]) -> tuple[list[float], list[float]]:
        resp = self.send_frame(Opcode.GRAPH,
                               pack_csr_payload(num_rows, num_cols,
                                                row_ptrs, col_indices, data))
        n = struct.unpack_from('<I', resp.payload, 0)[0]
        distances = list(struct.unpack_from(f'<{n}d', resp.payload, 4))
        predecessors = list(struct.unpack_from(f'<{n}d', resp.payload, 4 + n * 8))
        return distances, predecessors

    def call_dsp(self, real: list[float], imag: list[float]
                 ) -> tuple[list[float], list[float]]:
        return self.send_frame(Opcode.DSP,
                               pack_complex_payload(real, imag)).as_complex()

    def call_qsim(self, real: list[float], imag: list[float]
                  ) -> tuple[list[float], list[float]]:
        return self.send_frame(Opcode.QSIM,
                               pack_complex_payload(real, imag)).as_complex()

    def call_csp(self, constraints: str) -> str:
        return self.send_frame(Opcode.CSP, constraints.encode('utf-8')).as_text()

    # --- v7 extension opcodes (0x09–0x0B) ---

    def call_evolve(self, domain: str, ast_str: str,
                    iterations: int = 500, flags: int = 0) -> dict:
        resp = self.send_frame(Opcode.EVOLVE,
                               pack_evolve_payload(domain, ast_str,
                                                   iterations, flags))
        return resp.as_evolve()

    def call_censor(self, domain: str, ast_str: str) -> float:
        resp = self.send_frame(Opcode.CENSOR,
                               pack_censor_payload(domain, ast_str))
        return resp.as_float64()
