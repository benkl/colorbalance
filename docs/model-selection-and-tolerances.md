# Model Comparison, Selection Decision, and Export Tolerances

Issue #13: Model evaluation, decision record, and interchange approximation tolerances.

## 1. Model Evaluation Protocol

To avoid selecting a model based on training error alone, candidate calibration models were compared using:
1. **Leave-One-Out Cross-Validation (LOOCV)** across the 24 patches of the ColorChecker Classic (ColorChecker 2005 dataset, Bradford D50 to D65).
2. **Out-of-sample and boundary stability**: evaluating response behavior at the boundaries of the normalized RGB cube $[0.0, 1.0]^3$, checking for non-monotonicity, severe undershoot ($< 0$), and wild overshoot ($> 1$).
3. **Condition number and numerical stability**: matrix condition number $\kappa(M) = \|M\|_\infty \cdot \|M^{-1}\|_\infty$.

Tested models:
- **Constrained 3×3 linear matrix**: black-preserving, zero additive bias ($RGB_{out} = RGB_{in} \cdot M$).
- **Polynomial Degree 2 (Cheung 2004)**: 5 terms ($R, G, B, RG, GB$).
- **Root-Polynomial Degree 2 (Finlayson 2015)**: 6 terms ($R, G, B, \sqrt{RG}, \sqrt{GB}, \sqrt{BR}$).
- **Root-Polynomial Degree 3 (Finlayson 2015)**: 13 terms.

## 2. Aggregate Results

Results measured under non-linear sensor response ($V = I^{1.05} \cdot M_{sensor}$):

| Model | LOOCV Mean ΔE00 | LOOCV Median ΔE00 | LOOCV 95th %-ile | LOOCV Max ΔE00 | Boundary Min Output | Boundary Max Output | Condition Stability |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Constrained 3×3 Linear** | **1.027** | **0.928** | **1.377** | **4.198** | **-0.003** | **1.054** | **Well-conditioned ($\kappa \approx 1.15$)** |
| Polynomial Deg 2 (5 terms) | 0.924 | 0.608 | 3.302 | 4.513 | -0.041 | 1.120 | Moderate variance on high chroma |
| Root-Polynomial Deg 2 (6 terms) | 1.051 | 0.945 | 1.547 | 4.334 | 0.000 | 1.063 | Stable |
| Root-Polynomial Deg 3 (13 terms) | 2.819 | 0.975 | 10.747 | 30.457 | -0.300 | 2.130 | **Severe Overfitting & Overshoot** |

## 3. Decision Record: Retain Constrained 3×3 Matrix as Default

**Decision**: Retain the constrained 3×3 linear matrix model as the default calibration engine.

**Rationale**:
1. **Generalization Over Fitting**: While higher-order models reduce training error on the 24 measured samples, Degree 3 root-polynomials overfit dramatically, yielding a catastrophic 95th-percentile error of $10.75$ and max error of $30.46$ on held-out patches, along with severe boundary overshoot up to $2.13$.
2. **Domain Boundary Stability**: The linear 3×3 model exhibits almost zero boundary distortion (min $-0.003$, max $1.054$), preserving smooth gradients outside the chart samples.
3. **Zero Zero-Crossing Invariant**: The linear 3×3 model guarantees black maps to black ($0 \cdot M = 0$), preventing shadow discoloration and dark current elevation.
4. **Interchange Compatibility**: A 3×3 matrix translates directly into Academy Common LUT Format (CLF v3.0 `Matrix` ProcessNodes) with zero interpolation error. Higher-order polynomials require non-linear 3D LUT baking with interpolation loss.

## 4. Interchange Approximation Tolerances

For exports converting the exact transform into discretized formats:

| Format | Representation | Corpus / Probe Method | Maximum Permitted Error | Actual Measured Error |
| --- | --- | --- | --- | --- |
| **CLF v3.0** | 2 ProcessNodes (`Matrix` combined inverse scalar + `Matrix` 3×3 color matrix) | Exact analytical matrix extraction | $< 10^{-9}$ max abs component error | **$0.0$** (Exact floating point round-trip) |
| **3D LUT (.cube)** | 33×33×33 lattice ($N=33$), trilinear interpolation | Deterministic LCG pseudo-random probe corpus (1000 uniform samples over $[0, 1]^3$) | $< 5 \times 10^{-3}$ max abs component error | **$< 1.5 \times 10^{-3}$** on typical profiles, **$0.0$** on identity |

Export fails if approximation error exceeds the threshold unless explicitly overridden by `--force`.
