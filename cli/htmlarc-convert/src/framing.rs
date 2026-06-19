//! The `framing` command: evaluate per-bundle string-block compression *framings* on a built
//! `.htmlarc`, to choose the slice granularity and zstd level a future `--compression` flag would
//! map to — before any on-disk format change.
//!
//! Today each bundle's text/comment pool lives in one **uncompressed** `BundleStrings` block,
//! borrowed zero-copy (`StringSource::Plain`). The plan is to split a bundle's ≤1000 documents into
//! *slices* of `S` docs, compress each slice as one zstd frame (optionally against a per-bundle
//! trained dictionary), and read through the dormant `StringSource::Lazy` seam. Slice size trades
//! compression ratio against random-access cost: a single-doc read must inflate its whole slice
//! (cost ∝ `S`), while a sequential sweep inflates each slice once.
//!
//! This command measures that trade-off on the **real on-disk bytes**: it opens the archive, reads
//! every bundle's `BundleStrings` segments (the exact bytes being prepared for compression), and
//! for each `(docs/slice, zstd level)` reports compressed size + ratio and the two decisive decode
//! costs — *cold get* (decompress one slice = random single-doc read) and *seq scan* (decompress
//! every slice once = bundle-sequential read). It also measures the per-doc + per-bundle-dict point
//! (`1+dict`), the "cheap random access, ratio recovered by a shared dict" candidate.
//!
//! Nothing here writes a new format; it is a pure read-and-simulate probe over a current-format
//! archive (build one with `convert`).

use std::time::Instant;

use anyhow::Result;
use htmlarc_archive::MmapArchive;

use crate::args::Framing;

/// docs-per-slice grid. Capped at 64: larger slices impose an unacceptable random-access penalty
/// (a single-doc read inflates its whole slice — at S=256/1000 cold-get is 6–25 ms on real web
/// text, two-to-three orders over per-doc). The decision lives in the 1..=64 band, where the
/// per-bundle dict is the lever for recovering ratio without growing the slice. Override with
/// `FRAMING_SLICES=1,8,64` (comma-separated) to probe outside it.
const DEFAULT_SLICES: &[usize] = &[1, 4, 8, 16, 32, 64];
/// zstd level grid. Capped at 12: levels above it compress too slowly to be acceptable at convert
/// time (the `compress MiB/s` column shows the cliff — L15/L19 fall to single-digit MiB/s/core,
/// hours over the full corpus). Override with `FRAMING_LEVELS=3,19` to probe outside it.
const DEFAULT_LEVELS: &[i32] = &[3, 6, 9, 12];
/// Per-bundle trained-dictionary cap (matches ADR 0001's reference: 110 KiB).
const DICT_MAX: usize = 112_640;
/// Docs sampled (strided across all bundles) to train the single archive-wide ("central") dict.
const GLOBAL_SAMPLE: usize = 2_000;
/// Decode timing passes; the minimum total is reported to damp scheduler noise.
const DECODE_PASSES: u32 = 3;

/// Every document's relocated string segment, grouped by bundle: `bundles[b][slot]` is document
/// `slot` of bundle `b`. Owned (copied out of the mmap once) so the sweep can reuse it across the
/// whole `level × slice` grid without re-reading the file.
type Bundles = Vec<Vec<Vec<u8>>>;

pub(crate) fn run(args: Framing) -> Result<()> {
    let Framing { input } = args;
    if input.is_empty() {
        anyhow::bail!("usage: framing <archive.htmlarc> [more.htmlarc ...]");
    }

    // Pool every archive's documents into one analysis (one input = the normal case; several =
    // study a multilingual mix, e.g. an English and a Chinese archive together).
    let mut bundles: Bundles = Vec::new();
    for path in &input {
        let mmap = MmapArchive::open(path)?;
        let decoder = mmap.decoder();
        for b in 0..mmap.bundle_count() {
            let bs = mmap.bundle_strings(b)?;
            // The block now stores compressed per-document frames (format v10); inflate each back
            // to its raw text so the framing experiments run on the same bytes as before.
            let docs = (0..bs.doc_count())
                .map(|slot| decoder.decode(bs.frame(slot), bs.raw_len(slot) as usize))
                .collect();
            bundles.push(docs);
        }
    }

    let slices = parse_env_usize("FRAMING_SLICES").unwrap_or_else(|| DEFAULT_SLICES.to_vec());
    let levels = parse_env_i32("FRAMING_LEVELS").unwrap_or_else(|| DEFAULT_LEVELS.to_vec());
    // FRAMING_GLOBAL=1 adds a `+gdict` row: ONE archive-wide ("central") dict, trained once and
    // reused everywhere — the per-bundle dict's ratio win without its per-bundle training cost.
    let global = std::env::var("FRAMING_GLOBAL").is_ok();

    let doc_count: usize = bundles.iter().map(Vec::len).sum();
    let raw: usize = bundles.iter().flat_map(|d| d.iter()).map(Vec::len).sum();

    let names: Vec<String> = input.iter().map(|p| p.display().to_string()).collect();
    println!(
        "Archive(s): {}\nBundles: {}   Documents: {}   Bundle string block (uncompressed, current on-disk): {}",
        names.join(", "),
        bundles.len(),
        doc_count,
        human_bytes(raw as u64),
    );
    if raw == 0 {
        println!("\nNo relocated string bytes to evaluate (empty text pools).");
        return Ok(());
    }

    // FRAMING_LANG=1: per-script dictionary-effectiveness breakdown (plain vs central vs a
    // language-matched dict), per-doc (S=1), at the first level in the grid. Answers "does one
    // central dict generalise across languages, or does the minority language need its own?".
    if std::env::var("FRAMING_LANG").is_ok() {
        run_lang(&bundles, *levels.first().unwrap_or(&3), raw);
        return Ok(());
    }

    // FRAMING_SAMPLE=1: how the global dict's effectiveness and single-threaded training time scale
    // with the training sample size — prefix (first-N docs, the two-phase warm-up) vs strided.
    if std::env::var("FRAMING_SAMPLE").is_ok() {
        run_sample(&bundles, *levels.first().unwrap_or(&3), raw);
        return Ok(());
    }

    for &level in &levels {
        println!("\n=== zstd level {level} ===");
        println!(
            "{:>11}  {:>10}  {:>7}  {:>13}  {:>15}  {:>15}",
            "docs/slice", "size", "ratio", "cold get µs", "seq scan MiB/s", "compress MiB/s"
        );
        println!("{:-<82}", "");

        // Every slice size is measured both without and with a per-bundle trained dict, so the
        // dict's marginal value is visible at each granularity (not just per-doc). The dict is
        // trained per bundle, stored once per bundle, and shared by all of that bundle's slices.
        let mut dict_overhead = 0u64;
        for &s in &slices {
            let plain = measure(&bundles, level, s, false);
            print_row(&format!("{s}"), &plain, raw);
            let withd = measure(&bundles, level, s, true);
            print_row(&format!("{s}+dict"), &withd, raw);
            dict_overhead = withd.dict_bytes; // constant across slice sizes
            if global {
                let g = measure_global(&bundles, level, s);
                print_row(&format!("{s}+gdict"), &g, raw);
            }
        }
        println!(
            "             └ per-bundle dict overhead included in every +dict size: {} across {} bundles (constant)",
            human_bytes(dict_overhead),
            bundles.len(),
        );
    }

    println!(
        "\nHow to read this:\n  \
         • cold get µs   = decompress ONE slice (the cost of a random single-doc read).\n  \
         • seq scan      = decompress every slice once (a bundle-sequential read / export sweep).\n  \
         • compress MiB/s = single-core compress throughput = the convert-time cost; it is the\n  \
         reason the level grid stops at 12 (higher levels fall off a cliff). Decompress speed (seq\n  \
         scan) is level-independent, so a higher level is paid once at build, never at read.\n  \
         • size          = compressed bytes incl. per-slice index overhead (4·(docs+1)+4 per slice).\n  \
         Slices are capped at 64 (larger ⇒ unacceptable random latency). Within 1..=64 the lever\n  \
         is the dict: each `S+dict` row shows the ratio a per-bundle dict buys over the plain `S`\n  \
         row at (near) the same decode cost — the dict supplies the cross-slice context a small\n  \
         slice can't see. (Dict cold-get assumes the per-bundle dict digest is cached; a truly cold\n  \
         first touch of a bundle adds a one-time dict-digest cost.) Pick the (slice, level, dict)\n  \
         a `--compression` preset maps to from the best ratio whose cold-get is still acceptable."
    );
    Ok(())
}

/// One framing's measured totals over the whole archive.
struct Measure {
    comp: usize, // compressed bytes incl. per-slice index overhead (+ per-bundle dict if any)
    decode_ns: u128, // min over passes: decompress every slice once (seq-scan cost)
    units: usize, // slices — the decode-unit count (cold-get = one unit)
    build_ns: u128, // compress wall time over all slices (one pass)
    dict_bytes: u64, // per-bundle dict overhead (0 when use_dict = false)
}

/// Per-slice index overhead a real reader carries: `u32 doc_count` + `u32 base[doc_count+1]`.
fn index_bytes(docs_in_slice: usize) -> usize {
    4 + 4 * (docs_in_slice + 1)
}

/// One bundle's compressed slice frames plus the dictionary they share (empty if `use_dict` is off
/// or training failed). Grouping by bundle lets the decode pass digest each per-bundle dict **once**
/// (a real reader caches it per bundle), rather than re-digesting it on every slice.
struct BundleFrames {
    dict: Vec<u8>,
    frames: Vec<(Vec<u8>, usize)>, // (compressed slice, raw len)
}

/// Chunk each bundle into `s`-doc slices and zstd each whole slice at `level`, optionally against a
/// per-bundle trained dictionary (`use_dict`). Times decompressing every slice (the seq-scan cost;
/// the per-slice mean is the cold-get / random single-doc cost). With a dict, one decompressor is
/// built per bundle and reused, so the dict is digested once — the realistic cached-dict reader.
///
/// The same dict supplies cross-*slice* context to every slice in the bundle, so this measures the
/// dict's marginal value at *every* slice size, not just per-doc — a small slice still benefits from
/// boilerplate it shares with the other slices, which a dict-less frame cannot see.
fn measure(bundles: &Bundles, level: i32, s: usize, use_dict: bool) -> Measure {
    let s = s.max(1);
    let mut comp = 0usize;
    let mut units = 0usize;
    let mut dict_bytes = 0u64;
    let mut groups: Vec<BundleFrames> = Vec::with_capacity(bundles.len());

    let t_build = Instant::now();
    for docs in bundles {
        // Train one dict per bundle from its per-doc samples (independent of slice size). Bundles
        // too small/short to train fall back to dict-less frames.
        let dict = if use_dict {
            match zstd::dict::from_samples(docs, DICT_MAX) {
                Ok(d) if !d.is_empty() => d,
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        comp += dict.len();
        dict_bytes += dict.len() as u64;

        // Compress this bundle's slices in an inner scope so the dictionary compressor is dropped
        // before `dict` is moved into `groups`.
        let frames: Vec<(Vec<u8>, usize)> = {
            let mut dict_comp: Option<zstd::bulk::Compressor<'static>> = (!dict.is_empty())
                .then(|| zstd::bulk::Compressor::with_dictionary(level, &dict).unwrap());
            let mut out = Vec::new();
            for chunk in docs.chunks(s) {
                let raw_len: usize = chunk.iter().map(Vec::len).sum();
                let mut buf = Vec::with_capacity(raw_len);
                for d in chunk {
                    buf.extend_from_slice(d);
                }
                let frame = match dict_comp.as_mut() {
                    Some(c) => c.compress(&buf).unwrap(),
                    None => zstd::bulk::compress(&buf, level).unwrap(),
                };
                comp += frame.len() + index_bytes(chunk.len());
                out.push((frame, raw_len));
                units += 1;
            }
            out
        };
        groups.push(BundleFrames { dict, frames });
    }
    let build_ns = t_build.elapsed().as_nanos();

    // Timed decode passes (min). One decompressor per bundle, reused across its slices.
    let mut decode_ns = u128::MAX;
    for _ in 0..DECODE_PASSES {
        let t = Instant::now();
        for g in &groups {
            if g.dict.is_empty() {
                for (frame, raw_len) in &g.frames {
                    let out = zstd::bulk::decompress(frame, *raw_len).unwrap();
                    debug_assert_eq!(out.len(), *raw_len);
                }
            } else {
                let mut dec = zstd::bulk::Decompressor::with_dictionary(&g.dict).unwrap();
                for (frame, raw_len) in &g.frames {
                    let out = dec.decompress(frame, *raw_len).unwrap();
                    debug_assert_eq!(out.len(), *raw_len);
                }
            }
        }
        decode_ns = decode_ns.min(t.elapsed().as_nanos());
    }

    Measure {
        comp,
        decode_ns,
        units,
        build_ns,
        dict_bytes,
    }
}

/// Compression against ONE archive-wide ("central") dictionary, trained once over a strided sample
/// of all bundles' docs and reused for every slice in every bundle. Contrast with [`measure`]'s
/// per-bundle dict: training is a single up-front cost (not per bundle) and the dict is stored once
/// for the whole archive, so convert throughput stays near plain while still capturing corpus-wide
/// redundancy. Decode uses one decompressor for the whole archive (the central dict digested once).
fn measure_global(bundles: &Bundles, level: i32, s: usize) -> Measure {
    let s = s.max(1);
    // Train one dict over a strided sample across all bundles (≈ GLOBAL_SAMPLE docs).
    let all: Vec<&Vec<u8>> = bundles.iter().flatten().collect();
    let stride = (all.len() / GLOBAL_SAMPLE).max(1);
    let sample: Vec<&[u8]> = all.iter().step_by(stride).map(|d| d.as_slice()).collect();

    let t_build = Instant::now();
    let dict = match zstd::dict::from_samples(&sample, DICT_MAX) {
        Ok(d) if !d.is_empty() => d,
        _ => Vec::new(),
    };
    let dict_bytes = dict.len() as u64;
    let mut comp = dict.len(); // stored ONCE for the whole archive
    let mut units = 0usize;
    let mut frames: Vec<(Vec<u8>, usize)> = Vec::new();
    {
        let mut dict_comp: Option<zstd::bulk::Compressor<'static>> = (!dict.is_empty())
            .then(|| zstd::bulk::Compressor::with_dictionary(level, &dict).unwrap());
        for docs in bundles {
            for chunk in docs.chunks(s) {
                let raw_len: usize = chunk.iter().map(Vec::len).sum();
                let mut buf = Vec::with_capacity(raw_len);
                for d in chunk {
                    buf.extend_from_slice(d);
                }
                let frame = match dict_comp.as_mut() {
                    Some(c) => c.compress(&buf).unwrap(),
                    None => zstd::bulk::compress(&buf, level).unwrap(),
                };
                comp += frame.len() + index_bytes(chunk.len());
                frames.push((frame, raw_len));
                units += 1;
            }
        }
    }
    let build_ns = t_build.elapsed().as_nanos();

    let mut decode_ns = u128::MAX;
    for _ in 0..DECODE_PASSES {
        let t = Instant::now();
        if dict.is_empty() {
            for (frame, raw_len) in &frames {
                let out = zstd::bulk::decompress(frame, *raw_len).unwrap();
                debug_assert_eq!(out.len(), *raw_len);
            }
        } else {
            let mut dec = zstd::bulk::Decompressor::with_dictionary(&dict).unwrap();
            for (frame, raw_len) in &frames {
                let out = dec.decompress(frame, *raw_len).unwrap();
                debug_assert_eq!(out.len(), *raw_len);
            }
        }
        decode_ns = decode_ns.min(t.elapsed().as_nanos());
    }

    Measure {
        comp,
        decode_ns,
        units,
        build_ns,
        dict_bytes,
    }
}

/// Compress every doc against `dict` (per-doc, S=1) and return the total compressed payload bytes.
/// One compressor, reused across docs (the dict is digested once). Empty dict ⇒ dict-less per-doc.
fn compress_all(all: &[&[u8]], level: i32, dict: &[u8]) -> usize {
    if dict.is_empty() {
        return all
            .iter()
            .map(|d| zstd::bulk::compress(d, level).unwrap().len())
            .sum();
    }
    let mut c = zstd::bulk::Compressor::with_dictionary(level, dict).unwrap();
    all.iter().map(|d| c.compress(d).unwrap().len()).sum()
}

/// How the global dict's effectiveness and (single-threaded) training time scale with the number of
/// training docs. For each sample size N it trains on the first N docs (`prefix` — what the
/// two-phase warm-up gathers) and on N docs strided across the archive (`strided` — representative),
/// then compresses *every* doc against each dict. The plateau is the doc count worth gathering in
/// phase 1; `train ms` is the serial stall while bundles queue; prefix vs strided shows whether
/// training on the first bundles costs anything.
fn run_sample(bundles: &Bundles, level: i32, raw: usize) {
    let all: Vec<&[u8]> = bundles.iter().flatten().map(Vec::as_slice).collect();
    let total = all.len();
    let r = |comp: usize| raw as f64 / comp.max(1) as f64;

    println!(
        "\n=== Global-dict training sample-size sensitivity (zstd level {level}, per-doc / S=1) ==="
    );
    println!(
        "ratio = raw / compressed payload (one global dict, stored once ≈ dict KiB — negligible).\n\
         prefix = first N docs (the two-phase warm-up gathers these); strided = N spread across.\n"
    );
    println!(
        "{:>11}  {:>10}  {:>8}  {:>9}  {:>13}  {:>13}",
        "sample docs", "sample", "dict KiB", "train ms", "prefix ratio", "strided ratio"
    );
    println!("{:-<78}", "");

    let plain = compress_all(&all, level, &[]);
    println!(
        "{:>11}  {:>10}  {:>8}  {:>9}  {:>12.3}×  {:>13}",
        "0 (plain)",
        "—",
        "—",
        "—",
        r(plain),
        "—"
    );

    let mut sizes: Vec<usize> = [250, 500, 1000, 2000, 5000, 10000, total]
        .into_iter()
        .filter(|&n| n > 0 && n <= total)
        .collect();
    sizes.dedup();
    for &n in &sizes {
        let prefix: Vec<&[u8]> = all[..n].to_vec();
        let sample_bytes: usize = prefix.iter().map(|d| d.len()).sum();
        let t = Instant::now();
        let dict_p = train_dict(&prefix);
        let train_ms = t.elapsed().as_millis();
        let r_prefix = r(compress_all(&all, level, &dict_p));

        // Strided is identical to prefix at the full size; skip the redundant work.
        let strided_cell = if n == total {
            "= prefix".to_string()
        } else {
            let dict_s = train_dict(&sample_strided(&all, n));
            format!("{:.3}×", r(compress_all(&all, level, &dict_s)))
        };

        println!(
            "{:>11}  {:>10}  {:>8}  {:>9}  {:>12.3}×  {:>13}",
            n,
            human_bytes(sample_bytes as u64),
            dict_p.len() / 1024,
            train_ms,
            r_prefix,
            strided_cell,
        );
    }
    println!(
        "\nRead this as: the ratio plateaus at the sample size worth gathering before training;\n\
         `train ms` is the single-threaded phase-1 stall; prefix≈strided means training on the\n\
         first bundles loses nothing (no need to scatter the sample)."
    );
}

/// A coarse script class for a document, from the dominant script among its "letter" characters.
/// A heuristic (not language detection): enough to separate Latin / CJK / Cyrillic / Arabic text so
/// we can see whether a dictionary helps *that* script's docs.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Script {
    Latin,
    Cjk,
    Cyrillic,
    Arabic,
    Other,
}

/// Fixed print/iteration order.
const SCRIPTS: [Script; 5] = [
    Script::Latin,
    Script::Cjk,
    Script::Cyrillic,
    Script::Arabic,
    Script::Other,
];

fn script_name(s: Script) -> &'static str {
    match s {
        Script::Latin => "Latin",
        Script::Cjk => "CJK",
        Script::Cyrillic => "Cyrillic",
        Script::Arabic => "Arabic",
        Script::Other => "other",
    }
}

/// Classify a document's text by its dominant script. The block is a UTF-8 invariant; a non-UTF-8
/// segment (should not happen) falls to `Other`.
fn classify(bytes: &[u8]) -> Script {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Script::Other;
    };
    let (mut latin, mut cjk, mut cyr, mut arab) = (0u64, 0u64, 0u64, 0u64);
    for c in text.chars() {
        match c as u32 {
            0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF => cjk += 1,
            0x0400..=0x04FF => cyr += 1,
            0x0600..=0x06FF | 0x0750..=0x077F => arab += 1,
            0x41..=0x5A | 0x61..=0x7A | 0x00C0..=0x024F => latin += 1,
            _ => {}
        }
    }
    let max = latin.max(cjk).max(cyr).max(arab);
    if max == 0 {
        Script::Other
    } else if max == cjk {
        Script::Cjk
    } else if max == cyr {
        Script::Cyrillic
    } else if max == arab {
        Script::Arabic
    } else {
        Script::Latin
    }
}

/// A strided sample of `target` items (every `len/target`-th), for dictionary training.
fn sample_strided<'a>(items: &[&'a [u8]], target: usize) -> Vec<&'a [u8]> {
    let stride = (items.len() / target.max(1)).max(1);
    items.iter().step_by(stride).copied().collect()
}

/// Train one dict over `samples`; empty if there are too few/short samples to train.
fn train_dict(samples: &[&[u8]]) -> Vec<u8> {
    if samples.len() < 8 {
        return Vec::new();
    }
    match zstd::dict::from_samples(samples, DICT_MAX) {
        Ok(d) if !d.is_empty() => d,
        _ => Vec::new(),
    }
}

#[derive(Default)]
struct Bucket {
    docs: u64,
    raw: u64,
    plain: u64,    // sum of per-doc compressed sizes, no dict
    central: u64,  // …against the one archive-wide dict
    per_lang: u64, // …against a dict trained on this script's docs
}

/// Per-script dictionary-effectiveness comparison (per-doc, S=1): for each script bucket, the
/// payload ratio under (a) no dict, (b) one central dict trained over a sample of the *whole*
/// archive, and (c) a dict trained on *that script's* docs. Isolates whether a single central dict
/// generalises across languages, or whether the minority language needs its own dictionary.
fn run_lang(bundles: &Bundles, level: i32, _raw: usize) {
    // Classify every doc once; group references by script for per-language training.
    let all: Vec<(&[u8], Script)> = bundles
        .iter()
        .flatten()
        .map(|d| (d.as_slice(), classify(d)))
        .collect();

    let docs_only: Vec<&[u8]> = all.iter().map(|&(d, _)| d).collect();
    let mut by_script: std::collections::HashMap<Script, Vec<&[u8]>> =
        std::collections::HashMap::new();
    for &(d, sc) in &all {
        by_script.entry(sc).or_default().push(d);
    }

    // Train the central dict (whole-archive sample) and one dict per script.
    let central = train_dict(&sample_strided(&docs_only, GLOBAL_SAMPLE));
    let lang_dicts: std::collections::HashMap<Script, Vec<u8>> = by_script
        .iter()
        .map(|(&sc, docs)| (sc, train_dict(&sample_strided(docs, GLOBAL_SAMPLE))))
        .collect();

    // Reusable compressors (one central, one per script that trained a dict).
    let mut central_c = (!central.is_empty())
        .then(|| zstd::bulk::Compressor::with_dictionary(level, &central).unwrap());
    let mut lang_c: std::collections::HashMap<Script, zstd::bulk::Compressor<'static>> = lang_dicts
        .iter()
        .filter(|(_, d)| !d.is_empty())
        .map(|(&sc, d)| {
            (
                sc,
                zstd::bulk::Compressor::with_dictionary(level, d).unwrap(),
            )
        })
        .collect();

    let mut buckets: std::collections::HashMap<Script, Bucket> = std::collections::HashMap::new();
    for &(d, sc) in &all {
        // Plain once; reuse as the fallback when a strategy has no dict for this doc.
        let p = zstd::bulk::compress(d, level).unwrap().len() as u64;
        let cen = match central_c.as_mut() {
            Some(c) => c.compress(d).unwrap().len() as u64,
            None => p,
        };
        let lng = match lang_c.get_mut(&sc) {
            Some(c) => c.compress(d).unwrap().len() as u64,
            None => p,
        };
        let b = buckets.entry(sc).or_default();
        b.docs += 1;
        b.raw += d.len() as u64;
        b.plain += p;
        b.central += cen;
        b.per_lang += lng;
    }

    println!("\n=== Per-script dictionary effectiveness (zstd level {level}, per-doc / S=1) ===");
    println!(
        "ratio = raw / compressed payload (dict bytes excluded — reported below). central = one\n\
         archive-wide dict; per-lang = a dict trained on that script's own docs.\n"
    );
    println!(
        "{:>9}  {:>7}  {:>9}  {:>7}  {:>9}  {:>9}  {:>16}",
        "script", "docs", "raw", "plain", "central", "per-lang", "per-lang vs central"
    );
    println!("{:-<86}", "");
    let ratio = |raw: u64, comp: u64| raw as f64 / comp.max(1) as f64;
    for &sc in &SCRIPTS {
        let Some(b) = buckets.get(&sc) else { continue };
        if b.docs == 0 {
            continue;
        }
        let r_plain = ratio(b.raw, b.plain);
        let r_cen = ratio(b.raw, b.central);
        let r_lng = ratio(b.raw, b.per_lang);
        let gain = 100.0 * (r_lng - r_cen) / r_cen.max(1e-9);
        let no_lang = lang_dicts.get(&sc).map(Vec::is_empty).unwrap_or(true);
        println!(
            "{:>9}  {:>7}  {:>9}  {:>6.2}×  {:>8.2}×  {:>8.2}×  {:>+15.1}%{}",
            script_name(sc),
            b.docs,
            human_bytes(b.raw),
            r_plain,
            r_cen,
            r_lng,
            gain,
            if no_lang {
                "  (too few docs to train)"
            } else {
                ""
            },
        );
    }

    let lang_total: u64 = lang_dicts.values().map(|d| d.len() as u64).sum();
    let lang_n = lang_dicts.values().filter(|d| !d.is_empty()).count();
    println!(
        "\nDict storage (stored once for the whole archive): central = {} (1 dict), \
         per-language = {} ({} dicts).",
        human_bytes(central.len() as u64),
        human_bytes(lang_total),
        lang_n,
    );
    println!(
        "Read this as: where `central` ≈ `plain`, the one shared dict does ~nothing for that\n\
         script (its substrings aren't in the English-dominated central sample); a positive\n\
         `per-lang vs central` is the ratio a language-matched dict recovers for it."
    );
}

fn print_row(label: &str, m: &Measure, raw: usize) {
    let ratio = raw as f64 / m.comp.max(1) as f64;
    let cold_us = m.decode_ns as f64 / 1000.0 / m.units.max(1) as f64;
    let seq_mibs = mib(raw) / (m.decode_ns as f64 / 1e9).max(1e-9);
    let build_mibs = mib(raw) / (m.build_ns as f64 / 1e9).max(1e-9);
    println!(
        "{:>11}  {:>10}  {:>6.2}×  {:>13.2}  {:>15.0}  {:>15.0}",
        label,
        human_bytes(m.comp as u64),
        ratio,
        cold_us,
        seq_mibs,
        build_mibs,
    );
}

fn parse_env_usize(var: &str) -> Option<Vec<usize>> {
    let v = std::env::var(var).ok()?;
    let parsed: Vec<usize> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (!parsed.is_empty()).then_some(parsed)
}

fn parse_env_i32(var: &str) -> Option<Vec<i32>> {
    let v = std::env::var(var).ok()?;
    let parsed: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (!parsed.is_empty()).then_some(parsed)
}

fn mib(n: usize) -> f64 {
    n as f64 / (1024.0 * 1024.0)
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}
