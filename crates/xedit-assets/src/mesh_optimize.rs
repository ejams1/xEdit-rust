// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbMeshOptimize.pas
//
// Upstream ports these routines from meshoptimizer 0.21, Copyright (C)
// 2016-2024 Arseny Kapoulkine, under the MIT License (see NOTICE).

//! The index buffer optimizers of meshoptimizer as xEdit ports them: the
//! vertex cache and vertex fetch analyzers, the stripifier, the vertex cache
//! optimizer (Tom Forsyth's algorithm with meshoptimizer's tuned scores),
//! the overdraw optimizer and the vertex fetch remap.
//!
//! The routines take and return index buffers (`TTriIndices`, three indices
//! per triangle). Upstream's mistakes against meshoptimizer are kept and
//! marked; the arithmetic follows Delphi's Win64 rules: `Single` values are
//! worked out in double precision and rounded where they are stored, and an
//! inline variable initialised from a `Single` expression is a `Double`.
//!
//! The users are `TwbNifFile.SpellOptimize` (`data_format_nif::optimize`),
//! `StripifyTriangles` of `wbNifMath`, Sniff's `Optimize mesh` and
//! `Analyze mesh`, and LOD generation.

use crate::nif_math::{Vector3, same_value, vector3_cross};

/// `TVertexCacheStatistics`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VertexCacheStatistics {
    pub warps_executed: u32,
    pub vertices_transformed: u32,
    /// Transformed vertices per triangle; best case 0.5, worst case 3.0.
    pub acmr: f64,
    /// Transformed vertices per vertex; best case 1.0, worst case 6.0.
    pub atvr: f64,
}

/// `TVertexFetchStatistics`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VertexFetchStatistics {
    pub bytes_fetched: u32,
    /// Fetched bytes per vertex buffer byte; best case 1.0.
    pub overfetch: f64,
}

/// `Trunc` of a double with the floating point exceptions masked: a NaN or a
/// value beyond `Int64` gives the integer indefinite.
fn trunc(value: f64) -> i64 {
    if value.is_nan() || value >= 9_223_372_036_854_775_808.0 || value < -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        value as i64
    }
}

/// `GetVertexCount`: the largest index plus one.
fn get_vertex_count(indices: &[u32]) -> u32 {
    let mut result = 0u32;
    for &index in indices {
        if index > result {
            result = index;
        }
    }
    result.wrapping_add(1)
}

/// `meshopt_quantizeUnorm`.
pub fn quantize_unorm(mut v: f64, n: i32) -> i32 {
    let scale = ((1i32 << n) - 1) as f32;
    // UPSTREAM-QUIRK: meshoptimizer clamps to [0, 1]; the port sets every
    // positive value to 1, so a quantized value is 0 or the maximum.
    if v < 0.0 {
        v = 0.0;
    } else if v > 0.0 {
        v = 1.0;
    }
    trunc(v * f64::from(scale) + 0.5) as i32
}

/// `meshopt_quantizeSnorm`.
pub fn quantize_snorm(mut v: f64, n: i32) -> i32 {
    let scale = ((1i32 << (n - 1)) - 1) as f32;
    let r: f32 = if v >= 0.0 { 0.5 } else { -0.5 };
    v = v.clamp(-1.0, 1.0);
    trunc(v * f64::from(scale) + f64::from(r)) as i32
}

/// `meshopt_analyzeVertexCache`: cache hit statistics of a simplified FIFO
/// model.
pub fn analyze_vertex_cache(
    indices: &[u32],
    cache_size: u32,
    warp_size: u32,
    primgroup_size: u32,
) -> VertexCacheStatistics {
    let mut result = VertexCacheStatistics::default();
    let index_count = indices.len();
    let vertex_count = get_vertex_count(indices);
    let mut warp_offset = 0u32;
    let mut primgroup_offset = 0u32;
    let mut cache_timestamps = vec![0u32; vertex_count as usize];
    let mut timestamp = cache_size.wrapping_add(1);
    let mut i = 0usize;
    while i + 2 < index_count {
        let a = indices[i] as usize;
        let b = indices[i + 1] as usize;
        let c = indices[i + 2] as usize;
        let ac = timestamp.wrapping_sub(cache_timestamps[a]) > cache_size;
        let bc = timestamp.wrapping_sub(cache_timestamps[b]) > cache_size;
        let cc = timestamp.wrapping_sub(cache_timestamps[c]) > cache_size;
        // flush cache if triangle doesn't fit into warp or into the primitive buffer
        if (primgroup_size != 0 && primgroup_offset == primgroup_size)
            || (warp_size != 0 && warp_offset + u32::from(ac) + u32::from(bc) + u32::from(cc) > warp_size)
        {
            if warp_offset > 0 {
                result.warps_executed += 1;
            }
            warp_offset = 0;
            primgroup_offset = 0;
            // reset cache
            timestamp = timestamp.wrapping_add(cache_size + 1);
        }
        // update cache and add vertices to warp
        for j in 0..3 {
            let index = indices[i + j] as usize;
            if timestamp.wrapping_sub(cache_timestamps[index]) > cache_size {
                cache_timestamps[index] = timestamp;
                timestamp = timestamp.wrapping_add(1);
                result.vertices_transformed += 1;
                warp_offset += 1;
            }
        }
        primgroup_offset += 1;
        i += 3;
    }
    let unique_vertex_count = cache_timestamps.iter().filter(|&&stamp| stamp > 0).count();
    if warp_offset > 0 {
        result.warps_executed += 1;
    }
    result.acmr = if index_count != 0 {
        f64::from(result.vertices_transformed) / (index_count as f64 / 3.0)
    } else {
        0.0
    };
    result.atvr = if unique_vertex_count != 0 {
        f64::from(result.vertices_transformed) / unique_vertex_count as f64
    } else {
        0.0
    };
    result
}

/// `meshopt_analyzeVertexFetch`: cache hit statistics of a simplified direct
/// mapped model.
pub fn analyze_vertex_fetch(indices: &[u32], vertex_size: u32) -> VertexFetchStatistics {
    const CACHE_LINE: u32 = 64;
    const CACHE_SIZE: u32 = 128 * 1024;
    let mut result = VertexFetchStatistics::default();
    let vertex_count = get_vertex_count(indices);
    let mut vertex_visited = vec![0u8; vertex_count as usize];
    // simple direct mapped cache
    let mut cache = vec![0u32; (CACHE_SIZE / CACHE_LINE) as usize];
    let lines = cache.len() as u32;
    for &index in indices {
        vertex_visited[index as usize] = 1;
        let start_address = index.wrapping_mul(vertex_size);
        let end_address = start_address.wrapping_add(vertex_size);
        let start_tag = start_address / CACHE_LINE;
        let end_tag = end_address.wrapping_add(CACHE_LINE - 1) / CACHE_LINE;
        let mut tag = start_tag;
        while tag < end_tag {
            let line = (tag % lines) as usize;
            // we store +1 since cache is filled with 0 by default
            if cache[line] != tag.wrapping_add(1) {
                result.bytes_fetched = result.bytes_fetched.wrapping_add(CACHE_LINE);
            }
            cache[line] = tag.wrapping_add(1);
            tag += 1;
        }
    }
    let unique_vertex_count: u32 = vertex_visited.iter().map(|&v| u32::from(v)).sum();
    result.overfetch = if unique_vertex_count != 0 {
        f64::from(result.bytes_fetched) / f64::from(unique_vertex_count.wrapping_mul(vertex_size))
    } else {
        0.0
    };
    result
}

/// `meshopt_stripify`: a triangle strip of a (vertex cache optimized)
/// triangle list, strips stitched with degenerate triangles, or with
/// `restart_index` where that is not 0.
pub fn stripify(indices: &[u32], restart_index: u32) -> Vec<u32> {
    const BUFFER_CAPACITY: usize = 8;
    if indices.is_empty() {
        return Vec::new();
    }
    let len = indices.len() as u32;
    // strip length worst case without restarts is 2 degenerate indices and 3 indices per triangle
    // `Add` appends to the result, whose length is `strip_size`.
    let mut result: Vec<u32> = Vec::with_capacity((indices.len() / 3) * 5);
    let add = |result: &mut Vec<u32>, index: u32| result.push(index);
    let mut buffer = [[0u32; 3]; BUFFER_CAPACITY];
    let mut buffer_size = 0usize;
    let mut index_offset = 0u32;
    let mut parity = 0u32;
    let mut strip = [0u32; 3];

    // compute vertex valence; this is used to prioritize starting triangle for strips
    let index_max = indices.iter().copied().max().unwrap_or(0);
    let mut valence = vec![0u8; index_max as usize + 1];
    for &index in indices {
        valence[index as usize] = valence[index as usize].wrapping_add(1);
    }

    let find_strip_first = |buffer: &[[u32; 3]], buffer_size: usize, valence: &[u8]| -> usize {
        let mut result = 0usize;
        let mut iv = u32::MAX;
        for (i, triangle) in buffer.iter().enumerate().take(buffer_size) {
            let va = valence[triangle[0] as usize];
            let vb = valence[triangle[1] as usize];
            let vc = valence[triangle[2] as usize];
            let v = if va < vb && va < vc {
                va
            } else if vb < vc {
                vb
            } else {
                vc
            };
            if u32::from(v) < iv {
                result = i;
                iv = u32::from(v);
            }
        }
        result
    };
    let find_strip_next = |buffer: &[[u32; 3]], buffer_size: usize, e0: u32, e1: u32| -> i32 {
        for (i, &[a, b, c]) in buffer.iter().enumerate().take(buffer_size) {
            let i = i as i32;
            if e0 == a && e1 == b {
                return (i << 2) | 2;
            } else if e0 == b && e1 == c {
                return i << 2;
            } else if e0 == c && e1 == a {
                return (i << 2) | 1;
            }
        }
        -1
    };

    let mut next: i32 = -1;
    while buffer_size > 0 || index_offset < len {
        // fill triangle buffer
        while buffer_size < BUFFER_CAPACITY && index_offset < len {
            let at = index_offset as usize;
            buffer[buffer_size] = [indices[at], indices[at + 1], indices[at + 2]];
            buffer_size += 1;
            index_offset += 3;
        }
        if next >= 0 {
            let i = (next >> 2) as usize;
            let [a, b, c] = buffer[i];
            let v = buffer[i][(next & 3) as usize];
            // ordered removal from the buffer
            buffer.copy_within(i + 1..buffer_size, i);
            buffer_size -= 1;
            // update vertex valences for strip start heuristic
            for index in [a, b, c] {
                valence[index as usize] = valence[index as usize].wrapping_sub(1);
            }
            // find next triangle (note that edge order flips on every iteration)
            let cont = find_strip_next(
                &buffer,
                buffer_size,
                if parity != 0 { strip[1] } else { v },
                if parity != 0 { v } else { strip[1] },
            );
            let mut swap = -1;
            if cont < 0 {
                // UPSTREAM-QUIRK: meshoptimizer passes `parity ? strip[0] : v`
                // as the second edge vertex; the port passes the parity itself.
                swap = find_strip_next(
                    &buffer,
                    buffer_size,
                    if parity != 0 { v } else { strip[0] },
                    if parity != 0 { parity } else { strip[0] },
                );
            }
            if cont < 0 && swap >= 0 {
                // [a b c] => [a b a c]
                add(&mut result, strip[0]);
                add(&mut result, v);
                // next strip has same winding
                strip[1] = v;
                next = swap;
            } else {
                // emit the next vertex in the strip
                add(&mut result, v);
                // next strip has flipped winding
                strip[0] = strip[1];
                strip[1] = v;
                parity ^= 1;
                next = cont;
            }
        } else {
            // find the next new triangle with a heuristic to maximize the strip length
            let i = find_strip_first(&buffer, buffer_size, &valence);
            let [mut a, mut b, mut c] = buffer[i];
            // ordered removal from the buffer
            buffer.copy_within(i + 1..buffer_size, i);
            buffer_size -= 1;
            for index in [a, b, c] {
                valence[index as usize] = valence[index as usize].wrapping_sub(1);
            }
            // pre-rotate the triangle so that we will find a match in the existing buffer on the next iteration
            let ea = find_strip_next(&buffer, buffer_size, c, b);
            let eb = find_strip_next(&buffer, buffer_size, a, c);
            let ec = find_strip_next(&buffer, buffer_size, b, a);
            // pick the matching edge with the smallest triangle index in the buffer
            let mut mine = i32::MAX;
            if ea >= 0 && mine > ea {
                mine = ea;
            }
            if eb >= 0 && mine > eb {
                mine = eb;
            }
            if ec >= 0 && mine > ec {
                mine = ec;
            }
            if ea == mine {
                // keep abc
                next = ea;
            } else if eb == mine {
                // abc -> bca
                let t = a;
                a = b;
                b = c;
                c = t;
                next = eb;
            } else if ec == mine {
                // abc -> cab
                let t = c;
                c = b;
                b = a;
                a = t;
                next = ec;
            }
            if restart_index != 0 {
                if !result.is_empty() {
                    add(&mut result, restart_index);
                }
                add(&mut result, a);
                add(&mut result, b);
                add(&mut result, c);
                // new strip always starts with the same edge winding
                strip[0] = b;
                strip[1] = c;
                parity = 1;
            } else {
                if !result.is_empty() {
                    // connect last strip using degenerate triangles
                    add(&mut result, strip[1]);
                    add(&mut result, a);
                }
                // we always end up with outgoing edge "cb" in the end
                let e0 = if parity != 0 { c } else { b };
                let e1 = if parity != 0 { b } else { c };
                add(&mut result, a);
                add(&mut result, e0);
                add(&mut result, e1);
                strip[0] = e0;
                strip[1] = e1;
                parity ^= 1;
            }
        }
    }
    result
}

/// `meshopt_optimizeVertexCache`: the triangles reordered to reduce the
/// vertex shader invocations; `strip` tunes the scores for a list that is
/// stripified next.
pub fn optimize_vertex_cache(indices: &[u32], strip: bool) -> Vec<u32> {
    const CACHE_SIZE_MAX: usize = 16;
    const VALENCE_MAX: u32 = 8;
    const VERTEX_SCORE_CACHE: [f32; CACHE_SIZE_MAX + 1] = [
        0.0, 0.779, 0.791, 0.789, 0.981, 0.843, 0.726, 0.847, 0.882, 0.867, 0.799, 0.642, 0.613, 0.600, 0.568, 0.372,
        0.234,
    ];
    const VERTEX_SCORE_LIVE: [f32; VALENCE_MAX as usize + 1] =
        [0.0, 0.995, 0.713, 0.450, 0.404, 0.059, 0.005, 0.147, 0.006];
    const VERTEX_SCORE_CACHE_STRIP: [f32; CACHE_SIZE_MAX + 1] = [
        0.0, 1.0, 1.0, 1.0, 0.453, 0.561, 0.490, 0.459, 0.179, 0.526, 0.0, 0.227, 0.184, 0.490, 0.112, 0.050, 0.131,
    ];
    const VERTEX_SCORE_LIVE_STRIP: [f32; VALENCE_MAX as usize + 1] =
        [0.0, 0.956, 0.786, 0.577, 0.558, 0.618, 0.549, 0.499, 0.489];

    let index_count = indices.len();
    if index_count == 0 {
        return Vec::new();
    }
    let vertex_count = get_vertex_count(indices) as usize;
    let mut result = vec![0u32; index_count];
    let face_count = index_count / 3;
    let cache_size = 16usize;
    let (score_cache, score_live) = if strip {
        (&VERTEX_SCORE_CACHE_STRIP, &VERTEX_SCORE_LIVE_STRIP)
    } else {
        (&VERTEX_SCORE_CACHE, &VERTEX_SCORE_LIVE)
    };
    // `vertexScore`: a `Single` sum.
    let vertex_score = |cache_position: i32, live_triangles: u32| -> f32 {
        let clamped = live_triangles.min(VALENCE_MAX) as usize;
        (f64::from(score_cache[(1 + cache_position) as usize]) + f64::from(score_live[clamped])) as f32
    };

    // `buildTriangleAdjacency`
    let mut counts = vec![0u32; vertex_count];
    let mut offsets = vec![0u32; vertex_count];
    let mut data = vec![0u32; index_count];
    for &i in indices {
        counts[i as usize] += 1;
    }
    let mut offset = 0u32;
    for i in 0..vertex_count {
        offsets[i] = offset;
        offset += counts[i];
    }
    for i in 0..face_count {
        for k in 0..3 {
            let v = indices[i * 3 + k] as usize;
            data[offsets[v] as usize] = i as u32;
            offsets[v] += 1;
        }
    }
    for i in 0..vertex_count {
        offsets[i] -= counts[i];
    }

    // live triangle counts are the adjacency counts, which drop as triangles are emitted
    let mut emitted_flags = vec![false; face_count];
    let mut vertex_scores: Vec<f32> = (0..vertex_count).map(|i| vertex_score(-1, counts[i])).collect();
    let mut triangle_scores: Vec<f32> = (0..face_count)
        .map(|i| {
            let [a, b, c] = [indices[i * 3], indices[i * 3 + 1], indices[i * 3 + 2]];
            (f64::from(vertex_scores[a as usize])
                + f64::from(vertex_scores[b as usize])
                + f64::from(vertex_scores[c as usize])) as f32
        })
        .collect();

    let mut cache_holder = [[0u32; CACHE_SIZE_MAX + 4]; 2];
    let mut cache_index = 0usize;
    let mut cache_count = 0usize;
    let mut current_triangle = 0u32;
    let mut input_cursor = 1u32;
    let mut output_triangle = 0usize;

    while current_triangle != u32::MAX {
        let ct = current_triangle as usize;
        let a = indices[ct * 3];
        let b = indices[ct * 3 + 1];
        let c = indices[ct * 3 + 2];
        // output indices
        result[output_triangle * 3] = a;
        result[output_triangle * 3 + 1] = b;
        result[output_triangle * 3 + 2] = c;
        output_triangle += 1;
        // update emitted flags
        emitted_flags[ct] = true;
        triangle_scores[ct] = 0.0;
        // new triangle, then the old ones
        let (old, new) = if cache_index == 0 {
            let (first, second) = cache_holder.split_at_mut(1);
            (&first[0], &mut second[0])
        } else {
            let (first, second) = cache_holder.split_at_mut(1);
            (&second[0], &mut first[0])
        };
        let mut cache_write = 0usize;
        for index in [a, b, c] {
            new[cache_write] = index;
            cache_write += 1;
        }
        for &index in old.iter().take(cache_count) {
            new[cache_write] = index;
            if index != a && index != b && index != c {
                cache_write += 1;
            }
        }
        cache_index ^= 1;
        let cache = cache_holder[cache_index];
        cache_count = cache_write.min(cache_size);

        // remove emitted triangle from adjacency data
        for k in 0..3 {
            let index = indices[ct * 3 + k] as usize;
            let start = offsets[index] as usize;
            let size = counts[index] as usize;
            for i in 0..size {
                if data[start + i] == current_triangle {
                    data[start + i] = data[start + size - 1];
                    counts[index] -= 1;
                    break;
                }
            }
        }

        let mut best_triangle = u32::MAX;
        let mut best_score: f32 = 0.0;
        // update cache positions, vertex scores and triangle scores, and find next best triangle
        for (i, &index) in cache.iter().enumerate().take(cache_write) {
            let index = index as usize;
            if counts[index] == 0 {
                continue;
            }
            let cache_position = if i >= cache_size { -1 } else { i as i32 };
            // `var score := ...` and `var score_diff := ...` are doubles.
            let score = f64::from(vertex_score(cache_position, counts[index]));
            let score_diff = score - f64::from(vertex_scores[index]);
            vertex_scores[index] = score as f32;
            let start = offsets[index] as usize;
            for &tri in &data[start..start + counts[index] as usize] {
                let tri_score = f64::from(triangle_scores[tri as usize]) + score_diff;
                if f64::from(best_score) < tri_score {
                    best_triangle = tri;
                    best_score = tri_score as f32;
                }
                triangle_scores[tri as usize] = tri_score as f32;
            }
        }
        // step through input triangles in order if we hit a dead-end
        current_triangle = best_triangle;
        if current_triangle == u32::MAX {
            // `getNextTriangleDeadEnd`
            while (input_cursor as usize) < face_count {
                if !emitted_flags[input_cursor as usize] {
                    current_triangle = input_cursor;
                    break;
                }
                input_cursor += 1;
            }
        }
    }
    result
}

/// `meshopt_optimizeOverdraw`: the clusters of a vertex cache optimized list
/// reordered to reduce the pixel overdraw, by at most `threshold` worse
/// vertex cache efficiency.
pub fn optimize_overdraw(indices: &[u32], vertices: &[Vector3], threshold: f32) -> Vec<u32> {
    let index_count = indices.len();
    let vertex_count = vertices.len();
    if index_count == 0 || vertex_count == 0 {
        return Vec::new();
    }
    let face_count = index_count / 3;
    let cache_size = 16u32;
    let mut cache_timestamps = vec![0u32; vertex_count];

    // `updateCache`
    let update_cache = |a: u32, b: u32, c: u32, cache_timestamps: &mut [u32], timestamp: &mut u32| -> u32 {
        let mut result = 0;
        // UPSTREAM-QUIRK: meshoptimizer stamps each vertex that misses; the
        // port stamps the first one for all three.
        for v in [a, b, c] {
            if timestamp.wrapping_sub(cache_timestamps[v as usize]) > cache_size {
                cache_timestamps[a as usize] = *timestamp;
                *timestamp = timestamp.wrapping_add(1);
                result += 1;
            }
        }
        result
    };

    // generate hard boundaries from full-triangle cache misses
    let mut hard_clusters = vec![0u32; face_count];
    let hard_cluster_count = {
        cache_timestamps.fill(0);
        let mut timestamp = cache_size + 1;
        let mut count = 0usize;
        for i in 0..face_count {
            let m = update_cache(
                indices[i * 3],
                indices[i * 3 + 1],
                indices[i * 3 + 2],
                &mut cache_timestamps,
                &mut timestamp,
            );
            if i == 0 || m == 3 {
                hard_clusters[count] = i as u32;
                count += 1;
            }
        }
        count
    };

    // generate soft boundaries
    let mut clusters = vec![0u32; face_count + 1];
    let cluster_count = {
        cache_timestamps.fill(0);
        let mut timestamp = 0u32;
        let mut count = 0usize;
        for it in 0..hard_cluster_count {
            let start = hard_clusters[it] as usize;
            let end = if it + 1 < hard_cluster_count {
                hard_clusters[it + 1] as usize
            } else {
                face_count
            };
            // reset cache
            timestamp = timestamp.wrapping_add(cache_size + 1);
            // measure cluster ACMR
            let mut cluster_misses = 0u32;
            for i in start..end {
                cluster_misses += update_cache(
                    indices[i * 3],
                    indices[i * 3 + 1],
                    indices[i * 3 + 2],
                    &mut cache_timestamps,
                    &mut timestamp,
                );
            }
            // `var cluster_threshold` is a double.
            let cluster_threshold =
                f64::from(threshold) * (f64::from(cluster_misses as f32) / f64::from((end - start) as u32 as f32));
            // first cluster always starts from the hard cluster boundary
            clusters[count] = start as u32;
            count += 1;
            // reset cache
            timestamp = timestamp.wrapping_add(cache_size + 1);
            let mut running_misses = 0u32;
            let mut running_faces = 0u32;
            for i in start..end {
                running_misses += update_cache(
                    indices[i * 3],
                    indices[i * 3 + 1],
                    indices[i * 3 + 2],
                    &mut cache_timestamps,
                    &mut timestamp,
                );
                running_faces += 1;
                if f64::from(running_misses as f32) / f64::from(running_faces as f32) <= cluster_threshold {
                    // start a new cluster on the next triangle
                    clusters[count] = (i + 1) as u32;
                    count += 1;
                    // reset cache
                    timestamp = timestamp.wrapping_add(cache_size + 1);
                    running_misses = 0;
                    running_faces = 0;
                }
            }
            // merge the last complete cluster with the last incomplete one
            if clusters[count - 1] as usize != start {
                count -= 1;
            }
        }
        count
    };

    // `calculateSortData`
    let mut sort_data = vec![0f64; cluster_count];
    {
        // UPSTREAM-QUIRK: `mesh_centroid`, `cluster_centroid` and
        // `cluster_normal` are inline record variables that are never
        // initialised; the two in the loop keep their values from one
        // cluster to the next. The port starts them at zero.
        let mut mesh_centroid = Vector3::default();
        for &i in indices {
            mesh_centroid = mesh_centroid + vertices[i as usize];
        }
        mesh_centroid = mesh_centroid / index_count as f64;
        let mut cluster_centroid = Vector3::default();
        let mut cluster_normal = Vector3::default();
        for cluster in 0..cluster_count {
            let cluster_begin = clusters[cluster] as usize * 3;
            let cluster_end = if cluster + 1 < cluster_count {
                clusters[cluster + 1] as usize * 3
            } else {
                index_count
            };
            let mut cluster_area = 0f64;
            let mut i = cluster_begin;
            while i < cluster_end {
                let p0 = vertices[indices[i] as usize];
                let p1 = vertices[indices[i + 1] as usize];
                let p2 = vertices[indices[i + 2] as usize];
                let normal = vector3_cross(p1 - p0, p2 - p0);
                let area = normal.length();
                cluster_centroid.v[0] += (p0.x() + p1.x() + p2.x()) * (area / 3.0);
                cluster_centroid.v[1] += (p0.y() + p1.y() + p2.y()) * (area / 3.0);
                cluster_centroid.v[2] += (p0.z() + p1.z() + p2.z()) * (area / 3.0);
                cluster_normal = cluster_normal + normal;
                cluster_area += area;
                i += 3;
            }
            let inv_cluster_area = if same_value(cluster_area, 0.0) {
                0.0
            } else {
                1.0 / cluster_area
            };
            cluster_centroid = cluster_centroid * inv_cluster_area;
            let cluster_normal_length = cluster_normal.length();
            let inv_cluster_normal_length = if same_value(cluster_normal_length, 0.0) {
                0.0
            } else {
                1.0 / cluster_normal_length
            };
            cluster_normal = cluster_normal * inv_cluster_normal_length;
            let centroid_vector = cluster_centroid - mesh_centroid;
            sort_data[cluster] = centroid_vector.x() * cluster_normal.x()
                + centroid_vector.y() * cluster_normal.y()
                + centroid_vector.z() * cluster_normal.z();
        }
    }

    // `calculateSortOrderRadix`
    let mut sort_order = vec![0u32; cluster_count];
    {
        const SORT_BITS: i32 = 11;
        let mut sort_data_max = 1e-3f64;
        for &data in &sort_data {
            let dpa = data.abs();
            if sort_data_max < dpa {
                sort_data_max = dpa;
            }
        }
        let sort_keys: Vec<u16> = sort_data
            .iter()
            .map(|&data| {
                // flip the distribution since high dot product should come first
                let sort_key = 0.5 - 0.5 * (data / sort_data_max);
                (quantize_unorm(sort_key, SORT_BITS) & ((1 << SORT_BITS) - 1)) as u16
            })
            .collect();
        let mut histogram = vec![0u32; 1 << SORT_BITS];
        for &key in &sort_keys {
            histogram[key as usize] += 1;
        }
        let mut histogram_sum = 0u32;
        for slot in histogram.iter_mut() {
            let count = *slot;
            *slot = histogram_sum;
            histogram_sum += count;
        }
        for (i, &key) in sort_keys.iter().enumerate() {
            sort_order[histogram[key as usize] as usize] = i as u32;
            histogram[key as usize] += 1;
        }
    }

    // fill output buffer
    let mut result = Vec::with_capacity(index_count);
    for &cluster in &sort_order {
        let cluster = cluster as usize;
        let cluster_begin = clusters[cluster] as usize * 3;
        let cluster_end = if cluster + 1 < cluster_count {
            clusters[cluster + 1] as usize * 3
        } else {
            index_count
        };
        result.extend_from_slice(&indices[cluster_begin..cluster_end]);
    }
    result
}

/// `meshopt_optimizeVertexFetchRemap`: the new place of each vertex in the
/// order of first use; an unused vertex maps to `u32::MAX`.
pub fn optimize_vertex_fetch_remap(indices: &[u32]) -> Vec<u32> {
    if indices.is_empty() {
        return Vec::new();
    }
    let vertex_count = get_vertex_count(indices) as usize;
    let mut result = vec![u32::MAX; vertex_count];
    let mut next_vertex = 0u32;
    for &index in indices {
        if result[index as usize] == u32::MAX {
            result[index as usize] = next_vertex;
            next_vertex += 1;
        }
    }
    result
}

/// `meshopt_remapIndices`.
pub fn remap_indices(indices: &[u32], remap: &[u32]) -> Vec<u32> {
    indices.iter().map(|&index| remap[index as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grid of `n` by `n` quads, two triangles each, in row order.
    fn grid(n: u32) -> (Vec<u32>, Vec<Vector3>) {
        let mut indices = Vec::new();
        let mut vertices = Vec::new();
        for y in 0..=n {
            for x in 0..=n {
                vertices.push(Vector3::new(f64::from(x), f64::from(y), f64::from((x * y) % 3)));
            }
        }
        for y in 0..n {
            for x in 0..n {
                let a = y * (n + 1) + x;
                let b = a + 1;
                let c = a + n + 1;
                let d = c + 1;
                indices.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
        (indices, vertices)
    }

    fn sorted_triangles(indices: &[u32]) -> Vec<[u32; 3]> {
        let mut tris: Vec<[u32; 3]> = indices
            .chunks(3)
            .map(|t| {
                // the same triangle with its winding, rotated to its smallest index
                let r = (0..3).min_by_key(|&i| t[i]).unwrap();
                [t[r], t[(r + 1) % 3], t[(r + 2) % 3]]
            })
            .collect();
        tris.sort();
        tris
    }

    #[test]
    fn quantize_keeps_upstreams_clamp() {
        assert_eq!(quantize_unorm(0.0, 11), 0);
        assert_eq!(quantize_unorm(0.25, 11), 2047);
        assert_eq!(quantize_unorm(-0.5, 11), 0);
        assert_eq!(quantize_unorm(f64::NAN, 11) & 2047, 0);
        assert_eq!(quantize_snorm(0.5, 8), 64);
        assert_eq!(quantize_snorm(-2.0, 8), -127);
    }

    #[test]
    fn vertex_cache_keeps_every_triangle() {
        let (indices, _) = grid(12);
        for strip in [false, true] {
            let optimized = optimize_vertex_cache(&indices, strip);
            assert_eq!(sorted_triangles(&optimized), sorted_triangles(&indices));
            let before = analyze_vertex_cache(&indices, 16, 0, 0);
            let after = analyze_vertex_cache(&optimized, 16, 0, 0);
            assert!(after.acmr <= before.acmr, "{after:?} {before:?}");
        }
    }

    #[test]
    fn overdraw_keeps_every_triangle() {
        let (indices, vertices) = grid(9);
        let cached = optimize_vertex_cache(&indices, false);
        let optimized = optimize_overdraw(&cached, &vertices, 1.05);
        assert_eq!(optimized.len(), cached.len());
        assert_eq!(sorted_triangles(&optimized), sorted_triangles(&indices));
    }

    #[test]
    fn strip_triangulates_back() {
        let (indices, _) = grid(7);
        let cached = optimize_vertex_cache(&indices, true);
        let strip = stripify(&cached, 0);
        let tris = crate::nif_math::triangulate_strip(&strip);
        let flat: Vec<u32> = tris.iter().flatten().copied().collect();
        assert_eq!(sorted_triangles(&flat).len(), sorted_triangles(&indices).len());
    }

    #[test]
    fn fetch_remap_numbers_vertices_by_first_use() {
        let indices = [4, 2, 0, 2, 4, 5];
        let remap = optimize_vertex_fetch_remap(&indices);
        assert_eq!(remap, vec![2, u32::MAX, 1, u32::MAX, 0, 3]);
        assert_eq!(remap_indices(&indices, &remap), vec![0, 1, 2, 1, 0, 3]);
        let stats = analyze_vertex_fetch(&indices, 12);
        assert_eq!(stats.bytes_fetched, 128);
    }
}
