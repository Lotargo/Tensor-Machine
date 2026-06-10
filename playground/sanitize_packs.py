import os
import shutil
import zipfile
import hashlib
import subprocess
import sys

LAB_DIR = os.path.abspath("apl-rag-verification-lab")
TEMP_DIR = os.path.abspath("temp_unpack")

# ─── Public-safe replacements (ordered: more specific before general) ────
REPLACEMENTS = [
    # === Frame / protocol layout descriptions ===
    ("| 0 | 1 | Opcode | 0x0A (CENSOR) |", "| 0 | 1 | Type | 0x0A (policy gate) |"),
    ("[opcode:1B][len:4B LE][payload][CRC16:2B]", "[len:4B LE][payload][CRC16:2B]"),

    # === Long compound pipeline descriptions ===
    ("APL-RAG Verifier Bridge Layer (formulas.py + TCBP + Prolog censor)",
     "APL-RAG verification pipeline (computation + integrity check + policy gate)"),
    ("APL-RAG Verifier (formulas.py + TCBP + Prolog censor)",
     "APL-RAG verification pipeline (computation + integrity check + policy gate)"),
    ("APL-RAG Verifier bridge layer (formulas.py + TCBP + Prolog censor)",
     "APL-RAG verification pipeline (computation + integrity check + policy gate)"),
    ("live router integration", "future private integration, not part of this public artifact"),
    ("Live router integration", "Future private integration, not part of this public artifact"),
    ("Lisp router / Prolog daemon pool", "external verifier is not included"),
    ("Prolog daemon pool", "external verifier is not included"),
    ("Lisp router", "external verifier"),
    ("the verifier router", "the pipeline"),
    ("live the verifier and the router", "the full processing pipeline"),
    ("the verifier censor bridge", "the policy gate bridge"),
    ("the verifier integration", "the pipeline integration"),
    ("the verifier bridge", "the pipeline bridge"),

    # === Architecture name / RFC references ===
    ("APL-RAG Verifier Binary Protocol", "Local payload integrity demo"),
    ("APL-RAG Verifier FP stabilization", "Numerical stabilization module"),
    ("APL-RAG Verifier Symbolic Verification Layer", "Rule-based validation layer"),
    ("APL-RAG Verifier Observer Model", "Observer model (internal)"),
    ("APL-pi-phi RFC §23", "internal design notes"),
    ("APL-pi-phi RFC §25", "internal design notes"),
    ("APL-pi-phi architecture", "pipeline architecture"),
    ("APL-pi-phi", "verification lab prototype"),
    ("apl-pi-phi", "verification lab prototype"),
    ("APL-π-φ", "verification lab prototype"),
    ("the RFC architecture", "the pipeline architecture"),
    ("RFC architecture", "pipeline architecture"),
    ("the RFC", "the design notes"),
    ("The RFC", "The design notes"),
    ("the APL-RAG Verifier architecture", "the verification pipeline architecture"),
    ("APL-RAG Verifier Bridge Layer", "APL-RAG verification pipeline"),
    ("APL-RAG Verifier bridge layer", "APL-RAG verification pipeline"),
    ("APL-RAG Verifier", "APL-RAG Verification Lab"),

    # === Section references in docs ===
    ("§4, §15, §22, §24, §25, §27", "internal design sections"),
    ("§24.9", "internal section"),
    ("§25 Symbolic Verification Layer", "Rule-based validation layer (internal design)"),
    ("§25", "internal design"),
    ("§15", "internal design"),
    ("§22", "internal design"),
    ("§24", "internal design"),
    ("§27", "internal design"),

    # === VectorSpace / QIR / πφ ===
    ("VectorSpace", "internal data structure"),
    ("QIR", "internal query formalism"),

    # === Opcode references ===
    ("opcode 0x0A (CENSOR)", "type identifier 0x0A (policy gate)"),
    ("opcode 0x0B (SINGULARITY)", "type identifier 0x0B (singularity event)"),
    ("opcode = 0x0B  # SINGULARITY opcode (new: beyond CENSOR=0x0A)", "type_id = 0x0B  # singularity event type"),
    ("opcode = 0x0B", "type_id = 0x0B"),
    ("def build_tcbp_frame(opcode: int, payload: bytes)", "def build_demo_frame(type_id: int, payload: bytes)"),
    ("def make_tcbp_frame(opcode: int, payload: bytes)", "def make_demo_frame(type_id: int, payload: bytes)"),
    ("bytes([opcode])", "bytes([type_id])"),
    ("opcode 0x0A = CENSOR (v7 extension)", "type identifier 0x0A (demo)"),
    ("opcode 0x0B = SINGULARITY", "type identifier 0x0B (singularity)"),
    ("Opcode 0x0B (SINGULARITY) is new and not yet registered", "Type identifier 0x0B is a demo placeholder"),
    ("opcode 0x0A", "type identifier 0x0A"),
    ("opcode 0x0B", "type identifier 0x0B"),
    ("opcode=0x0A", "type_id=0x0A"),
    ("Opcode 0x0B", "Type identifier 0x0B"),
    ("CENSOR opcode", "POLICY_GATE type"),
    ("SINGULARITY opcode", "SINGULARITY type"),

    # === CENSOR / SINGULARITY names ===
    ("(CENSOR)", "(POLICY_GATE)"),
    ("CENSOR = 0x0A", "POLICY_GATE = 0x0A"),
    ("CENSOR", "POLICY_GATE"),
    ("SINGULARITY", "SINGULARITY_EVENT"),

    # === Protocol / transport renames ===
    ("TCBP transport", "demo transport"),
    ("TCBP binary frame", "demo integrity frame"),
    ("TCBP frame", "demo frame"),
    ("TCBP-совместимую", "демо-совместимую"),
    ("TCBP-факт", "строку"),
    ("TCBP protocol", "demo transport layer"),
    ("TCBP client library", "demo transport library"),
    ("TCBP encoding", "demo transport encoding"),
    ("TCBP", "demo transport frame"),

    # === Function/variable renames ===
    ("bridge/tcbp.py", "demo_transport.py"),
    ("tcbp.py", "demo_transport.py"),
    ("bridge.tcbp", "demo_transport"),
    ("build_tcbp_frame", "build_demo_frame"),
    ("make_tcbp_frame", "make_demo_frame"),
    ("validate_tcbp_frame", "validate_demo_frame"),
    ("make_tcbp_singularity_frame", "make_demo_singularity_frame"),
    ("to_tcbp_payload", "to_demo_payload"),
    ("tcbp_reject", "transport_reject"),
    ("tcbp_ok", "transport_ok"),
    ("tcbp_reason", "transport_reason"),
    ("tcbp_counts", "transport_counts"),

    # === Router / daemon ===
    ("the router router", "the pipeline"),
    ("the router router", "the pipeline"),
    ("the router Lisp router", "the pipeline"),
    ("the router", "the pipeline"),
    ("daemon pool", "worker pool"),
    ("daemon", "worker"),
    ("Daemon", "Worker"),

    # === Censor → policy gate ===
    ("censor_verdict", "policy_gate_verdict"),
    ("Censor singularity policy", "Policy gate singularity policy"),
    ("Censor policy", "Policy gate"),
    ("censor policy", "policy gate"),
    ("Censor Rules", "Policy Gate"),
    ("Censor rule", "Policy gate rule"),
    ("Censor singularity policy", "Policy gate singularity policy"),

    # === Prolog/Lisp → generic ===
    ("Prolog-style censor", "rule-based policy gate"),
    ("Prolog-цензор", "цензор"),
    ("Prolog-факт", "строку данных"),
    ("Prolog censor (simulated)", "policy gate (simulated)"),
    ("Prolog censor", "policy gate"),
    ("Prolog-style", "rule-based"),
    ("Prolog", "Verifier"),
    ("Lisp Router", "Verifier Bridge"),
    ("Lisp", "Verifier"),

    # === Experiment titles ===
    ("Adversarial protocol checks", "Adversarial input and integrity checks"),

    # === Binary Protocol / Symbolic Verification Layer ===
    ("Binary Protocol", "Local payload integrity demo"),
    ("Symbolic Verification Layer", "Rule-based validation layer"),
]

INLINE_CRC_CODE = """
def crc16_ccitt_false(data: bytes) -> int:
    crc = 0xFFFF
    for byte in data:
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc
"""

MINIMAL_FORMULAS = """# Minimal formulas for verification demo
def kahan_sum(values: list[float]) -> float:
    total = 0.0
    carry = 0.0
    for v in values:
        y = v - carry
        t = total + y
        carry = (t - total) - y
        total = t
    return total
"""

def compute_sha256(filepath):
    h = hashlib.sha256()
    with open(filepath, 'rb') as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def remove_pycache(root_dir):
    """Remove all __pycache__ directories under root_dir."""
    for dirpath, dirnames, filenames in os.walk(root_dir, topdown=False):
        if os.path.basename(dirpath) == "__pycache__":
            shutil.rmtree(dirpath, ignore_errors=True)
            print(f"  - Removed {dirpath}")

def sanitize_file_content(filepath):
    _, ext = os.path.splitext(filepath)
    if ext not in ['.py', '.md', '.txt']:
        return

    with open(filepath, 'r', encoding='utf-8', errors='ignore') as f:
        content = f.read()

    # Apply inline CRC code injection to python files first
    if ext == '.py':
        if "from bridge.tcbp import" in content or "import bridge.tcbp" in content:
            lines = content.splitlines()
            new_lines = []
            for line in lines:
                if "bridge.tcbp" in line:
                    continue
                new_lines.append(line)
            content = "\n".join(new_lines)
            content = INLINE_CRC_CODE + "\n" + content

    # Apply all ordered replacements to the content
    for old, new in REPLACEMENTS:
        content = content.replace(old, new)

    with open(filepath, 'w', encoding='utf-8') as f:
        f.write(content)

def process_pack(exp_name, zip_name):
    print(f"\\n=== Processing {exp_name} ({zip_name}) ===")
    zip_path = os.path.join(LAB_DIR, exp_name, zip_name)
    unpack_path = os.path.join(TEMP_DIR, exp_name)

    if os.path.exists(unpack_path):
        shutil.rmtree(unpack_path)
    os.makedirs(unpack_path)

    # 1. Unzip
    with zipfile.ZipFile(zip_path, 'r') as zip_ref:
        zip_ref.extractall(unpack_path)

    # 2. Delete bridge/tcbp.py and bridge directory
    bridge_dir = os.path.join(unpack_path, "bridge")
    if os.path.exists(bridge_dir):
        shutil.rmtree(bridge_dir)
        print("  - Deleted bridge directory.")

    # 3. Clean formulas.py if it exists
    formulas_path = os.path.join(unpack_path, "formulas.py")
    if os.path.exists(formulas_path):
        with open(formulas_path, 'w', encoding='utf-8') as f:
            f.write(MINIMAL_FORMULAS)
        print("  - Replaced formulas.py with minimal Kahan sum implementation.")

    # 4. Remove __pycache__ directories
    remove_pycache(unpack_path)

    # 5. Recursively sanitize all file contents
    for root, dirs, files in os.walk(unpack_path):
        for file in files:
            file_path = os.path.join(root, file)
            sanitize_file_content(file_path)

    print("  - Sanitized file contents.")

    # 6. Run tests locally on Windows and WSL to verify
    test_run_success = True
    test_files = ["test_gravity.py", "test_adversarial.py", "exp_singularity.py"]
    for tf in test_files:
        tf_path = os.path.join(unpack_path, tf)
        if os.path.exists(tf_path):
            print(f"  - Running test: {tf} on Windows...")
            res = subprocess.run([sys.executable, tf], cwd=unpack_path, capture_output=True, text=True)
            if res.returncode != 0:
                print(f"    ERROR: {tf} failed on Windows!")
                print("STDOUT:", res.stdout)
                print("STDERR:", res.stderr)
                test_run_success = False
            else:
                print(f"    PASS: {tf} succeeded.")

            print(f"  - Running test: {tf} on WSL...")
            res_wsl = subprocess.run(["wsl", "python3", tf], cwd=unpack_path, capture_output=True, text=True)
            if res_wsl.returncode != 0:
                print(f"    ERROR: {tf} failed on WSL!")
                print("STDOUT:", res_wsl.stdout)
                print("STDERR:", res_wsl.stderr)
                test_run_success = False
            else:
                print(f"    PASS: {tf} succeeded on WSL.")

    if not test_run_success:
        print("  WARNING: Some tests failed! Check output.")

    # 7. Regenerate manifest
    manifest_path = os.path.join(unpack_path, "exp_docs", "MANIFEST.sha256")
    if os.path.exists(manifest_path):
        manifest_entries = []
        for root, dirs, files in os.walk(unpack_path):
            for file in files:
                if file.endswith(".pyc"):
                    continue
                full_path = os.path.join(root, file)
                rel_path = os.path.relpath(full_path, unpack_path).replace("\\", "/")
                if rel_path == "exp_docs/MANIFEST.sha256":
                    continue
                sha = compute_sha256(full_path)
                manifest_entries.append(f"{sha}  {rel_path}")
        
        manifest_entries.sort(key=lambda x: x.split("  ")[1])

        with open(manifest_path, 'w', encoding='utf-8') as f:
            f.write(f"# {exp_name} Artifact Manifest\n")
            f.write("# Sanitized public-safe v3\n")
            f.write("# ============================================================\n\n")
            f.write("\n".join(manifest_entries) + "\n")
        print("  - Regenerated MANIFEST.sha256.")

    # 9. Re-zip (excluding __pycache__ and .pyc)
    os.remove(zip_path)
    with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zip_ref:
        for root, dirs, files in os.walk(unpack_path):
            # Skip __pycache__ directories
            dirs[:] = [d for d in dirs if d != "__pycache__"]
            for file in files:
                if file.endswith(".pyc"):
                    continue
                full_path = os.path.join(root, file)
                rel_path = os.path.relpath(full_path, unpack_path)
                zip_ref.write(full_path, rel_path)
    print(f"  - Repacked {zip_name} successfully.")

def main():
    if not os.path.exists(TEMP_DIR):
        os.makedirs(TEMP_DIR)

    process_pack("EXP-001", "EXP-001-reproducibility-pack.zip")
    process_pack("EXP-002", "EXP-002-reproducibility-pack.zip")
    process_pack("EXP-003", "EXP-003-reproducibility-pack.zip")
    process_pack("EXP-004", "EXP-004-alpha-reproducibility-pack.zip")

    if os.path.exists(TEMP_DIR):
        shutil.rmtree(TEMP_DIR)
        print("\\nCleaned up temporary unpack directory.")

if __name__ == "__main__":
    main()
