//! lay: the layout loop. Solves rooms from saved houses in milliseconds, reading the rules at every run.
//!   lay [--rules FILE] [--room SUBSTR] [--seed N] [--why ITEM] [--watch] [-o OUT.json] HOUSE.house.json...
//! Prints one line per room (time, what is placed, what is not, and why); OUT.json is what tools/layview.py draws. --watch solves again whenever
//! the rule file changes.
use ig_layout::{house, rules, solve};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (mut rules_p, mut room, mut seed, mut why, mut out, mut watch, mut houses) = ("rules/layout.json".to_string(), None, 0u64, None, None, false, vec![]);
    let mut i = 0;
    while i < a.len() {
        let flag = a[i].clone();
        let mut val = || { i += 1; a[i].clone() };
        match flag.as_str() {
            "--rules" => rules_p = val(),
            "--room" => room = Some(val()),
            "--seed" => seed = val().parse().unwrap(),
            "--why" => why = Some(val()),
            "-o" => out = Some(val()),
            "--watch" => watch = true,
            h => houses.push(h.to_string()),
        }
        i += 1;
    }
    ig_assets::pieces::install_pieces();
    let mtime = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut last = None;
    loop {
        if !watch || mtime(&rules_p) != last {
            last = mtime(&rules_p);
            run(&rules_p, room.as_deref(), seed, why.as_deref(), out.as_deref(), &houses);
        }
        if !watch { break }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn run(rules_p: &str, room: Option<&str>, seed: u64, why: Option<&str>, out: Option<&str>, houses: &[String]) {
    let t0 = std::time::Instant::now();
    let r = rules::load(rules_p);
    let mut sols = vec![];
    let (mut rooms_n, mut miss_must, mut incomplete, mut ms_max) = (0, 0, 0, 0.0f64);
    for h in houses {
        let hname = std::path::Path::new(h).file_stem().and_then(|s| s.to_str()).unwrap_or("?").trim_end_matches(".house").to_string();
        for rm in house::load(h) {
            if room.map_or(false, |s| !rm.name.contains(s)) { continue }
            let Some(rr) = r.get(&rm.kind) else { continue };
            let hs = hname.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
            let s = solve::solve(&rm, rr, seed ^ hs, why);
            if std::env::var_os("LAYVOL").is_some() { for r in &s.report { if !r.placed { eprintln!("{hname} {} {} unplaced {:?}", rm.name, r.id, r.why); } } }
            rooms_n += 1;
            ms_max = ms_max.max(s.ms);
            // (a `may` item left out is not a fault: it only says what stopped it)
            let miss: Vec<String> = s.report.iter().filter(|x| !x.placed && x.tier < 2).map(|x| { let mut w: Vec<_> = x.why.iter().collect(); w.sort_by_key(|y| std::cmp::Reverse(*y.1)); format!("{}[t{}: {}]", x.id, x.tier, w.iter().take(3).map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")) }).collect();
            if s.report.iter().any(|x| !x.placed && x.tier == 0) { miss_must += 1; }
            if !miss.is_empty() { incomplete += 1; }
            let left: Vec<&str> = s.report.iter().filter(|x| !x.placed && x.tier >= 2).map(|x| x.id.as_str()).collect();
            let placed: Vec<String> = s.report.iter().filter(|x| x.placed).map(|x| if x.variant == "as_is" { x.id.clone() } else { format!("{}({})", x.id, x.variant) }).collect();
            println!("{hname} {:<18} {:5.1} m2 {:6.1} ms {} | {}{}{}", rm.name, rm.area, s.ms, placed.join(" "), if miss.is_empty() { String::new() } else { format!("| MISSING {}", miss.join(" ")) }, if s.doors_joined { "" } else { " | DOORS CUT" }, if left.is_empty() { String::new() } else { format!(" (without {})", left.join(" ")) });
            sols.push(serde_json::json!({ "house": hname, "sol": s }));
        }
    }
    println!("{rooms_n} rooms, {} complete, {miss_must} missing a must, slowest {ms_max:.1} ms, total {:.0} ms", rooms_n - incomplete, t0.elapsed().as_secs_f64() * 1000.0);
    if let Some(o) = out { std::fs::write(o, serde_json::to_vec(&sols).unwrap()).unwrap(); }
}
