# FPS benchmark protocol v1

Status: specification only. The launcher does not yet install a benchmark mod, drive a camera path, or collect frame timings.

This protocol compares Minecraft performance profiles on the same computer. It is not a benchmark of launcher startup or download speed.

## 1. Comparison setup

Compare a baseline profile (normally vanilla) with a candidate profile. Keep these conditions identical:

- Minecraft version, loader version, Java runtime and JVM arguments
- installed mod file names, versions and SHA-256 hashes
- world file and seed, player position, camera route, weather and time of day
- resolution, window mode, render distance, graphics settings, frame cap and VSync
- GPU driver, operating-system power mode and other test conditions

Record hardware and software details with each run. Close unrelated GPU/CPU-heavy applications. Do not change settings between profiles. Alternate profile order between comparisons to reduce order and thermal bias.

A run consists of a **60-second warm-up**, followed by **180 seconds of measurement**. Perform **three measured runs per profile**. Use the same saved world and repeatable camera route for every run. If the route or world cannot be reproduced, label the comparison as non-repeatable.

## 2. Measurements

Collect frame times in milliseconds or nanoseconds from an in-game instrumentation source. Do not infer frame-time samples from a single FPS counter.

For each run, report:

- mean FPS over the measured interval
- 1% low and 0.1% low
- p50, p95 and p99 frame time
- peak Java process memory, if available
- game launch-to-ready time, reported separately from FPS
- crashes, disconnects, missing samples or other run errors

Define 1% low as **1000 divided by the arithmetic mean frame time of the slowest 1% of measured frames**. Define 0.1% low the same way for the slowest 0.1%. Percentile frame times use the nearest-rank method over all measured frame times. Keep raw samples or a lossless equivalent so the summary can be recalculated.

## 3. Aggregation and interpretation

Keep all three run records. For each profile, report the median of the three run-level mean FPS values and the median of the three run-level 1% lows. Also report the minimum and maximum for both values.

A profile is **unstable** if the range across its three runs exceeds 3% of the median for either mean FPS or 1% low:

`(max - min) / median > 0.03`

Do not claim an improvement from an unstable comparison. Mark incomplete runs and exclude them from the three-run aggregate; repeat them under the same conditions.

For v1, call a candidate **promising** only when the median mean FPS and median 1% low each improve by at least 5% over baseline, neither profile is unstable, no run crashes, and median p99 frame time does not regress by more than 5%. This is a comparison rule, not a promised result on every computer.

Percentage change is `(candidate / baseline - 1) * 100`. Show the measured values and each run alongside percentages.

## 4. Run record

Save one UTF-8 JSON record per run. Use this shape; fields marked nullable may be `null` when the source cannot measure them.

```json
{
  "protocol_version": 1,
  "run_id": "uuid",
  "profile_id": "optimized",
  "comparison_id": "uuid",
  "started_at_utc": "2026-01-01T12:00:00Z",
  "minecraft": {
    "version": "1.21.1",
    "loader": "fabric",
    "loader_version": "0.16.14",
    "java_version": "21.0.5",
    "jvm_args_sha256": "hex"
  },
  "mods": [
    { "file": "sodium.jar", "version": "x.y.z", "sha256": "hex" }
  ],
  "hardware": {
    "os": "Windows 11",
    "cpu": "recorded by user or detector",
    "gpu": "recorded by user or detector",
    "gpu_driver": "recorded version",
    "ram_bytes": 0
  },
  "scenario": {
    "world_sha256": "hex",
    "seed": "redacted-or-recorded",
    "route_id": "route-v1",
    "resolution": [1920, 1080],
    "render_distance_chunks": 12,
    "settings_sha256": "hex",
    "warmup_seconds": 60,
    "measurement_seconds": 180
  },
  "measurements": {
    "frames": 0,
    "mean_fps": 0.0,
    "low_1_percent_fps": 0.0,
    "low_0_1_percent_fps": 0.0,
    "frametime_ms": { "p50": 0.0, "p95": 0.0, "p99": 0.0 },
    "peak_java_memory_bytes": null,
    "launch_to_ready_ms": null
  },
  "result": "complete",
  "notes": []
}
```

The example values are placeholders. A producer must use the actual values or `null` where permitted; it must never write fabricated measurements. Store raw frame-time samples in a separate file when they would make the JSON record too large, and include its relative path and SHA-256 in the record.

## 5. Privacy and permissions

Keep benchmark records on the user's device by default, under the user's profile. Do not require administrator rights. Hardware identifiers, world data, seeds, paths and logs can be sensitive; ask before sharing any report and provide an export that the user can inspect. Do not upload telemetry automatically.

## 6. Implementation boundary

This document defines the comparison contract only. A future in-game measurement component must declare its data source, precision and known limitations, and the launcher must preserve protocol version and raw per-run results. Until that exists, the UI must label benchmark values as unavailable rather than display simulated FPS.
