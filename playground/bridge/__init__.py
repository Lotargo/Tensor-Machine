"""
APL-π-φ Bridge: Python ↔ TCBP ↔ Lisp Router ↔ v7 Engine ↔ Prolog Censor

Exports:
    TcbpClient          — low-level TCBP binary frame client
    ApiClient           — high-level numpy/Python API
    FormulaBridge       — connects formulas.py to v7 evolver
    workflow_*          — canned orchestration scripts
"""

from .tcbp import TcbpClient, Opcode, crc16_ccitt_false
from .client import ApiClient
from .formula_orchestrator import FormulaBridge
from . import workflow
