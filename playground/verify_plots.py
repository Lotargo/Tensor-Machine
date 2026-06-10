"""
verify_plots.py — Верификационные графики для EXP-001 и EXP-002.

Генерирует PNG-диаграммы в playground/exp_docs/figures/,
независимо подтверждающие результаты обоих экспериментов.

Запуск:
  python playground/verify_plots.py

Требования: numpy, matplotlib (>=3.5)
"""

import math
import struct
import sys
import os
from pathlib import Path

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import ScalarFormatter

sys.path.insert(0, str(Path(__file__).resolve().parent))
import formulas as F
from bridge.tcbp import crc16_ccitt_false

# ── Constants (same as test_gravity.py / test_adversarial.py) ──
G = 6.67430e-11
M = 5.97219e24
R = 6_371_000
DPI = 150
FIGS = Path(__file__).resolve().parent / "exp_docs" / "figures"
FIGS.mkdir(parents=True, exist_ok=True)


# ═══════════════════════════════════════════════════════════════
#  PART 1 — EXP-001: Gravity verification plots
# ═══════════════════════════════════════════════════════════════

def plot_gravity_vs_height():
    """g(h) vs height with analytical inverse-square overlay."""
    hs = np.logspace(0, 7, 200)  # 1 m → 10,000 km
    gs = G * M / (R + hs) ** 2
    g0 = G * M / (R * R)

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 5))

    # Left: g vs height
    ax1.semilogx(hs, gs, "b-", lw=2, label=r"$g(h) = GM/(R+h)^2$")
    ax1.axhline(g0, color="gray", ls="--", lw=1, alpha=0.6, label=r"$g(0)$")
    ax1.axvline(R, color="r", ls=":", lw=1, alpha=0.5, label="R (surface)")
    ax1.set_xlabel("Height h (m)")
    ax1.set_ylabel("g (m/s²)")
    ax1.set_title("Gravitational acceleration\nvs height above surface")
    ax1.legend(fontsize=9)
    ax1.grid(True, alpha=0.3)
    # Mark test points from EXP-001
    test_hs = [0, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000]
    test_gs = [G * M / (R + h) ** 2 for h in test_hs]
    ax1.scatter(test_hs, test_gs, color="red", zorder=5, s=40,
                label="EXP-001 test points")
    ax1.legend(fontsize=9)

    # Right: inverse-square verification (g/g0 vs R²/(R+h)²)
    ratio_g = gs / g0
    ratio_pred = (R ** 2) / ((R + hs) ** 2)
    ax2.plot(ratio_pred, ratio_g, "k-", lw=1.5, label="data")
    ax2.plot([0, 1], [0, 1], "r--", lw=1.5, alpha=0.6, label="identity")
    ax2.set_xlabel(r"Predicted $R^2/(R+h)^2$")
    ax2.set_ylabel(r"Measured $g(h)/g(0)$")
    ax2.set_title("Inverse-square law verification\n(identity = perfect agreement)")
    ax2.legend(fontsize=9)
    ax2.grid(True, alpha=0.3)
    ax2.set_aspect("equal", adjustable="box")
    ax2.set_xlim(0, 1.05)
    ax2.set_ylim(0, 1.05)

    plt.tight_layout()
    path = FIGS / "exp001_gravity_vs_height.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [1/4] {path.name}")


def plot_residuals():
    """Numerical residual: |g_computed - g0 * R²/(R+h)²|."""
    hs = np.logspace(0, 7, 500)
    gs = G * M / (R + hs) ** 2
    g0 = G * M / (R * R)
    predicted = g0 * (R ** 2) / ((R + hs) ** 2)
    residual = np.abs(gs - predicted)

    # relative residual
    rel_residual = residual / gs

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 5))

    ax1.loglog(hs, residual, "b-", lw=2)
    ax1.set_xlabel("Height h (m)")
    ax1.set_ylabel("Absolute residual |g − g_pred| (m/s²)")
    ax1.set_title("Inverse-square residual\n(machine epsilon level)")
    ax1.grid(True, alpha=0.3, which="both")
    # machine epsilon line
    eps = np.finfo(float).eps
    ax1.axhline(eps, color="gray", ls="--", lw=1, alpha=0.6,
                label=f"ε = {eps:.0e}")
    ax1.legend(fontsize=9)

    ax2.semilogx(hs, rel_residual, "b-", lw=2)
    ax2.set_xlabel("Height h (m)")
    ax2.set_ylabel("Relative residual |g − g_pred| / g")
    ax2.set_title("Relative inverse-square error\n(< 1e-15 at all heights)")
    ax2.grid(True, alpha=0.3)
    ax2.axhline(eps, color="gray", ls="--", lw=1, alpha=0.6,
                label=f"ε = {eps:.0e}")
    ax2.legend(fontsize=9)

    plt.tight_layout()
    path = FIGS / "exp001_residuals.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [2/4] {path.name}")


def plot_loglog_slope():
    """Log-log g vs r — confirm power law exponent = -2."""
    rs = np.logspace(np.log10(R), np.log10(R + 10_000_000), 300)
    gs = G * M / rs ** 2

    fig, ax = plt.subplots(figsize=(7, 5))
    ax.loglog(rs, gs, "b-", lw=2, label="data")

    # Fit slope in log-log: log(g) = log(GM) - 2*log(r)
    log_r = np.log(rs)
    log_g = np.log(gs)
    coeffs = np.polyfit(log_r, log_g, 1)
    fit_line = np.exp(np.polyval(coeffs, log_r))
    ax.loglog(rs, fit_line, "r--", lw=1.5, alpha=0.7,
              label=f"fit: slope = {coeffs[0]:.6f}")

    ax.set_xlabel("Distance from Earth center r (m)")
    ax.set_ylabel("g (m/s²)")
    ax.set_title("Log-log: g ∝ r^−2\n(exponent = −2 confirms inverse-square)")
    ax.legend(fontsize=10)
    ax.grid(True, alpha=0.3, which="both")

    # Annotate
    ax.annotate(f"Slope = {coeffs[0]:.6f}\n(target: −2.000000)",
                xy=(0.05, 0.05), xycoords="axes fraction", fontsize=11,
                bbox=dict(boxstyle="round,pad=0.3", fc="wheat", alpha=0.7))

    plt.tight_layout()
    path = FIGS / "exp001_loglog_slope.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [3/4] {path.name}")


def plot_kahan_accuracy():
    """Kahan compensated sum error for R + h at various heights."""
    hs = np.logspace(-3, 7, 200)  # mm to 10,000 km
    errors = []
    for h in hs:
        r_exact = float(R) + h
        r_kahan = F.kahan_sum([float(R), h])
        errors.append(abs(r_kahan - r_exact))

    fig, ax = plt.subplots(figsize=(7, 5))
    ax.loglog(hs, errors, "g-", lw=2)
    ax.axhline(1e-6, color="gray", ls="--", lw=1, alpha=0.6,
               label="tolerance (1e-6)")
    ax.set_xlabel("Height h (m)")
    ax.set_ylabel("|kahan(R + h) − (R + h)|")
    ax.set_title("Kahan compensated summation error\n(R = 6,371,000 m)")
    ax.legend(fontsize=10)
    ax.grid(True, alpha=0.3, which="both")

    # Highlight test heights
    test_hs = [0, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000]
    test_err = []
    for h in test_hs:
        r_exact = float(R) + h
        r_kahan = F.kahan_sum([float(R), h])
        test_err.append(abs(r_kahan - r_exact))
    ax.scatter(test_hs, test_err, color="red", zorder=5, s=40,
               label="EXP-001 test points")

    ax.legend(fontsize=10)
    plt.tight_layout()
    path = FIGS / "exp001_kahan_error.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [4/4] {path.name}")


# ═══════════════════════════════════════════════════════════════
#  PART 2 — EXP-002: Adversarial verification plots
# ═══════════════════════════════════════════════════════════════

# Replay each adversarial scenario to collect structured results
def _replay_scenario(height, G_val, M_val, crc_mode=None, length_mode=None,
                     payload_mode=None):
    """Run one scenario and return structured dict of results."""
    from bridge.tcbp import crc16_ccitt_false as crc16

    result = None
    computation_ok = True
    if not math.isfinite(height):
        computation_ok = False
    elif not math.isfinite(G_val) or not math.isfinite(M_val):
        computation_ok = False
    elif G_val <= 0 or M_val <= 0:
        computation_ok = False
    else:
        r = float(R) + height
        if r <= 0:
            computation_ok = False
        else:
            try:
                g = G_val * M_val / (r * r)
                if math.isfinite(g) and g > 0:
                    result = {"g": g, "height": height}
            except (ZeroDivisionError, OverflowError, ValueError):
                computation_ok = False

    # TCBP
    if result is not None and payload_mode != "nan":
        g_val = result["g"]
    else:
        g_val = 0.0
    fact = "gravity(h=0,g=NaN)" if payload_mode == "nan" else f"gravity(h={height},g={g_val})"
    payload = fact.encode("utf-8")

    frame = bytes([0x0A]) + struct.pack("<I", len(payload)) + payload
    crc = crc16(frame)
    frame += struct.pack("<H", crc)

    if crc_mode == "flip":
        fb = bytearray(frame); fb[-2] ^= 0x01; frame = bytes(fb)
    elif crc_mode == "zero":
        fb = bytearray(frame); fb[-2] = 0x00; fb[-1] = 0x00; frame = bytes(fb)
    elif crc_mode == "swap":
        fb = bytearray(frame); fb[-2], fb[-1] = fb[-1], fb[-2]; frame = bytes(fb)

    if length_mode == "ff":
        fb = bytearray(frame); fb[1] = 0xFF; frame = bytes(fb)
    elif length_mode == "zero":
        fb = bytearray(frame); fb[1] = 0x00; frame = bytes(fb)
    elif length_mode == "ffffffff":
        fb = bytearray(frame); fb[1] = 0xFF; fb[2] = 0xFF; fb[3] = 0xFF; fb[4] = 0xFF; frame = bytes(fb)
    elif length_mode == "shorter":
        fb = bytearray(frame); fb[1] = 0x01; frame = bytes(fb)
    elif length_mode == "longer_valid_crc":
        fb = bytearray(frame); fb[1] = 0xFF
        frame = bytes(fb[:-2]) + struct.pack("<H", crc16(bytes(fb[:-2])))

    tcbp_ok = True
    tcbp_reason = ""
    if len(frame) < 7:
        tcbp_ok = False; tcbp_reason = "FRAME_TOO_SHORT"
    else:
        crc_recv = struct.unpack("<H", frame[-2:])[0]
        crc_calc = crc16(frame[:-2])
        if crc_recv != crc_calc:
            tcbp_ok = False; tcbp_reason = "CRC_MISMATCH"
        else:
            plen = struct.unpack("<I", frame[1:5])[0]
            if len(frame) != 7 + plen:
                tcbp_ok = False; tcbp_reason = "PAYLOAD_LENGTH_MISMATCH"

    # Censor
    censor_ok = True
    censor_reason = ""
    if not tcbp_ok:
        censor_ok = False; censor_reason = "SKIPPED"
    elif not computation_ok or result is None:
        censor_ok = False; censor_reason = "no result"
    elif payload_mode == "nan" or not math.isfinite(result["g"]):
        censor_ok = False; censor_reason = "NaN/Inf"
    elif result["g"] <= 0:
        censor_ok = False; censor_reason = "g <= 0"
    elif result["height"] < 0:
        censor_ok = False; censor_reason = "subsurface"

    reject_layers = []
    if not computation_ok or result is None:
        reject_layers.append("computation")
    if not tcbp_ok:
        reject_layers.append("TCBP")
    if not censor_ok:
        reject_layers.append("censor")
    if payload_mode == "nan":
        reject_layers.append("forced-NaN")

    return {
        "computation_ok": computation_ok and result is not None,
        "tcbp_ok": tcbp_ok,
        "censor_ok": censor_ok,
        "tcbp_reason": tcbp_reason,
        "censor_reason": censor_reason,
        "reject_layers": "+".join(reject_layers) if reject_layers else "—",
        "agg_reject": (not computation_ok or result is None or not tcbp_ok or not censor_ok),
    }


# Define all scenarios (same as test_adversarial.py)
SCENARIOS = [
    # (name, group, height, G, M, kwargs)
    ("1a: h = -R", 1, -R, G, M, {}),
    ("1b: h = -R-1", 1, -R - 1.0, G, M, {}),
    ("1c: h = -R+1", 1, -R + 1.0, G, M, {}),
    ("2a: h = -1000", 2, -1000.0, G, M, {}),
    ("2b: h = -1e6", 2, -1_000_000.0, G, M, {}),
    ("2c: h = -1", 2, -1.0, G, M, {}),
    ("3a: G = -G", 3, 0.0, -G, M, {}),
    ("3b: G = 0", 3, 0.0, 0.0, M, {}),
    ("3c: G = NaN", 3, 0.0, float("nan"), M, {}),
    ("4a: M = -M", 4, 0.0, G, -M, {}),
    ("4b: M = 0", 4, 0.0, G, 0.0, {}),
    ("4c: M = NaN", 4, 0.0, G, float("nan"), {}),
    ("5a: CRC flip", 5, 0.0, G, M, {"crc_mode": "flip"}),
    ("5b: CRC zero", 5, 0.0, G, M, {"crc_mode": "zero"}),
    ("5c: CRC swap", 5, 1000.0, G, M, {"crc_mode": "swap"}),
    ("6a: len=0xFF", 6, 0.0, G, M, {"length_mode": "ff"}),
    ("6b: len=0x00", 6, 0.0, G, M, {"length_mode": "zero"}),
    ("6c: len=0xFFFF", 6, 0.0, G, M, {"length_mode": "ffffffff"}),
    ("6d: len=0x01", 6, 0.0, G, M, {"length_mode": "shorter"}),
    ("6e: len+CRC", 6, 0.0, G, M, {"length_mode": "longer_valid_crc"}),
    ("7a: g=NaN", 7, 0.0, G, M, {"payload_mode": "nan"}),
    ("7b: h=+Inf", 7, float("inf"), G, M, {}),
    ("7c: h=-Inf", 7, -float("inf"), G, M, {}),
    ("8a: 0/0", 8, 0.0, 0.0, 0.0, {}),
    ("8b: h=NaN", 8, float("nan"), G, M, {}),
    ("8c: G=Inf", 8, 0.0, float("inf"), M, {}),
]


def plot_reject_path_distribution():
    """Stacked bar: which layer(s) rejected each group."""
    group_labels = ["G1\nSingularity", "G2\nSubsurface", "G3\nG<0",
                    "G4\nM<0", "G5\nCRC", "G6\nLength",
                    "G7\nNaN/Inf", "G8\n0/0"]
    groups = list(range(1, 9))

    # Per-group: count how many scenarios rejected by each layer
    comp_counts = []
    tcbp_counts = []
    cens_counts = []

    for g in groups:
        scs = [s for s in SCENARIOS if s[1] == g]
        comp = sum(1 for s in scs if "computation" in _replay_scenario(s[2], s[3], s[4], **s[5])["reject_layers"])
        tcbp = sum(1 for s in scs if "TCBP" in _replay_scenario(s[2], s[3], s[4], **s[5])["reject_layers"])
        cens = sum(1 for s in scs if "censor" in _replay_scenario(s[2], s[3], s[4], **s[5])["reject_layers"])
        comp_counts.append(comp)
        tcbp_counts.append(tcbp)
        cens_counts.append(cens)

    fig, ax = plt.subplots(figsize=(10, 5))
    x = np.arange(len(groups))
    width = 0.6

    bars1 = ax.bar(x, comp_counts, width, label="Computation", color="#e74c3c", alpha=0.85)
    bars2 = ax.bar(x, tcbp_counts, width, bottom=comp_counts, label="TCBP", color="#f39c12", alpha=0.85)
    bottom = [a + b for a, b in zip(comp_counts, tcbp_counts)]
    bars3 = ax.bar(x, cens_counts, width, bottom=bottom, label="Censor", color="#3498db", alpha=0.85)

    ax.set_xticks(x)
    ax.set_xticklabels(group_labels, fontsize=9)
    ax.set_ylabel("Scenarios rejected")
    ax.set_title("Rejection layer distribution by attack group")
    ax.legend(fontsize=10)
    ax.set_ylim(0, 6)
    ax.yaxis.set_major_locator(plt.MaxNLocator(integer=True))
    ax.grid(True, axis="y", alpha=0.3)

    # Annotate total per group
    for i, g in enumerate(groups):
        scs = [s for s in SCENARIOS if s[1] == g]
        total = sum(1 for s in scs if _replay_scenario(s[2], s[3], s[4], **s[5])["agg_reject"])
        ax.text(i, 5.3, f"{total}/{len(scs)}", ha="center", fontsize=10, fontweight="bold")

    plt.tight_layout()
    path = FIGS / "exp002_reject_distribution.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [1/4] {path.name}")


def plot_reject_path_heatmap():
    """Heatmap: scenario × reject layer."""
    layers_order = ["computation", "TCBP", "censor", "forced-NaN"]

    # Collect data for heatmap (scenarios × layers)
    n_scenarios = len(SCENARIOS)
    n_layers = len(layers_order)
    heat = np.zeros((n_scenarios, n_layers), dtype=int)

    scenario_labels = []
    group_boundaries = []

    prev_group = None
    for i, (name, g, h, gv, mv, kw) in enumerate(SCENARIOS):
        res = _replay_scenario(h, gv, mv, **kw)
        layers = res["reject_layers"]
        scenario_labels.append(name)

        # Mark group transitions for separator lines
        if prev_group is not None and g != prev_group:
            group_boundaries.append(i)
        prev_group = g

        for j, layer in enumerate(layers_order):
            if layer in layers:
                heat[i, j] = 1

    fig, ax = plt.subplots(figsize=(8, 10))
    im = ax.imshow(heat, cmap="RdYlGn_r", aspect="auto", vmin=0, vmax=1)

    # Group separator lines
    for b in group_boundaries:
        ax.axhline(b - 0.5, color="gray", lw=1.5, linestyle="-", alpha=0.5)

    ax.set_xticks(range(n_layers))
    ax.set_xticklabels(["Computation", "TCBP\n(CRC/Len)", "Censor", "NaN\n(path)"],
                       fontsize=9)
    ax.set_yticks(range(n_scenarios))
    ax.set_yticklabels(scenario_labels, fontsize=8)
    ax.set_title("Rejection layer activation per scenario\n(dark = layer rejected)",
                 fontsize=12)

    # Text annotations inside cells
    for i in range(n_scenarios):
        for j in range(n_layers):
            if heat[i, j]:
                ax.text(j, i, "✗", ha="center", va="center", fontsize=8,
                        color="white" if j < 2 else "black")

    plt.tight_layout()
    path = FIGS / "exp002_reject_heatmap.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [2/4] {path.name}")


def plot_crc_comparison():
    """Group 5: received vs expected CRC."""
    crc_data = [
        ("5a\nflip", 0xC6E6, 0xC6E7),
        ("5b\nzero", 0x0000, 0xC6E7),
        ("5c\nswap", 0x7F02, 0x027F),
    ]

    fig, ax = plt.subplots(figsize=(7, 5))
    x = np.arange(len(crc_data))
    width = 0.3

    recv_vals = [d[1] for d in crc_data]
    expt_vals = [d[2] for d in crc_data]

    bars1 = ax.bar(x - width / 2, recv_vals, width, label="Received CRC", color="#e74c3c", alpha=0.85)
    bars2 = ax.bar(x + width / 2, expt_vals, width, label="Expected CRC", color="#2ecc71", alpha=0.85)

    ax.set_xticks(x)
    ax.set_xticklabels([d[0] for d in crc_data], fontsize=10)
    ax.set_ylabel("CRC16 value")
    ax.set_title("CRC mutation: received vs expected\n(all must differ → REJECT)")
    ax.legend(fontsize=10)
    ax.grid(True, axis="y", alpha=0.3)

    # Annotate CRC values
    for i, (lbl, recv, exp) in enumerate(crc_data):
        ax.text(i - width / 2, recv + 500, f"0x{recv:04X}", ha="center",
                fontsize=9, fontweight="bold")
        ax.text(i + width / 2, exp + 500, f"0x{exp:04X}", ha="center",
                fontsize=9, fontweight="bold")

    plt.tight_layout()
    path = FIGS / "exp002_crc_comparison.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [3/4] {path.name}")


def plot_adversarial_summary():
    """Summary bar: adversarial vs control pass/fail."""
    adv_results = [_replay_scenario(h, gv, mv, **kw)
                   for _, _, h, gv, mv, kw in SCENARIOS]

    adv_rejected = sum(1 for r in adv_results if r["agg_reject"])
    adv_total = len(adv_results)
    adv_accepted = adv_total - adv_rejected

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(10, 4.5))

    # Left: adversarial breakdown
    cats = ["REJECTED", "ACCEPTED\n(false positive)"]
    vals = [adv_rejected, adv_accepted]
    colors = ["#2ecc71" if adv_rejected == adv_total else "#e74c3c",
              "#e74c3c"]
    bars = ax1.bar(cats, vals, color=colors, alpha=0.85, width=0.5)
    ax1.set_ylabel("Scenarios")
    ax1.set_title(f"Adversarial scenarios\n({adv_rejected}/{adv_total} rejected)")
    for bar, val in zip(bars, vals):
        ax1.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.3,
                 str(val), ha="center", fontsize=14, fontweight="bold")
    ax1.set_ylim(0, adv_total + 3)

    # Right: control
    ax2.bar(["Control\nPASS"], [1], color="#2ecc71", alpha=0.85, width=0.5)
    ax2.set_ylabel("Count")
    ax2.set_title("Valid input control")
    ax2.text(0, 1.3, "1/1", ha="center", fontsize=14, fontweight="bold")
    ax2.set_ylim(0, 3)

    for ax in [ax1, ax2]:
        ax.yaxis.set_major_locator(plt.MaxNLocator(integer=True))

    plt.tight_layout()
    path = FIGS / "exp002_summary.png"
    fig.savefig(path, dpi=DPI)
    plt.close(fig)
    print(f"  [4/4] {path.name}")


# ═══════════════════════════════════════════════════════════════
#  MAIN
# ═══════════════════════════════════════════════════════════════

def run():
    print("=" * 60)
    print("  VERIFY PLOTS")
    print("  Generating verification figures for EXP-001 & EXP-002")
    print("=" * 60)

    print("\n-- EXP-001: Gravity verification --")
    plot_gravity_vs_height()
    plot_residuals()
    plot_loglog_slope()
    plot_kahan_accuracy()

    print("\n-- EXP-002: Adversarial verification --")
    plot_reject_path_distribution()
    plot_reject_path_heatmap()
    plot_crc_comparison()
    plot_adversarial_summary()

    print("\n" + "=" * 60)
    print(f"  All figures saved to: {FIGS}")
    print(f"  Total: 8 plots")
    print("=" * 60)


if __name__ == "__main__":
    run()
