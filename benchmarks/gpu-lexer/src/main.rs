mod engine;
mod gpu;
mod metal;
mod tokenizer;
use std::{hint::black_box, time::Instant};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};
fn syntect_runs(ss: &SyntaxSet, ts: &ThemeSet, lang: &str, code: &str) -> Vec<(usize, usize, u32)> {
    let Some(syntax) = ss.find_syntax_by_token(lang) else {
        return Vec::new();
    };
    let mut h = HighlightLines::new(syntax, &ts.themes["InspiredGitHub"]);
    let mut out: Vec<(usize, usize, u32)> = Vec::new();
    let mut offset = 0;
    for line in LinesWithEndings::from(code) {
        for (style, s) in h.highlight_line(line, ss).unwrap() {
            let c = style.foreground;
            let color = (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32;
            let end = offset + s.len();
            if let Some(last) = out.last_mut()
                && last.2 == color
            {
                last.1 = end;
                offset = end;
                continue;
            }
            out.push((offset, end, color));
            offset = end;
        }
    }
    out
}
fn stats(v: &mut [f64]) -> serde_json::Value {
    if v.is_empty() {
        return serde_json::Value::Null;
    }
    v.sort_by(f64::total_cmp);
    serde_json::json!({"median_ms":v[v.len()/2],"p95_ms":v[((v.len() as f64*0.95).ceil() as usize-1).min(v.len()-1)],"min_ms":v[0]})
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("tokens") {
        let code = std::fs::read_to_string(&args[2]).unwrap();
        let t = tokenizer::tokenize(&code);
        println!(
            "{}",
            serde_json::json!({"features":t.features,"ranges":t.utf16_ranges})
        );
        return;
    }
    let start = Instant::now();
    let mut gpu = engine::Engine::new(args.iter().any(|s| s == "--f32"));
    let gpu_init = start.elapsed().as_secs_f64() * 1000.;
    eprintln!(
        "GPU ready ({gpu_init:.1} ms): {} f16={}",
        gpu.adapter(),
        gpu.f16()
    );
    if args.get(1).map(String::as_str) == Some("labels-batch") {
        let cases: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&args[2]).unwrap()).unwrap();
        for case in cases.as_array().unwrap() {
            let tokens = tokenizer::tokenize(case["code"].as_str().unwrap());
            let labels = gpu.labels(&tokens.features);
            println!("{}", serde_json::json!({"id":case["id"], "labels":labels}));
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("labels") {
        let code = std::fs::read_to_string(&args[2]).unwrap();
        let tokens = tokenizer::tokenize(&code);
        println!("{}", serde_json::json!(gpu.labels(&tokens.features)));
        return;
    }
    if args.get(1).map(String::as_str) == Some("profile") {
        gpu.set_profile(true);
        let code = std::fs::read_to_string(&args[2]).unwrap();
        for _ in 0..30 {
            black_box(gpu.highlight(&code));
        }
        return;
    }
    let start = Instant::now();
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syn_init = start.elapsed().as_secs_f64() * 1000.;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(
            std::env::var("LEXER_MANIFEST").unwrap_or_else(|_| "fixtures/manifest.json".into()),
        ))
        .unwrap(),
    )
    .unwrap();
    let reps = std::env::var("LEXER_REPS")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(30);
    assert!(reps > 0);
    let measure_syntect = std::env::var_os("LEXER_GPU_ONLY").is_none();
    let mut results = Vec::new();
    for fixture in manifest.as_array().unwrap() {
        let path = fixture["file"].as_str().unwrap();
        let lang = fixture["language"].as_str().unwrap();
        let code = std::fs::read_to_string(root.join("fixtures").join(path)).unwrap();
        let start = Instant::now();
        let gr = gpu.highlight(&code);
        let first_gpu = start.elapsed().as_secs_f64() * 1000.;
        let supported = ss.find_syntax_by_token(lang).is_some();
        let start = Instant::now();
        let sr = if measure_syntect {
            syntect_runs(&ss, &ts, lang, &code)
        } else {
            Vec::new()
        };
        let first_syn = start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(gr.last().map(|r| r.1), Some(code.len()));
        if supported && measure_syntect {
            assert_eq!(sr.last().map(|r| r.1), Some(code.len()));
        }
        let mut gt = Vec::new();
        let mut st = Vec::new();
        for i in 0..reps {
            for which in if i % 2 == 0 { [0, 1] } else { [1, 0] } {
                let start = Instant::now();
                if which == 0 {
                    black_box(gpu.highlight(black_box(&code)));
                    gt.push(start.elapsed().as_secs_f64() * 1000.)
                } else if supported && measure_syntect {
                    black_box(syntect_runs(&ss, &ts, lang, black_box(&code)));
                    st.push(start.elapsed().as_secs_f64() * 1000.)
                }
            }
        }
        let result = serde_json::json!({"file":path,"language":lang,"bytes":code.len(),"tokens":tokenizer::tokenize(&code).ranges.len(),"syntect_supported":supported,"syntect_measured":supported && measure_syntect,"zork_would_skip":!supported||code.len()>65536||code.lines().any(|l|l.len()>4096),"gpu_first_ms":first_gpu,"syntect_first_ms":(supported && measure_syntect).then_some(first_syn),"gpu":stats(&mut gt),"syntect":stats(&mut st),"gpu_runs":gr.len(),"syntect_runs":sr.len()});
        eprintln!(
            "{path}: GPU {:.3}ms / syntect {:?}ms",
            gt[gt.len() / 2],
            st.get(st.len() / 2)
        );
        results.push(result);
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"adapter":gpu.adapter(),"f16":gpu.f16(),"gpu_init_ms":gpu_init,"syntect_init_ms":syn_init,"repetitions":reps,"results":results})).unwrap());
}
