//! Streaming summary of tracing-chrome's newline-delimited JSON array.
//! Keeps only SDR spans, so full Bevy traces need not fit in memory.
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufRead, BufReader},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: trace_summary TRACE.json")?;
    let reader = BufReader::new(File::open(path)?);
    let mut active: HashMap<(u64, String), Vec<f64>> = HashMap::new();
    let mut durations: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut unmatched = 0;
    for line in reader.lines() {
        let line = line?;
        if !line.contains("\"name\":\"sdr.") {
            continue;
        }
        let event: Value = serde_json::from_str(line.trim().trim_end_matches(','))?;
        let name = event["name"].as_str().ok_or("missing span name")?;
        let thread = event["tid"].as_u64().ok_or("missing thread")?;
        let ts = event["ts"].as_f64().ok_or("missing timestamp")?;
        let key = (thread, name.to_owned());
        match event["ph"].as_str() {
            Some("B") => active.entry(key).or_default().push(ts),
            Some("E") => {
                if let Some(start) = active.get_mut(&key).and_then(Vec::pop) {
                    let group = name.split(':').next().unwrap();
                    durations
                        .entry(group.to_owned())
                        .or_default()
                        .push(ts - start);
                } else {
                    unmatched += 1;
                }
            }
            _ => {}
        }
    }
    let unclosed: usize = active.values().map(Vec::len).sum();
    eprintln!(
        "Unmatched ends: {unmatched}; unclosed spans: {unclosed}. Durations include child spans and waits."
    );
    if durations.is_empty() {
        return Err("No SDR spans found; expected tracing-chrome newline JSON".into());
    }
    println!("span,count,mean_us,p50_us,p95_us,max_us");
    for (name, mut values) in durations {
        values.sort_by(f64::total_cmp);
        let percentile =
            |p: f64| values[((values.len() as f64 * p).ceil() as usize).saturating_sub(1)];
        println!(
            "{name},{},{:.3},{:.3},{:.3},{:.3}",
            values.len(),
            values.iter().sum::<f64>() / values.len() as f64,
            percentile(0.5),
            percentile(0.95),
            values.last().unwrap()
        );
    }
    if unmatched + unclosed > 0 {
        return Err("Incomplete trace: review partial statistics above".into());
    }
    Ok(())
}
