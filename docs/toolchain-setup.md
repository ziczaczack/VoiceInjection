# Stage 4 Toolchain Setup — Lessons Learned

This document records why getting `whisper-rs` + CUDA building on this machine took so long, and what the fixes were. Save your future self some pain: if any of these symptoms reappear, jump straight to the relevant section.

## Target stack

| Component | Version | Where |
|---|---|---|
| CUDA Toolkit | 13.2 | `D:\NVIDIA\CUDA\v13.2` |
| LLVM (libclang) | 22.1.5 | `D:\Program Files\LLVM\` |
| CMake | 4.3.2 | `D:\Program Files\bin\cmake.exe` |
| MSVC | VS 2022 | (standard install) |
| GPU | RTX 4060 (Ada, sm_89) | — |
| whisper-rs | 0.16.0 | crates.io |
| whisper-rs-sys | 0.15.0 (bindgen 0.72) | crates.io |

## Required persisted env vars (User scope)

```
LIBCLANG_PATH            = D:\Program Files\LLVM\bin
CMAKE_CUDA_ARCHITECTURES = 89
CL                       = /Zc:preprocessor
CUDA_PATH                = D:\NVIDIA\CUDA\v13.2
```

`WHISPER_DONT_GENERATE_BINDINGS` should **not** be set. The bundled bindings shipped in whisper-rs-sys were generated on Linux and contain glibc types unusable on Windows.

---

## The dependency chain

```
Your Rust code (lib.rs)
   ↓
whisper-rs (Rust wrapper)
   ↓
whisper-rs-sys (FFI bindings)     ← bindgen runs here at build time
   ↓
whisper.cpp (C/C++ source)
   ↓
ggml (C/C++/CUDA kernels)         ← cmake + cl.exe + nvcc compile this
   ↓
CUDA Toolkit 13.2 + MSVC 2022 + LLVM 22 (libclang)
```

Every layer contributed a problem. They are ordered below by when they surfaced during the build.

---

## Blocker 1 — CUDA 13 dropped old GPU architectures

**Symptom:** `Unsupported gpu architecture 'compute_52'`

**Why:** ggml's `CMakeLists.txt` defaults to the arch list `52;61;70;75` (Maxwell → Turing) when nothing is specified. CUDA 13 removed sm_5x (Maxwell) and sm_6x (Pascal). Minimum is now sm_75 (Turing).

**Fix:** `CMAKE_CUDA_ARCHITECTURES=89` (RTX 4060 native arch). ggml uses `if (NOT DEFINED CMAKE_CUDA_ARCHITECTURES)` so the env var wins.

---

## Blocker 2 — CUDA 13's CCCL needs the modern MSVC preprocessor

**Symptom:** `MSVC/cl.exe with traditional preprocessor is used. Please switch to the standard conforming preprocessor by passing /Zc:preprocessor`

**Why:** CUDA 13 ships CCCL (CUDA C++ Core Libraries) which uses `__VA_OPT__` and other modern preprocessor features. MSVC's default preprocessor is from the 1990s. The conforming version exists since MSVC 2019 16.5 but is opt-in.

**Fix:** Set `CL=/Zc:preprocessor`. Anything in the `CL` env var is auto-prepended to every `cl.exe` invocation — saves having to thread the flag through CMake.

---

## Blocker 3 — ggml hardcoded C++11 for non-SYCL builds (only affects old whisper-rs-sys)

**Symptom:** `CUB requires at least C++17`

**Why:** Older ggml had:
```cmake
if (GGML_SYCL)
    set(CMAKE_CXX_STANDARD 17)
else()
    set(CMAKE_CXX_STANDARD 11)
endif()
```
Modern CUB (CUDA Unbound, pulled in via CCCL) requires C++17. CUDA ≤12 was lenient about C++11; CUDA 13 isn't.

**Fix (legacy):** Patch to `if (GGML_SYCL OR GGML_CUDA)`. Must patch both the registry source-cache copy AND the `target/.../whisper-rs-sys-*/out/` copy, because cargo re-extracts the tarball on fresh builds.

**Fix (now):** **Already resolved upstream in whisper-rs-sys 0.15.0** — the bundled whisper.cpp sets C++17 unconditionally. No patching needed if you stay on ≥0.15.

---

## Blocker 4 (the worst) — bindgen 0.69 + libclang 22 = opaque struct

**Symptom:** 60+ Rust errors like `no field 'greedy' on type 'whisper_full_params'`. Bindings file had:
```rust
pub struct whisper_full_params {
    pub _address: u8,    // opaque fallback
}
```
with a layout test asserting `size = 264, align = 8`. bindgen *knew the size* but produced no fields.

**Root cause:** `whisper_full_params` contains nested anonymous structs:
```c
struct whisper_full_params {
    // ...
    struct { int best_of; } greedy;
    struct { int beam_size; float patience; } beam_search;
};
```
libclang 22 changed how it represents synthetic cursor names for anonymous nested types. bindgen 0.69.5 (released March 2024, predates LLVM 19) couldn't translate the new AST shape, gave up on the outer struct, emitted `_address: u8`. Size came from a different code path (`sizeof()` query) which still worked. **bindgen did not loudly error** — it silently produced unusable bindings.

**The trap we fell into:** Tried `WHISPER_DONT_GENERATE_BINDINGS=1` to use the pre-bundled `src/bindings.rs`. Those were generated **on Linux** — full of glibc-only types like `_G_fpos_t`, `_IO_FILE` whose sizes don't match Windows MSVC. Compilation failed with size-assertion overflows.

**Env var stickiness gotcha:** whisper-rs-sys's `build.rs` explicitly excludes `WHISPER_DONT_GENERATE_BINDINGS` from cargo's rerun-if-env-changed triggers:
```rust
let is_whisper_flag = key.starts_with("WHISPER_") && key != "WHISPER_DONT_GENERATE_BINDINGS";
```
So toggling that var does NOT cause cargo to rebuild. You must manually `rm -rf target/.../whisper-rs-sys-*` to force re-extraction.

**Fix:** Upgrade to whisper-rs-sys 0.15 (uses bindgen 0.72). bindgen 0.72 handles libclang 22's anonymous-struct AST correctly.

---

## Blocker 5 — API breakage after upgrade

**Symptom (compile errors after upgrading to 0.16):**
- `no method named 'context' found for type 'i32'` on `full_n_segments()`
- `no method named 'full_get_segment_text'`

**Why:** whisper-rs 0.16 reorganized the segment API:
- `full_n_segments()` returns `c_int` directly (no `Result`)
- Segments are now first-class objects: `get_segment(i) → Option<WhisperSegment>` with methods `.to_str_lossy()`, `.start_timestamp()`, etc.

**Fix (already applied in `src-tauri/src/transcribe/local.rs`):**
```rust
let n = state.full_n_segments();
for i in 0..n {
    let seg = state.get_segment(i).ok_or_else(|| anyhow!("segment {i} missing"))?;
    let s = seg.to_str_lossy().context("segment text")?;
    text.push_str(&s);
}
```

---

## LLVM install sub-saga

**Slow GitHub release download from China:** Use a mirror prefix like `gh-proxy.com` or `mirror.ghproxy.com`, or a multi-threaded downloader (IDM/Motrix), or `scoop install llvm`.

**"Path too long, installer cannot edit path":** Windows has a 2047-character hard limit on the PATH env var and yours is near full. The LLVM installer wants to append `D:\Program Files\LLVM\bin` and fails the whole step when PATH is too long.

**Workaround:** Skip the PATH option in the installer entirely. Set `LIBCLANG_PATH=D:\Program Files\LLVM\bin` as a User env var instead — bindgen reads this directly to find `libclang.dll` without needing PATH modification.

---

## The meta-reason this took so long

This stack combines three toolchains that were each released around late 2025:
- CUDA 13.2
- LLVM 22.1.5
- MSVC 2022 with new CUDA 13 integration

against a Rust crate (whisper-rs 0.13) released earlier in 2025, before any of the above existed. Each maintainer tested against the *previous* generation of its neighbors, so the cracks all show up at the seams.

**Version-skew rule of thumb:** when you're on the latest of *one* toolchain, expect to either upgrade or downgrade *all* the others to match it. Mixing cutting-edge with year-old crates guarantees pain.

**Counterfactual:** On CUDA 12.6 + LLVM 19 + whisper-rs 0.13 every blocker above would have vanished and `cargo check` would have worked first try.

---

## Quick recovery checklist

If `cargo check` breaks after a future LLVM/CUDA upgrade or a `cargo update`:

1. **Check env vars** — all four (`LIBCLANG_PATH`, `CMAKE_CUDA_ARCHITECTURES`, `CL`, `CUDA_PATH`) still set?
2. **Check arch list** — did CUDA drop your `CMAKE_CUDA_ARCHITECTURES` value?
3. **Check C++ standard** — `grep CMAKE_CXX_STANDARD` in the active ggml CMakeLists; should be 17+ for CUDA.
4. **Check bindgen version vs LLVM** — `cargo tree | grep bindgen` should be within ~1 minor of current LLVM. If LLVM is much newer, upgrade whisper-rs.
5. **Don't use bundled bindings on Windows** — `WHISPER_DONT_GENERATE_BINDINGS` should be unset.
6. **Wipe and re-extract** — if anything looks cached-stale: `Remove-Item -Recurse -Force target/debug/build/whisper-rs-sys-*`.
