import subprocess
import time
import re
import math

import numpy as np
import sympy as sp
from scipy.sparse import csr_matrix
from scipy.sparse.csgraph import dijkstra

def run_cmd(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    return result.stdout.strip()

def extract_complex_array(lua_output):
    lines = lua_output.split('\n')
    arr = []
    for line in lines:
        if '+' in line and 'i' in line:
            parts = line.split('+')
            r = float(parts[0].strip())
            i = float(parts[1].replace('i','').strip())
            arr.append(complex(r, i))
    return np.array(arr)

def start_services():
    print("Starting background services...")
    subprocess.Popen("apl --noCIN --noSV -f coprocessor/matrix.apl", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.Popen("sbcl --noinform --eval '(load \"router/server.lisp\")' --eval '(tcbp-server:start-server)'", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2)

def stop_services():
    print("Stopping background services...")
    subprocess.run("kill $(lsof -t -i :8443) 2>/dev/null || true", shell=True)
    subprocess.run("kill $(lsof -t -i :8444) 2>/dev/null || true", shell=True)

def main():
    start_services()
    try:
        print("Running A/B Tests...")

        # SYM
        x = sp.Symbol('x')
        f = x * x
        expected_sym = sp.diff(f, x) # 2*x

        lua_sym = run_cmd("luajit client/main.lua sym '(* x x),x'")
        print(f"Python SYM Baseline: {expected_sym}")
        print(f"Lua/Lisp SYM Result: {lua_sym}")
        assert "(+ X X)" in lua_sym or "(* 2 X)" in lua_sym, "SYM mismatch"

        # DSP
        vec = np.array([1.0, 0.0, -1.0, 0.0])
        expected_fft = np.fft.fft(vec)

        lua_dsp = run_cmd("luajit client/main.lua dsp '1.0,0.0,0.0,0.0,-1.0,0.0,0.0,0.0'")
        parsed_fft = extract_complex_array(lua_dsp)
        print(f"Python DSP Baseline: {expected_fft}")
        print(f"Lua/Lisp DSP Result: {parsed_fft}")
        np.testing.assert_allclose(parsed_fft.real, expected_fft.real, atol=1e-7)
        np.testing.assert_allclose(parsed_fft.imag, expected_fft.imag, atol=1e-7)

        # GRAPH
        row = np.array([0, 0, 1])
        col = np.array([1, 2, 2])
        data = np.array([1.0, 4.0, 2.0])
        graph = csr_matrix((data, (row, col)), shape=(3, 3))
        expected_dist = dijkstra(csgraph=graph, directed=True, indices=0)

        lua_graph = run_cmd("luajit client/main.lua graph '3,0,2,3,3,1,2,2,1.0,4.0,2.0'")
        parsed_graph = extract_complex_array(lua_graph) # we packed it as complex array to send floats back
        print(f"Python GRAPH Baseline: {expected_dist}")
        print(f"Lua/Lisp GRAPH Result: {parsed_graph.real}")
        np.testing.assert_allclose(parsed_graph.real, expected_dist, atol=1e-7)

        print("All A/B Tests Passed!")
    finally:
        stop_services()

if __name__ == "__main__":
    main()
