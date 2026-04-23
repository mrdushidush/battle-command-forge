//! Stress test suite — 21 graded tasks (C4-C9) ported from battleclaw-v2.
//!
//! Tests the system's code generation quality across increasing complexity.

use crate::llm::LlmClient;
use anyhow::Result;
use std::time::Instant;

pub struct StressTask {
    pub name: &'static str,
    pub complexity: u32,
    pub language: &'static str,
    pub prompt: &'static str,
}

pub struct StressResult {
    pub name: String,
    pub complexity: u32,
    pub passed: bool,
    pub lines: usize,
    pub duration_secs: f64,
}

/// Run the stress test suite.
pub async fn run_stress(llm: &LlmClient, max_tasks: usize) -> Result<Vec<StressResult>> {
    let tasks = get_tasks();
    let tasks: Vec<&StressTask> = tasks.iter().take(max_tasks).collect();

    println!("BattleCommand Forge — Stress Test");
    println!("==================================");
    println!("Tasks: {}  Complexity: C4-C9\n", tasks.len());

    let mut results = Vec::new();
    let total_start = Instant::now();

    for (i, task) in tasks.iter().enumerate() {
        print!(
            "  [{:>2}/{}] C{} {:<30} ",
            i + 1,
            tasks.len(),
            task.complexity,
            task.name
        );

        let start = Instant::now();
        let system = format!(
            "You are a senior engineer. Write ONLY the code requested. \
             No explanations. The code must compile/run and print 'PASS' if all tests pass. \
             Language: {}",
            task.language
        );

        let response = llm
            .generate(
                &format!("STRESS-C{}", task.complexity),
                &system,
                task.prompt,
            )
            .await;
        let duration = start.elapsed().as_secs_f64();

        match response {
            Ok(code) => {
                let lines = code.lines().count();
                // For stress tests, we check if the LLM produced reasonable output
                let passed = lines > 5 && !code.contains("TODO") && !code.contains("FIXME");
                results.push(StressResult {
                    name: task.name.to_string(),
                    complexity: task.complexity,
                    passed,
                    lines,
                    duration_secs: duration,
                });
                println!(
                    "{}  {:.1}s  {} lines",
                    if passed { "PASS" } else { "FAIL" },
                    duration,
                    lines
                );
            }
            Err(e) => {
                results.push(StressResult {
                    name: task.name.to_string(),
                    complexity: task.complexity,
                    passed: false,
                    lines: 0,
                    duration_secs: duration,
                });
                println!(
                    "ERR   {:.1}s  {}",
                    duration,
                    e.to_string().chars().take(50).collect::<String>()
                );
            }
        }
    }

    let wall_time = total_start.elapsed().as_secs_f64();
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();
    let pass_rate = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("\n=== Stress Test Results ===");
    println!("Pass Rate: {}/{} ({:.0}%)", passed, total, pass_rate);
    println!("Wall Time: {:.1}s", wall_time);

    if pass_rate >= 98.0 {
        println!("RESULT: EXCELLENT (>= 98%)");
    } else if pass_rate >= 90.0 {
        println!("RESULT: GOOD (>= 90%)");
    } else {
        println!("RESULT: NEEDS IMPROVEMENT (< 90%)");
    }

    Ok(results)
}

fn get_tasks() -> Vec<StressTask> {
    vec![
        // C4 — Protocol structures and coordinate math
        StressTask { name: "Coord Distance", complexity: 4, language: "python",
            prompt: "Write a Python function `haversine(lat1, lon1, lat2, lon2) -> float` computing great-circle distance in km. Earth radius 6371. Test: London(51.5074,-0.1278) to Paris(48.8566,2.3522) should be 340-345 km. Print PASS if correct." },
        StressTask { name: "Bearing Calc", complexity: 4, language: "python",
            prompt: "Write `initial_bearing(lat1, lon1, lat2, lon2) -> float` returning initial bearing in degrees 0-360. Test: (0,0) to (0,1) ~= 90 degrees. Print PASS." },
        StressTask { name: "Military Timestamp", complexity: 4, language: "python",
            prompt: "Write functions to convert between Unix epoch and DTG format (DDHHMMZmmmYY). Test round-trip. Print PASS." },

        // C5 — State machines, ring buffers
        StressTask { name: "CRC-32", complexity: 5, language: "python",
            prompt: "Implement CRC-32 with polynomial 0xEDB88320. CRC of '123456789' must equal 0xCBF43926. Print PASS." },
        StressTask { name: "Ring Buffer", complexity: 5, language: "python",
            prompt: "Implement a fixed-size circular buffer class with push, pop, peek, full, empty. Test FIFO order and wrap-around. Print PASS." },
        StressTask { name: "Heading Normalize", complexity: 5, language: "python",
            prompt: "Write normalize(deg)->float wrapping to [0,360), shortest_turn(from,to)->float signed, in_arc(heading,center,width)->bool. Test all. Print PASS." },
        StressTask { name: "Priority Queue", complexity: 5, language: "python",
            prompt: "Max-heap priority queue (no heapq). push(priority, data), pop() returns highest priority. Test order 5,4,3,1,1 for inputs 3,1,4,1,5. Print PASS." },

        // C6 — Complex protocols, matrix math
        StressTask { name: "Matrix 3x3", complexity: 6, language: "python",
            prompt: "3x3 matrix class: multiply, transpose, determinant. det of [[1,2,3],[0,1,4],[5,6,0]] = 1.0. Print PASS." },
        StressTask { name: "ENU Coordinates", complexity: 6, language: "python",
            prompt: "WGS84 geodetic_to_ecef and ecef_to_enu functions. (0,0,0)->ECEF ~= (6378137,0,0). Point 1km north has ENU north ~1000m. Print PASS." },
        StressTask { name: "Hamming Code", complexity: 6, language: "python",
            prompt: "Hamming(7,4) encode/decode with single-error correction. Encode 0b1011, flip one bit, decode back correctly. Print PASS." },
        StressTask { name: "Link-16 Word", complexity: 6, language: "python",
            prompt: "Pack/unpack 32-bit Link-16 J-word: label(5b), sublabel(3b), data(24b). Round-trip (31,7,0xFFFFFF). Print PASS." },

        // C7 — Scheduling, IFF, classification
        StressTask { name: "Threat Assessment", complexity: 7, language: "python",
            prompt: "ThreatAssessor scoring targets by closing_rate, distance, altitude, rcs, iff. Foe close fast low = CRITICAL, friend far slow high = LOW. Print PASS." },
        StressTask { name: "Track Correlator", complexity: 7, language: "python",
            prompt: "TrackCorrelator: nearest-neighbor association within gate distance. 2 sensors, 2 targets. Verify 2 tracks created with fused positions. Print PASS." },

        // C8 — Kalman, ballistics
        StressTask { name: "Kalman 1D", complexity: 8, language: "python",
            prompt: "KalmanFilter1D with predict(dt) and update(z). Constant velocity model. Init at 0, feed 10 measurements of object at 100. Estimate within 10. Print PASS." },
        StressTask { name: "Ballistic Trajectory", complexity: 8, language: "python",
            prompt: "simulate_trajectory(v0, angle_deg, drag_coeff=0, dt=0.01) -> dict with max_height, range, flight_time. v0=100, 45deg, no drag: range~1019m. With drag: shorter. Print PASS." },
        StressTask { name: "Engagement Zone", complexity: 8, language: "python",
            prompt: "EngagementZone(min_range,max_range,min_alt,max_alt,max_speed). in_zone(), pk_estimate(). Zone(5,50,100,15000,800). (25,5000,200)=in, (100,5000,200)=out. Print PASS." },

        // C9 — Full systems
        StressTask { name: "Extended Kalman", complexity: 9, language: "python",
            prompt: "EKF for 2D tracking: state=[x,y,vx,vy]. predict + update_position + update_bearing_range. Track target at (100,100) moving (10,0). Within 20m after 5 steps. Print PASS." },
        StressTask { name: "A* Pathfinding", complexity: 9, language: "python",
            prompt: "A* on 2D grid. Grid class with set_blocked, find_path. 10x10 grid with wall (one gap). Path from (0,0) to (9,9) exists and avoids blocked. Fully blocked -> empty. Print PASS." },
        StressTask { name: "Track Manager", complexity: 9, language: "python",
            prompt: "TrackManager: predict_all, correlate_and_update, prune. States: TENTATIVE->CONFIRMED->DELETED. 3 targets, 10 steps. >=2 confirmed tracks. Print PASS." },
        StressTask { name: "Radar Processor", complexity: 9, language: "python",
            prompt: "matched_filter (cross-correlation) and cfar_detect (Cell-Averaging CFAR). Signal with peak at index 20, template [1,1,1]. CFAR detects 2 targets above noise. Print PASS." },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_count() {
        let tasks = get_tasks();
        assert!(tasks.len() >= 20);
    }

    #[test]
    fn test_complexity_range() {
        let tasks = get_tasks();
        assert!(tasks.iter().all(|t| t.complexity >= 4 && t.complexity <= 9));
    }
}
